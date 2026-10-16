"""Probe: low-free-memory warning and plain out-of-memory message (EQ-LOWMEM).

Uses FAKES (no real model): a fake OsKit free-memory reading and a fake
model host. Checks:
- low memory on the processor -> one warning with the rounded numbers, and
  the load still happens;
- enough memory -> no warning;
- CUDA -> no memory check at all;
- a load failing with "mkl_malloc: failed to allocate memory" -> the plain
  message (both as an error reply and as a raised error);
- any other load error -> unchanged "Model failed to load — ..." text;
- the real free_memory_mb() on this PC returns a plausible number.

One line per check; exit 1 on any failure. Run with the project venv:

    venv/Scripts/python.exe tools/probe_lowmem.py
"""

import logging
import sys
from pathlib import Path
from types import SimpleNamespace

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import _probe_env  # noqa: F401,E402  isolated EQHO_CONFIG_DIR before any src import

import src.model_host as model_host_mod
_RealModelHost = model_host_mod.ModelHost
import src.oskit as oskit_mod
import src.transcriber as transcriber_mod

_real_get = oskit_mod.get

# Keep log output quiet. The transcriber reads the host's last_load_error,
# never its log lines.
logging.basicConfig(level=logging.INFO, handlers=[logging.NullHandler()])
sys.stdout.reconfigure(errors="backslashreplace")

failures = 0


def check(name: str, ok: bool, detail: str = "") -> None:
    global failures
    if not ok:
        failures += 1
    print(f"[{'PASS' if ok else 'FAIL'}] {name}" + (f" - {detail}" if detail else ""))


class FakeKit:
    def __init__(self, free_mb):
        self.free_mb = free_mb
        self.calls = 0

    def free_memory_mb(self):
        self.calls += 1
        return self.free_mb

    def disable_os_mic_ducking(self):
        pass


class FakeHost:
    """mode: "ok", "reply" (error reply like the real host), "raise"."""

    def __init__(self, backend="faster-whisper"):
        self.backend = backend
        self.loads = 0
        self.mode = "ok"
        self.error = ""
        self.info = {}
        self.last_load_error = ""
        self.log_line = True
        self._ready = None

    def is_ready(self, model_size):
        return self._ready == model_size

    def set_backend(self, backend):
        self.backend = backend

    def load_model(self, model_size, model_dir):
        self.loads += 1
        if self.mode == "raise":
            raise model_host_mod.ModelHostError(self.error)
        if self.mode == "reply":
            self.last_load_error = self.error
            if self.log_line:
                model_host_mod.log.error("Model host load failed: %s", self.error)
            return False
        self.last_load_error = ""
        self._ready = model_size
        return True


def make(free_mb, cuda=False, backend="faster-whisper", model="distil-large-v3"):
    kit = FakeKit(free_mb)
    oskit_mod.get = lambda: kit
    model_host_mod._cuda_available = lambda: cuda
    model_host_mod.resolve_backend = lambda setting: backend
    model_host_mod.ModelHost = FakeHost
    settings = SimpleNamespace(model_size=model, engine_backend=backend,
                               resolve_model_dir=lambda: Path("."))
    tr = transcriber_mod.VoiceTranscriber(settings)
    notices = []
    aborts = []
    tr.set_callbacks(on_partial=lambda t: None, on_complete=lambda t: None,
                     on_abort=lambda r, s: aborts.append(r),
                     on_notice=notices.append)
    return tr, kit, notices, aborts


def abort_reason(tr, aborts):
    tr._running = True
    tr._transcription_loop(tr.current_session())
    return aborts[-1] if aborts else None


# 1. Low memory on the processor: 0.8 GB free, distil-large-v3 needs
#    1510*1.3+300 = 2263 MB -> "about 2.2 GB".
tr, kit, notices, _ = make(free_mb=820)
tr._ensure_model()
expected = ("Your computer is short on free memory (about 0.8 GB free; this speech "
            "model needs about 2.2 GB). Close some programs, or pick a smaller "
            "model in Eqho → Models.")
check("low memory -> one warning", len(notices) == 1, repr(notices))
check("warning has the rounded numbers", notices[:1] == [expected], repr(notices[:1]))
check("load still happens", tr._host.loads == 1 and tr._model_loaded)

# 1b. whisper.cpp also runs on the PC's memory.
tr, kit, notices, _ = make(free_mb=500, backend="whisper.cpp", model="small")
tr._ensure_model()
check("whisper.cpp low memory -> warning", len(notices) == 1, repr(notices))

# 2. Enough memory -> no warning.
tr, kit, notices, _ = make(free_mb=8000)
tr._ensure_model()
check("enough memory -> no warning", notices == [] and tr._host.loads == 1)

# 3. CUDA (faster-whisper on NVIDIA) -> no check.
tr, kit, notices, _ = make(free_mb=100, cuda=True)
tr._ensure_model()
check("CUDA -> no memory check", kit.calls == 0 and notices == [], f"calls={kit.calls}")

# 4. Out-of-memory load failure -> plain message (error reply path).
plain = transcriber_mod.LOW_MEMORY_LOAD_TEXT
tr, kit, notices, aborts = make(free_mb=8000)
tr._host.mode, tr._host.error = "reply", "RuntimeError: mkl_malloc: failed to allocate memory"
reason = abort_reason(tr, aborts)
check("mkl_malloc reply -> plain message", reason == plain, repr(reason))

# 4b. ... raised error path.
tr, kit, notices, aborts = make(free_mb=8000)
tr._host.mode, tr._host.error = "raise", "MKL_MALLOC: Failed to allocate memory"
reason = abort_reason(tr, aborts)
check("mkl_malloc raised -> plain message", reason == plain, repr(reason))

# 4c. The host logs nothing at all -> plain message still appears (the
#     result can't depend on the wording of a log line).
tr, kit, notices, aborts = make(free_mb=8000)
tr._host.mode, tr._host.error = "reply", "RuntimeError: mkl_malloc: failed to allocate memory"
tr._host.log_line = False
reason = abort_reason(tr, aborts)
check("no host log line -> plain message", reason == plain, repr(reason))

# 4d. The real ModelHost starts with an empty last_load_error.
check("ModelHost.last_load_error starts empty",
      _RealModelHost().last_load_error == "")

# 5. Any other error -> unchanged behaviour.
tr, kit, notices, aborts = make(free_mb=8000)
tr._host.mode, tr._host.error = "reply", "OSError: model file is corrupt"
reason = abort_reason(tr, aborts)
check("other error -> unchanged message",
      reason == "Model failed to load — Model host failed to load distil-large-v3",
      repr(reason))

# 6. Real free_memory_mb() on this PC.
oskit_mod.get = _real_get
real = _real_get().free_memory_mb()
check("real free_memory_mb() plausible",
      real is not None and 16 <= real <= 4 * 1024 * 1024, f"{real} MB")

sys.exit(1 if failures else 0)
