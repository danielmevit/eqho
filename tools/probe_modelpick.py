"""Probe: the right speech model for this PC (EQ-PICK).

Fake hardware (monkeypatched helpers) -> gpu/cpu x en/de give the four
expected models; hardware_class() is computed once; _cuda_available() is
cached. No GUI, no downloads.

One line per check. Exit 0 on success, 1 on failure:
    venv/Scripts/python.exe tools/probe_modelpick.py
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import src.model_host as model_host
import src.modelpick as modelpick

failures = []


def check(ok: bool, label: str) -> None:
    print(f"[{'PASS' if ok else 'FAIL'}] {label}")
    if not ok:
        failures.append(label)


def fake(cuda: bool, backend: str) -> list:
    calls = []
    model_host._cuda_available = lambda: (calls.append("cuda"), cuda)[1]
    model_host.resolve_backend = lambda s: (calls.append("resolve"), backend)[1]
    modelpick._hw_class = None
    return calls


def main() -> int:
    real_cuda, real_resolve = model_host._cuda_available, model_host.resolve_backend
    cases = [
        ("NVIDIA", True, "faster-whisper", "gpu"),
        ("graphics card, whisper.cpp", False, "whisper.cpp", "gpu"),
        ("processor only", False, "faster-whisper", "cpu"),
    ]
    expect = {"gpu": ("distil-large-v3", "large-v3-turbo"),
              "cpu": ("distil-small.en", "small")}
    for name, cuda, backend, hw in cases:
        fake(cuda, backend)
        check(modelpick.hardware_class() == hw, f"{name} -> hardware_class {hw}")
        en, de = expect[hw]
        check(modelpick.recommend_model("en") == en, f'{name}: recommend_model("en") -> {en}')
        check(modelpick.recommend_model("de") == de, f'{name}: recommend_model("de") -> {de}')

    calls = fake(False, "faster-whisper")
    for _ in range(5):
        modelpick.hardware_class()
        modelpick.recommend_model("en")
    check(calls == ["cuda", "resolve"], f"hardware_class computed once (helper calls: {calls})")

    fake(False, "faster-whisper")
    model_host._cuda_available = lambda: 1 / 0
    check(modelpick.hardware_class() == "cpu", "detection error -> cpu")

    model_host._cuda_available, model_host.resolve_backend = real_cuda, real_resolve
    model_host._cuda_cache = None
    first = model_host._cuda_available()
    import ctranslate2
    saved = ctranslate2.get_supported_compute_types
    ctranslate2.get_supported_compute_types = lambda *a: 1 / 0
    try:
        check(model_host._cuda_available() == first, "_cuda_available() result cached")
    finally:
        ctranslate2.get_supported_compute_types = saved
    modelpick._hw_class = None
    print(f"[INFO] this PC: hardware_class={modelpick.hardware_class()}, "
          f"en -> {modelpick.recommend_model('en')}, de -> {modelpick.recommend_model('de')}")

    print("PASS" if not failures else f"FAIL ({len(failures)} checks)")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
