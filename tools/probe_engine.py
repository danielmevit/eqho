"""Probe: engine auto-selection and the auto fallback (Task EQ-AMD).

No model load, no GPU, no microphone needed — ctranslate2 / find_spec / the
AVX2 check are monkeypatched, and the fallback runs against a fake model host.

Checks (src/model_host.py resolve_backend + src/transcriber.py):
1. CUDA available -> faster-whisper.
2. No CUDA + pywhispercpp installed + AVX2 -> whisper.cpp.
3. No AVX2 (Windows) -> faster-whisper even with pywhispercpp installed.
4. No pywhispercpp -> faster-whisper.
5. Explicit settings ("faster-whisper" / "whisper.cpp") pass through untouched.
6. Auto fallback: a host whose whisper.cpp load raises -> falls back to
   faster-whisper once, dictation still works.
7. Explicit whisper.cpp keeps today's behaviour: the load error surfaces,
   no silent fallback.
8. Auto vs. graphics adapters (EQ-SLOWPC, fake DXGI lists): 128 MB Intel ->
   faster-whisper; 8 GB AMD -> whisper.cpp; none / DXGI failure ->
   faster-whisper; CUDA -> faster-whisper. The real DXGI query runs once.
9. whisper.cpp models live under <model_dir>/whispercpp/ and is_model_cached
   follows that file (temp dir, fake file, no download).

Standard library + src only. Runs with plain python:
    python tools/probe_engine.py
Exit 0 on success ("probe_engine: ALL OK"), 1 on failure.
"""

import importlib.util
import sys
import threading
import types
from pathlib import Path

# Ensure repo root is on sys.path
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import _probe_env  # noqa: F401,E402  isolated EQHO_CONFIG_DIR before any src import

import src.model_host as model_host
from src.model_host import ModelHostError, resolve_backend
from src.transcriber import VoiceTranscriber


_INTEL_128MB = {"name": "Intel(R) UHD Graphics 620", "vendor": 0x8086,
                "dedicated": 128 * 1024 ** 2}
_AMD_8GB = {"name": "AMD Radeon RX 6600", "vendor": 0x1002,
            "dedicated": 8 * 1024 ** 3}


class _Patches:
    """Scoped monkeypatcher: fake ctranslate2, fake find_spec, fake AVX2."""

    def __init__(self, cuda_types=None, pywhispercpp=True, avx2=True,
                 adapters=None):
        self.cuda_types = cuda_types          # None -> ctranslate2 missing/broken
        # fake DXGI list; None -> one 8 GB AMD card, "fail" -> query raises
        self.adapters = [_AMD_8GB] if adapters is None else adapters
        self.pywhispercpp = pywhispercpp
        self.avx2 = avx2
        self._saved = {}

    def __enter__(self):
        # ctranslate2: inject a fake module (or one whose probe of CUDA raises,
        # which exercises the same except-branch as a missing import)
        self._saved["ctranslate2"] = sys.modules.get("ctranslate2")
        fake = types.ModuleType("ctranslate2")
        if self.cuda_types is None:
            def _broken(device):
                raise ImportError("no ctranslate2")
            fake.get_supported_compute_types = _broken
        else:
            fake.get_supported_compute_types = lambda device: list(self.cuda_types)
        sys.modules["ctranslate2"] = fake
        # find_spec: point at a dummy spec (or None)
        self._saved["find_spec"] = importlib.util.find_spec
        spec = types.SimpleNamespace() if self.pywhispercpp else None
        importlib.util.find_spec = lambda name: spec if name == "pywhispercpp" \
            else self._saved["find_spec"](name)
        # AVX2 check
        self._saved["avx2"] = model_host._cpu_has_avx2
        model_host._cpu_has_avx2 = lambda: self.avx2
        # DXGI adapter list (Windows rule applies on every OS in the probe)
        self._saved["dxgi"] = model_host._list_adapters_dxgi
        self._saved["platform"] = sys.platform

        def _fake_dxgi():
            if self.adapters == "fail":
                raise OSError("fake DXGI failure")
            return list(self.adapters)
        model_host._list_adapters_dxgi = _fake_dxgi
        model_host._adapters_cache = None
        sys.platform = "win32"
        return self

    def __exit__(self, *exc):
        importlib.util.find_spec = self._saved["find_spec"]
        model_host._cpu_has_avx2 = self._saved["avx2"]
        model_host._list_adapters_dxgi = self._saved["dxgi"]
        model_host._adapters_cache = None
        sys.platform = self._saved["platform"]
        if self._saved["ctranslate2"] is None:
            sys.modules.pop("ctranslate2", None)
        else:
            sys.modules["ctranslate2"] = self._saved["ctranslate2"]
        return False


