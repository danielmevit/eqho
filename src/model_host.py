"""Out-of-process model host.

The Whisper model runs in a CHILD process. Switching models kills the child and
spawns a fresh one — a fresh process only ever loads ONE model, which is the
only reliable path on CUDA stacks where loading a second model in-process
crashes natively (see docs/ai/GOTCHAS). The main app never loads a model, so a
native inference crash kills only the child (auto-respawned), never the app.

The inference BACKEND inside the child is pluggable:
- "faster-whisper" (CTranslate2): NVIDIA CUDA / CPU. Default today.
- "whisper.cpp": AMD/Intel via Vulkan, NVIDIA via CUDA, Apple via Metal, CPU.
  This is also the mobile engine and desktop Phase-8 target. Scaffolded here;
  requires a whisper.cpp Python binding + (for AMD) a Vulkan-enabled build.
"""

import logging
import multiprocessing as mp
import threading
import time
from typing import Optional

log = logging.getLogger(__name__)

_LOAD_TIMEOUT = 600.0   # model load may include a first-run download
_TRANSCRIBE_TIMEOUT = 120.0


# -- Child process -------------------------------------------------------------

def _host_worker(to_child, from_child, backend: str) -> None:
    """Runs in the child process: load one model, then serve transcribe calls."""
    import numpy as np  # noqa: F401 (ensures numpy is importable in the child)
    engine = None
    while True:
        try:
            msg = to_child.get()
        except Exception:
            break
        kind = msg[0]
        if kind == "shutdown":
            break
        if kind == "load":
            try:
                engine = _load_backend(backend, msg[1])
                from_child.put(("ready", engine.info))
            except Exception as e:
                from_child.put(("error", f"{type(e).__name__}: {e}"))
        elif kind == "transcribe":
            if engine is None:
                from_child.put(("error", "no model loaded"))
                continue
            try:
                segs = engine.transcribe(msg[1])
                from_child.put(("segments", segs))
            except Exception as e:
                from_child.put(("error", f"{type(e).__name__}: {e}"))


def _load_backend(backend: str, cfg: dict):
    if backend == "whisper.cpp":
        return _WhisperCppBackend(cfg)
    return _FasterWhisperBackend(cfg)


class _FasterWhisperBackend:
    """CTranslate2 backend — NVIDIA CUDA or CPU (no AMD)."""

    def __init__(self, cfg: dict):
        import os
        import numpy as np
        from faster_whisper import WhisperModel

        model_dir = cfg["model_dir"]
        target = cfg["model_size"]
        os.environ["HF_HUB_CACHE"] = str(model_dir) + "/huggingface"

        device, compute = self._pick(target)
        model = WhisperModel(target, device=device, compute_type=compute,
                             download_root=str(model_dir))
        # Verify CUDA / warm CPU with a dummy inference
        try:
            list(model.transcribe(np.zeros(16000, dtype=np.float32),
                                  language="en", beam_size=1)[0])
        except Exception as e:
            if device == "cuda":
                log.warning("CUDA inference failed (%s), falling back to CPU.", e)
                model = WhisperModel(target, device="cpu", compute_type="int8",
                                     download_root=str(model_dir))
                device, compute = "cpu", "int8"
        self._model = model
        self.info = {"backend": "faster-whisper", "device": device, "compute": compute}

    @staticmethod
    def _pick(model_size: str):
        if model_size == "large-v3":  # too big for 6 GB VRAM
            return "cpu", "int8"
        try:
            import ctranslate2
            if "int8_float16" in ctranslate2.get_supported_compute_types("cuda"):
                return "cuda", "int8_float16"
        except Exception:
            pass
        return "cpu", "int8"

    def transcribe(self, p: dict):
        segments, _ = self._model.transcribe(
            p["audio"], language=p["language"], beam_size=p["beam_size"],
            initial_prompt=p.get("initial_prompt") or None,
        )
        return [{"text": s.text, "no_speech_prob": s.no_speech_prob,
                 "avg_logprob": s.avg_logprob} for s in segments]


# Eqho model name -> whisper.cpp model name. whisper.cpp doesn't ship the
# distil variants, so they map to their closest standard model.
_WHISPERCPP_NAME = {
    "tiny": "tiny", "tiny.en": "tiny.en",
    "base": "base", "base.en": "base.en",
    "small": "small", "small.en": "small.en",
    "medium": "medium", "medium.en": "medium.en",
    "large-v3": "large-v3", "large-v3-turbo": "large-v3-turbo",
    "distil-large-v3": "large-v3-turbo",   # fast, near-large accuracy
    "distil-medium.en": "medium.en",
    "distil-small.en": "small.en",
}


