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
import os
import sys
import threading
import time
from typing import Optional

log = logging.getLogger(__name__)


def _cpu_threads() -> int:
    """Physical-core estimate for CPU inference: os.cpu_count() // 2 (SMT), clamped to 4..16."""
    count = os.cpu_count() or 4
    return max(4, min(16, count // 2))

_LOAD_INACTIVITY_TIMEOUT = 120.0  # a load fails only after this long with NO
                                  # progress message from the child — a slow
                                  # first-run download that keeps reporting
                                  # progress never times out (F8)
_TRANSCRIBE_TIMEOUT = 120.0


class ModelHostError(RuntimeError):
    """A model-host request failed (child dead, timeout, or an error reply).

    On timeout/error the child is killed before this is raised, so no stale
    reply can ever be read by a later request."""


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
                engine = _load_backend(
                    backend, msg[1],
                    progress=lambda m: from_child.put(("progress", m)))
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


def _load_backend(backend: str, cfg: dict, progress=None):
    if backend == "whisper.cpp":
        return _WhisperCppBackend(cfg, progress)
    return _FasterWhisperBackend(cfg, progress)


def _progress_tqdm(report):
    """A tqdm subclass that forwards HF-hub download progress to the parent
    process (used as snapshot_download's tqdm_class)."""
    from tqdm import tqdm

    class _ReportingTqdm(tqdm):
        def update(self, n=1):
            super().update(n)
            now = time.monotonic()
            if self.n > 0 and now - getattr(self, "_eqho_last", 0.0) >= 2.0:
                self._eqho_last = now
                if self.total:
                    report("downloading model %d%%"
                           % min(100, int(100 * self.n / self.total)))
                else:
                    report("downloading model %d MiB" % (self.n // 1048576))

    return _ReportingTqdm


def _download_model_files(target: str, model_dir, report) -> str:
    """Resolve `target` to a local CTranslate2 model dir, downloading from the
    HF hub with progress reports when not cached. faster-whisper's
    download_model() hardcodes a silent tqdm, so the uncached path calls
    huggingface_hub.snapshot_download directly with the same allow_patterns
    plus a tqdm subclass that reports bytes to the parent — this is what keeps
    the load timeout inactivity-based during a first-run download."""
    from faster_whisper.utils import download_model
    try:
        return download_model(target, cache_dir=str(model_dir), local_files_only=True)
    except Exception:
        pass  # not cached yet — download below
    repo_id = None
    try:
        import faster_whisper.utils as fw_utils
        repo_id = fw_utils._MODELS.get(target, target)
    except Exception:
        pass
    if repo_id:
        import huggingface_hub
        return huggingface_hub.snapshot_download(
            repo_id,
            allow_patterns=["config.json", "preprocessor_config.json",
                            "model.bin", "tokenizer.json", "vocabulary.*"],
            cache_dir=str(model_dir),
            tqdm_class=_progress_tqdm(report),
        )
    # faster-whisper's private API drifted — download without progress reports.
    report("downloading model files (no progress available)")
    return download_model(target, cache_dir=str(model_dir))


class _FasterWhisperBackend:
    """CTranslate2 backend — NVIDIA CUDA or CPU (no AMD)."""

    def __init__(self, cfg: dict, progress=None):
        import os
        import numpy as np
        from faster_whisper import WhisperModel

        report = progress or (lambda m: None)
        model_dir = cfg["model_dir"]
        target = cfg["model_size"]
        os.environ["HF_HUB_CACHE"] = str(model_dir) + "/huggingface"

        device, compute = self._pick(target)
        report("downloading model files")
        model_path = _download_model_files(target, model_dir, report)
        report(f"loading model on {device} ({compute})")
        model_kwargs = {}
        if device == "cpu":
            threads = _cpu_threads()
            model_kwargs["cpu_threads"] = threads
            log.info("Loading faster-whisper on CPU with %d threads", threads)
        model = WhisperModel(model_path, device=device, compute_type=compute, **model_kwargs)
        # Verify CUDA / warm CPU with a dummy inference
        try:
            report("warming up")
            list(model.transcribe(np.zeros(16000, dtype=np.float32),
                                  language="en", beam_size=1)[0])
        except Exception as e:
            if device == "cuda":
                log.warning("CUDA inference failed (%s), falling back to CPU.", e)
                report("CUDA failed — reloading on CPU")
                threads = _cpu_threads()
                log.info("Loading faster-whisper on CPU with %d threads (CUDA fallback)", threads)
                model = WhisperModel(model_path, device="cpu", compute_type="int8", cpu_threads=threads)
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


def whispercpp_models_dir(model_dir) -> "Path":
    """whisper.cpp GGML models live next to Eqho's other models."""
    from pathlib import Path
    return Path(model_dir) / "whispercpp"


def whispercpp_model_path(model_dir, model_key: str) -> "Path":
    """File pywhispercpp downloads for `model_key` (ggml-<name>.bin)."""
    name = _WHISPERCPP_NAME.get(model_key, "base")
    return whispercpp_models_dir(model_dir) / f"ggml-{name}.bin"


class _WhisperCppBackend:
    """whisper.cpp backend via pywhispercpp — cross-vendor GPU (Vulkan on
    AMD/Intel/NVIDIA, Metal on Apple) or CPU. Same engine as Eqho Mobile.
    The GPU backend is chosen at pywhispercpp BUILD time (GGML_VULKAN=1)."""

    def __init__(self, cfg: dict, progress=None):
        report = progress or (lambda m: None)
        from pywhispercpp.model import Model  # requires pywhispercpp installed
        target = cfg["model_size"]
        name = _WHISPERCPP_NAME.get(target, "base")
        # pywhispercpp auto-downloads the GGML model by name to its own cache
        # and offers no progress hook — coarse phase messages are the best
        # available (a stalled download still hits the inactivity timeout).
        report("downloading whisper.cpp model")
        models_dir = whispercpp_models_dir(cfg["model_dir"])
        models_dir.mkdir(parents=True, exist_ok=True)
        self._model = Model(name, models_dir=str(models_dir))
        report("whisper.cpp model loaded")
        self.info = {"backend": "whisper.cpp", "model": name}

    def transcribe(self, p: dict):
        lang = p["language"] or "auto"
        # pywhispercpp accepts a float32 mono 16 kHz numpy array directly
        kwargs = {"language": lang}
        if p.get("initial_prompt"):
            kwargs["initial_prompt"] = p["initial_prompt"]
        # Greedy decoding on purpose (no beam settings): slow GPUs can't afford beam search.
        segments = self._model.transcribe(p["audio"], **kwargs)
        # whisper.cpp segments carry text only — no per-segment confidence, so
        # return safe defaults (the transcriber's confidence gate then never
        # wrongly drops them; the RMS gate + blocklist still apply).
        return [{"text": s.text, "no_speech_prob": 0.0, "avg_logprob": 0.0}
                for s in segments]


def _cpu_has_avx2() -> bool:
    """True if the CPU can run the bundled whisper.cpp build. Its CPU part
    (ggml-cpu.dll) is compiled for AVX2 + FMA + F16C and would crash on CPUs
    without AVX2 (e.g. Celeron / Pentium Silver), so on Windows ask the
    kernel whether AVX2 is present (PF_AVX2_INSTRUCTIONS_AVAILABLE = 40).
    Other OSes keep today's behaviour (no wheel is shipped there)."""
    if sys.platform != "win32":
        return True
    try:
        import ctypes
        return bool(ctypes.windll.kernel32.IsProcessorFeaturePresent(40))
    except Exception:
        return False


_MIN_GPU_MEMORY = 2 * 1024 ** 3   # own (dedicated) memory a GPU needs for auto
_adapters_cache: Optional[list] = None


def _list_adapters_dxgi() -> list:
    """Graphics adapters via DXGI (ctypes, Windows only) as dicts
    {name, vendor, dedicated}; software adapters are skipped. Raises on failure."""
    import ctypes
    from ctypes import wintypes

    class GUID(ctypes.Structure):
        _fields_ = [("d1", ctypes.c_uint32), ("d2", ctypes.c_uint16),
                    ("d3", ctypes.c_uint16), ("d4", ctypes.c_ubyte * 8)]

    class DESC1(ctypes.Structure):
        _fields_ = [("Description", ctypes.c_wchar * 128),
                    ("VendorId", ctypes.c_uint), ("DeviceId", ctypes.c_uint),
                    ("SubSysId", ctypes.c_uint), ("Revision", ctypes.c_uint),
                    ("DedicatedVideoMemory", ctypes.c_size_t),
                    ("DedicatedSystemMemory", ctypes.c_size_t),
                    ("SharedSystemMemory", ctypes.c_size_t),
                    ("LuidLow", wintypes.DWORD), ("LuidHigh", wintypes.LONG),
                    ("Flags", ctypes.c_uint)]

    # IID_IDXGIFactory1 {770aae78-f26f-4dba-a829-253c83d1b387}
    iid = GUID(0x770AAE78, 0xF26F, 0x4DBA,
               (ctypes.c_ubyte * 8)(0xA8, 0x29, 0x25, 0x3C, 0x83, 0xD1, 0xB3, 0x87))
    HRESULT = ctypes.c_long

    def method(obj, index, *argtypes):
        vtbl = ctypes.cast(obj, ctypes.POINTER(ctypes.POINTER(ctypes.c_void_p)))[0]
        return ctypes.WINFUNCTYPE(HRESULT, ctypes.c_void_p, *argtypes)(vtbl[index])

    def release(obj):
        vtbl = ctypes.cast(obj, ctypes.POINTER(ctypes.POINTER(ctypes.c_void_p)))[0]
        ctypes.WINFUNCTYPE(ctypes.c_ulong, ctypes.c_void_p)(vtbl[2])(obj)

    create = ctypes.windll.dxgi.CreateDXGIFactory1
    create.restype = HRESULT
    create.argtypes = [ctypes.POINTER(GUID), ctypes.POINTER(ctypes.c_void_p)]
    factory = ctypes.c_void_p()
    hr = create(ctypes.byref(iid), ctypes.byref(factory))
    if hr < 0 or not factory.value:
        raise OSError(f"CreateDXGIFactory1 failed (0x{hr & 0xFFFFFFFF:08X})")
    adapters = []
    try:
        enum1 = method(factory, 12, ctypes.c_uint, ctypes.POINTER(ctypes.c_void_p))
        i = 0
        while i < 64:
            adapter = ctypes.c_void_p()
            hr = enum1(factory, i, ctypes.byref(adapter))
            if hr < 0 or not adapter.value:  # DXGI_ERROR_NOT_FOUND ends the list
                break
            try:
                desc = DESC1()
                get_desc1 = method(adapter, 10, ctypes.POINTER(DESC1))
                if get_desc1(adapter, ctypes.byref(desc)) >= 0:
                    software = bool(desc.Flags & 2) or desc.VendorId == 0x1414
                    if not software:
                        adapters.append({"name": desc.Description,
                                         "vendor": desc.VendorId,
                                         "dedicated": int(desc.DedicatedVideoMemory)})
            finally:
                release(adapter)
            i += 1
    finally:
        release(factory)
    return adapters


def _gpu_adapters() -> list:
    """Hardware graphics adapters, queried once per process. A failed query
    counts as "no graphics card" (empty list)."""
    global _adapters_cache
    if _adapters_cache is None:
        try:
            _adapters_cache = _list_adapters_dxgi()
        except Exception as e:
            log.debug("DXGI adapter query failed: %s", e)
            _adapters_cache = []
    return _adapters_cache


_cuda_cache = None   # (ctranslate2 module, result)


def _cuda_available() -> bool:
    """NVIDIA+CUDA usable? Checked once per process (hardware doesn't change);
    re-checked only if a different ctranslate2 module is loaded (probes)."""
    global _cuda_cache
    mod = sys.modules.get("ctranslate2")
    if _cuda_cache is not None and mod is not None and _cuda_cache[0] is mod:
        return _cuda_cache[1]
    try:
        import ctranslate2
        ok = "int8_float16" in ctranslate2.get_supported_compute_types("cuda")
    except Exception:
        ctranslate2, ok = None, False
    _cuda_cache = (ctranslate2, ok)
    return ok


_auto_logged = False


def resolve_backend(setting: str) -> str:
    """Turn the engine_backend setting into a concrete backend.
    "auto": NVIDIA+CUDA -> faster-whisper (fastest there); otherwise
    whisper.cpp if installed AND (on Windows) the CPU has AVX2 — the bundled
    Vulkan wheel's CPU part needs that baseline — AND (on Windows) a graphics
    card with >= 2 GiB of its own memory: on built-in graphics faster-whisper
    on the CPU is faster (Intel UHD 620 benchmark); else faster-whisper."""
    global _auto_logged
    if setting in ("faster-whisper", "whisper.cpp"):
        return setting
    if _cuda_available():
        return "faster-whisper"
    choice = "faster-whisper"
    adapters = None
    try:
        import importlib.util
        if (importlib.util.find_spec("pywhispercpp") is not None
                and _cpu_has_avx2()):
            if sys.platform == "win32":
                adapters = _gpu_adapters()
                if any(a["dedicated"] >= _MIN_GPU_MEMORY for a in adapters):
                    choice = "whisper.cpp"
            else:
                choice = "whisper.cpp"
    except Exception:
        pass
    if not _auto_logged:
        _auto_logged = True
        seen = ("n/a" if adapters is None else ", ".join(
            f"{a['name']} {a['dedicated'] // (1024 * 1024)} MB" for a in adapters) or "none")
        log.info("Engine auto: graphics adapters [%s] -> %s", seen, choice)
    return choice


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
        # The child's error text from the last failed load ("" after a
        # successful load or before any load).
        self.last_load_error: str = ""

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

    def _recv_load(self, inactivity_timeout: float):
        """Wait for a load reply. The child emits ("progress", msg) notes
        while downloading/loading/warming; each one resets the inactivity
        deadline, so a slow first-run download never times out as long as it
        keeps making progress. Only `inactivity_timeout` seconds of SILENCE
        fail the load."""
        deadline = time.monotonic() + inactivity_timeout
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                return ("error", "timeout (no progress from model process)")
            try:
                msg = self._from.get(timeout=min(1.0, remaining))
            except Exception:
                if self._proc is None or not self._proc.is_alive():
                    return ("error", "model process died")
                continue
            if msg[0] == "progress":
                log.info("Model host load: %s", msg[1])
                deadline = time.monotonic() + inactivity_timeout
                continue
            return msg

    def load_model(self, model_size: str, model_dir) -> bool:
        """Ensure `model_size` is loaded in a fresh child. Returns True on
        success. The wait is inactivity-based: the child reports progress
        while downloading/loading, and the load only fails after
        `_LOAD_INACTIVITY_TIMEOUT` seconds with no progress message."""
        with self._io:
            if self._current == model_size and self._proc and self._proc.is_alive():
                self.last_load_error = ""
                return True
            self.last_load_error = ""
            self._kill()  # fresh process per model = the reliable path
            self._spawn()
            log.info("Model host: loading %s (backend=%s)", model_size, self._backend)
            self._to.put(("load", {"model_size": model_size, "model_dir": str(model_dir)}))
            kind, payload = self._recv_load(_LOAD_INACTIVITY_TIMEOUT)
            if kind == "ready":
                self._current = model_size
                self._info = payload
                log.info("Model host ready: %s %s", model_size, payload)
                return True
            self.last_load_error = str(payload) or "load failed"
            log.error("Model host load failed: %s", payload)
            self._kill()
            return False

    def transcribe(self, audio, language: str, beam_size: int,
                   initial_prompt: str = "") -> list:
        with self._io:
            if not self._proc or not self._proc.is_alive():
                raise ModelHostError("model process is not running")
            self._to.put(("transcribe", {
                "audio": audio, "language": language, "beam_size": beam_size,
                "initial_prompt": initial_prompt,
            }))
            kind, payload = self._recv(_TRANSCRIBE_TIMEOUT)
            if kind == "segments":
                return payload
            # Timeout or an error reply: the child's state is unknown — its
            # late reply would otherwise be read by the NEXT transcribe
            # (every answer one dictation behind, forever). Kill the child so
            # no stale reply can ever surface; the next load_model() respawns
            # a fresh one. Report the failure to the caller instead of
            # returning empty text.
            self._kill()
            log.error("Model host transcribe failed: %s", payload)
            raise ModelHostError(f"transcribe failed: {payload}")

    @property
    def info(self) -> dict:
        return dict(self._info)

    def is_ready(self, model_size: str) -> bool:
        return (self._current == model_size and self._proc is not None
                and self._proc.is_alive())

    @property
    def backend(self) -> str:
        return self._backend

    def set_backend(self, backend: str) -> None:
        """Switch the inference backend (e.g. faster-whisper <-> whisper.cpp).
        Kills the current child so the next load_model() spawns a fresh one on
        the new backend."""
        with self._io:
            if backend != self._backend:
                self._kill()
                self._backend = backend

    def shutdown(self) -> None:
        with self._io:
            if self._proc is not None and self._to is not None:
                try:
                    self._to.put(("shutdown",))
                except Exception:
                    pass
            self._kill()