def check(name, cond, detail=""):
    if not cond:
        raise AssertionError(f"{name}: {detail}")
    print(f"  ok: {name}")


def test_resolve_backend():
    print("Testing resolve_backend auto selection...")

    with _Patches(cuda_types=["int8_float16"], pywhispercpp=True, avx2=True):
        check("CUDA ok -> faster-whisper",
              resolve_backend("auto") == "faster-whisper",
              f"got {resolve_backend('auto')}")

    with _Patches(cuda_types=["int8"], pywhispercpp=True, avx2=True):
        check("no CUDA + pywhispercpp + AVX2 -> whisper.cpp",
              resolve_backend("auto") == "whisper.cpp",
              f"got {resolve_backend('auto')}")

    with _Patches(cuda_types=["int8"], pywhispercpp=True, avx2=False):
        check("no AVX2 -> faster-whisper",
              resolve_backend("auto") == "faster-whisper",
              f"got {resolve_backend('auto')}")

    with _Patches(cuda_types=["int8"], pywhispercpp=False, avx2=True):
        check("no pywhispercpp -> faster-whisper",
              resolve_backend("auto") == "faster-whisper",
              f"got {resolve_backend('auto')}")

    with _Patches(cuda_types=None, pywhispercpp=False, avx2=False):
        check("no ctranslate2 + nothing installed -> faster-whisper",
              resolve_backend("auto") == "faster-whisper",
              f"got {resolve_backend('auto')}")

    with _Patches(cuda_types=["int8_float16"], pywhispercpp=True, avx2=True):
        check("explicit faster-whisper passes through",
              resolve_backend("faster-whisper") == "faster-whisper")
        check("explicit whisper.cpp passes through",
              resolve_backend("whisper.cpp") == "whisper.cpp")

    with _Patches(cuda_types=["int8"], pywhispercpp=False, avx2=False):
        check("explicit whisper.cpp passes through even when not installed",
              resolve_backend("whisper.cpp") == "whisper.cpp")

    print("  -> resolve_backend tests passed.")


def test_adapters():
    print("Testing auto vs. graphics adapters (EQ-SLOWPC)...")
    cases = [
        ("[128 MB Intel] -> faster-whisper", ["int8"], [_INTEL_128MB], "faster-whisper"),
        ("[8 GB AMD] -> whisper.cpp", ["int8"], [_AMD_8GB], "whisper.cpp"),
        ("[128 MB Intel, 8 GB AMD] -> whisper.cpp", ["int8"],
         [_INTEL_128MB, _AMD_8GB], "whisper.cpp"),
        ("[] -> faster-whisper", ["int8"], [], "faster-whisper"),
        ("DXGI failure -> faster-whisper", ["int8"], "fail", "faster-whisper"),
        ("CUDA present + 8 GB AMD -> faster-whisper", ["int8_float16"],
         [_AMD_8GB], "faster-whisper"),
    ]
    for name, cuda, adapters, want in cases:
        with _Patches(cuda_types=cuda, pywhispercpp=True, avx2=True, adapters=adapters):
            got = resolve_backend("auto")
            check(name, got == want, f"got {got}")

    with _Patches(cuda_types=["int8"], adapters=[_AMD_8GB]):
        calls = []
        real = model_host._list_adapters_dxgi
        model_host._list_adapters_dxgi = lambda: calls.append(1) or real()
        resolve_backend("auto")
        resolve_backend("auto")
        check("DXGI queried once per process (cached)", len(calls) == 1, f"{calls}")

    if sys.platform == "win32":
        try:
            found = model_host._list_adapters_dxgi()
            seen = ", ".join(f"{a['name']} {a['dedicated'] // 1024 ** 2} MB "
                             f"(vendor 0x{a['vendor']:04X})" for a in found) or "none"
            print(f"  info: real DXGI query found: {seen}")
            check("real DXGI query returns a list", isinstance(found, list))
        except Exception as e:
            print(f"  info: real DXGI query failed ({e}) -> auto treats it as no card")
    else:
        print("  info: real DXGI query skipped (not Windows)")

    print("  -> adapter tests passed.")