class _WhisperCppBackend:
    """whisper.cpp backend via pywhispercpp — cross-vendor GPU (Vulkan on
    AMD/Intel/NVIDIA, Metal on Apple) or CPU. Same engine as Eqho Mobile.
    The GPU backend is chosen at pywhispercpp BUILD time (GGML_VULKAN=1)."""

    def __init__(self, cfg: dict):
        from pywhispercpp.model import Model  # requires pywhispercpp installed
        target = cfg["model_size"]
        name = _WHISPERCPP_NAME.get(target, "base")
        # pywhispercpp auto-downloads the GGML model by name to its own cache
        self._model = Model(name)
        self.info = {"backend": "whisper.cpp", "model": name}

    def transcribe(self, p: dict):
        lang = p["language"] or "auto"
        # pywhispercpp accepts a float32 mono 16 kHz numpy array directly
        segments = self._model.transcribe(p["audio"], language=lang)
        # whisper.cpp segments carry text only — no per-segment confidence, so
        # return safe defaults (the transcriber's confidence gate then never
        # wrongly drops them; the RMS gate + blocklist still apply).
        return [{"text": s.text, "no_speech_prob": 0.0, "avg_logprob": 0.0}
                for s in segments]


def resolve_backend(setting: str) -> str:
    """Turn the engine_backend setting into a concrete backend.
    "auto": NVIDIA+CUDA -> faster-whisper (fastest there); otherwise
    whisper.cpp if installed (AMD/Intel GPU or faster CPU), else faster-whisper."""
    if setting in ("faster-whisper", "whisper.cpp"):
        return setting
    try:
        import ctranslate2
        if "int8_float16" in ctranslate2.get_supported_compute_types("cuda"):
            return "faster-whisper"
    except Exception:
        pass
    try:
        import importlib.util
        if importlib.util.find_spec("pywhispercpp") is not None:
            return "whisper.cpp"
    except Exception:
        pass
    return "faster-whisper"


# -- Main process --------------------------------------------------------------

class ModelHost:
    """Main-process handle to the child model process."""

    def __init__(self, backend: str = "faster-whisper"):
        self._backend = backend
        self._ctx = mp.get_context("spawn")
        self._proc: Optional[mp.process.BaseProcess] = None
        self._to = None
        self._from = None
        self._current: Optional[str] = None
        self._info: dict = {}
        self._io = threading.Lock()

    def _spawn(self) -> None:
        self._to = self._ctx.Queue()
        self._from = self._ctx.Queue()
        self._proc = self._ctx.Process(
            target=_host_worker, args=(self._to, self._from, self._backend), daemon=True)
        self._proc.start()

    def _kill(self) -> None:
        if self._proc is not None:
            try:
                self._proc.terminate()  # kills the process → frees VRAM cleanly
                self._proc.join(timeout=5)
            except Exception:
                pass
        self._proc = None
        self._current = None

    def _recv(self, timeout: float):
        """Block for a reply, but bail if the child dies."""
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            try:
                return self._from.get(timeout=1.0)
            except Exception:
                if self._proc is None or not self._proc.is_alive():
                    return ("error", "model process died")
        return ("error", "timeout")

    def load_model(self, model_size: str, model_dir) -> bool:
        """Ensure `model_size` is loaded in a fresh child. Returns True on success."""
        with self._io:
            if self._current == model_size and self._proc and self._proc.is_alive():
                return True
            self._kill()  # fresh process per model = the reliable path
            self._spawn()
            log.info("Model host: loading %s (backend=%s)", model_size, self._backend)
            self._to.put(("load", {"model_size": model_size, "model_dir": str(model_dir)}))
            kind, payload = self._recv(_LOAD_TIMEOUT)
            if kind == "ready":
                self._current = model_size
                self._info = payload
                log.info("Model host ready: %s %s", model_size, payload)
                return True
            log.error("Model host load failed: %s", payload)
            self._kill()
            return False

    def transcribe(self, audio, language: str, beam_size: int,
                   initial_prompt: str = "") -> list:
        with self._io:
            if not self._proc or not self._proc.is_alive():
                return []
            self._to.put(("transcribe", {
                "audio": audio, "language": language, "beam_size": beam_size,
                "initial_prompt": initial_prompt,
            }))
            kind, payload = self._recv(_TRANSCRIBE_TIMEOUT)
            if kind == "segments":
                return payload
            log.error("Model host transcribe failed: %s", payload)
            return []

    @property
    def info(self) -> dict:
        return dict(self._info)

    def is_ready(self, model_size: str) -> bool:
        return (self._current == model_size and self._proc is not None
                and self._proc.is_alive())

    def shutdown(self) -> None:
        with self._io:
            if self._proc is not None and self._to is not None:
                try:
                    self._to.put(("shutdown",))
                except Exception:
                    pass
            self._kill()
