"""Headless model-switch diagnostic.

    python run.py --diagnose [target-model]     (default: distil-large-v3)

Replays the exact in-app model-switch path with per-phase timings:
cache checks for every model, load of the CURRENT model, then the switch to
the target — printing how long each phase takes. If any phase exceeds 60 s,
all thread stacks are dumped so a hang shows its exact location.
Settings are NEVER saved — this is read-only for your config.
"""

import logging
import sys
import threading
import time


def _timed(label: str, fn):
    print(f">>> {label} ...", flush=True)
    guard = threading.Timer(60.0, _hang_dump, args=(label,))
    guard.daemon = True
    guard.start()
    t0 = time.monotonic()
    try:
        result = fn()
        print(f"<<< {label}: OK in {time.monotonic() - t0:.2f}s", flush=True)
        return result
    except Exception as e:
        print(f"<<< {label}: FAILED after {time.monotonic() - t0:.2f}s — {type(e).__name__}: {e}", flush=True)
        raise
    finally:
        guard.cancel()


def _hang_dump(label: str) -> None:
    from .watchdog import dump_stacks
    print(f"!!! {label} has been running for 60s — dumping thread stacks", flush=True)
    dump_stacks(f"--diagnose phase '{label}' exceeded 60s")


def run_diagnose(argv) -> int:
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s [%(levelname)s] %(name)s: %(message)s",
        datefmt="%H:%M:%S",
    )
    args = [a for a in argv[1:] if not a.startswith("--")]
    target = args[0] if args else "distil-large-v3"

    from src.settings import Settings, WHISPER_MODELS, is_model_cached
    settings = Settings.load()
    current = settings.model_size
    print(f"model_dir : {settings.resolve_model_dir()}")
    print(f"current   : {current}")
    print(f"target    : {target}")

    print("\n-- cache checks (each timed) --")
    for key in WHISPER_MODELS:
        t0 = time.monotonic()
        cached = is_model_cached(settings, key)
        print(f"  {key:<18} cached={cached}  ({(time.monotonic() - t0) * 1000:.0f} ms)")

    from src.transcriber import VoiceTranscriber
    transcriber = VoiceTranscriber(settings)

    print("\n-- load current model (as at app start) --")
    _timed(f"load {current}", transcriber._ensure_model)

    print("\n-- switch to target (exact in-app reload path) --")
    settings.model_size = target  # in-memory only, never saved
    _timed("reload_model()", transcriber.reload_model)
    _timed(f"load {target}", transcriber._ensure_model)

    import numpy as np
    from src.transcriber import SAMPLE_RATE
    _timed("transcribe 0.5s silence", lambda: transcriber.transcribe_once(
        np.zeros(SAMPLE_RATE // 2, dtype=np.float32)))

    print("\nDiagnosis complete — no hang on this path.")
    return 0