def test_whispercpp_model_dir():
    print("Testing whisper.cpp model folder + is_model_cached...")
    import tempfile
    import src.settings as settings_mod
    from src.settings import Settings, is_model_cached

    with tempfile.TemporaryDirectory() as td:
        path = model_host.whispercpp_model_path(td, "small")
        check("whisper.cpp model path under <model_dir>/whispercpp/",
              path.parent == Path(td) / "whispercpp" and path.name == "ggml-small.bin",
              str(path))
        check("distil model maps to its whisper.cpp file",
              model_host.whispercpp_model_path(td, "distil-large-v3").name
              == "ggml-large-v3-turbo.bin")
        s = Settings()
        s.engine_backend = "whisper.cpp"
        s.resolve_model_dir = lambda: Path(td)
        settings_mod._cache_check_memo.clear()
        check("is_model_cached False before the file exists",
              not is_model_cached(s, "small"))
        path.parent.mkdir(parents=True)
        path.write_bytes(b"fake")
        settings_mod._cache_check_memo.clear()
        check("is_model_cached True once the file exists", is_model_cached(s, "small"))
        check("other models still absent", not is_model_cached(s, "base"))
        settings_mod._cache_check_memo.clear()

    print("  -> model folder tests passed.")


class _FakeHost:
    """Model-host stand-in: whisper.cpp load raises (or returns False),
    faster-whisper load succeeds. Records every load and backend switch."""

    def __init__(self, backend="whisper.cpp", raise_on_fail=True):
        self.backend = backend
        self.raise_on_fail = raise_on_fail
        self.loads = []
        self.info = {"backend": backend}

    def is_ready(self, target):
        return False

    def load_model(self, target, model_dir):
        self.loads.append(self.backend)
        if self.backend == "whisper.cpp":
            if self.raise_on_fail:
                raise ModelHostError("whisper.cpp load crashed (child died)")
            return False
        return True

    def set_backend(self, backend):
        self.backend = backend


def _bare_transcriber(host, auto_engine):
    """A VoiceTranscriber without running __init__ (no mic, no oskit, no
    settings side effects) — just the attributes _ensure_model touches."""
    t = object.__new__(VoiceTranscriber)
    t._settings = types.SimpleNamespace(
        model_size="tiny", resolve_model_dir=lambda: Path("."))
    t._host = host
    t._model_lock = threading.Lock()
    t._model_dir = Path(".")
    t._model_loaded = False
    t._current_model_size = None
    t._auto_engine = auto_engine
    t._auto_fallback_used = False
    return t


def test_auto_fallback():
    print("Testing auto fallback to faster-whisper...")

    # Auto engine + whisper.cpp load raises -> falls back, dictation works
    host = _FakeHost("whisper.cpp", raise_on_fail=True)
    t = _bare_transcriber(host, auto_engine=True)
    t._ensure_model()
    check("auto + whisper.cpp load raises -> faster-whisper used",
          host.backend == "faster-whisper" and t._model_loaded,
          f"backend={host.backend} loaded={t._model_loaded}")
    check("fallback loaded faster-whisper exactly once",
          host.loads == ["whisper.cpp", "faster-whisper"], f"loads={host.loads}")

    # Auto engine + whisper.cpp load returns False -> same fallback
    host = _FakeHost("whisper.cpp", raise_on_fail=False)
    t = _bare_transcriber(host, auto_engine=True)
    t._ensure_model()
    check("auto + whisper.cpp load fails (error reply) -> faster-whisper used",
          host.backend == "faster-whisper" and t._model_loaded,
          f"backend={host.backend} loaded={t._model_loaded}")

    # Fallback happens only once per run: a second failure surfaces the error
    host = _FakeHost("whisper.cpp", raise_on_fail=False)
    t = _bare_transcriber(host, auto_engine=True)
    t._auto_fallback_used = True
    try:
        t._ensure_model()
        raise AssertionError("second failure should surface, not pass")
    except RuntimeError:
        pass
    check("fallback is once per run (second failure raises RuntimeError)",
          host.backend == "whisper.cpp" and not t._model_loaded)

    # Explicit engine keeps today's behaviour: the error surfaces, no fallback
    host = _FakeHost("whisper.cpp", raise_on_fail=True)
    t = _bare_transcriber(host, auto_engine=False)
    try:
        t._ensure_model()
        raise AssertionError("explicit whisper.cpp failure should raise")
    except ModelHostError:
        pass
    check("explicit whisper.cpp failure surfaces (no silent fallback)",
          host.backend == "whisper.cpp" and not t._model_loaded)

    print("  -> Auto fallback tests passed.")


def main():
    print("Running engine probe (EQ-AMD)...")
    try:
        test_resolve_backend()
        test_auto_fallback()
        test_adapters()
        test_whispercpp_model_dir()
        print("\nprobe_engine: ALL OK")
        return 0
    except Exception as e:
        import traceback
        print(f"\nprobe_engine: FAILED: {e}", file=sys.stderr)
        traceback.print_exc()
        return 1


if __name__ == "__main__":
    sys.exit(main())
