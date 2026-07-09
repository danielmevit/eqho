"""Headless self-check: settings, audio stack, model load, transcription.

Run with `python run.py --smoke`. No GUI, tray, or hotkeys — safe in CI and
from WSL. Prints a JSON report and exits 0 (pass) / 1 (fail). Uses the `tiny`
model (~150 MB, downloaded on first use) so the check stays fast.
"""

import json
import logging
import sys


def run_smoke() -> int:
    logging.basicConfig(level=logging.WARNING)
    report: dict = {"ok": False, "steps": {}}
    try:
        from src.version import __version__
        report["version"] = __version__

        from src.settings import Settings
        settings = Settings.load()
        report["steps"]["settings"] = "ok"
        report["model_dir"] = str(settings.resolve_model_dir())

        from src.audio import list_input_devices
        devices = list_input_devices()
        report["steps"]["audio_devices"] = len(devices)

        import numpy as np
        from src.transcriber import VoiceTranscriber, SAMPLE_RATE

        settings.model_size = "tiny"  # in-memory only, never saved
        transcriber = VoiceTranscriber(settings)
        text = transcriber.transcribe_once(
            np.zeros(SAMPLE_RATE // 2, dtype=np.float32)
        )
        report["steps"]["model_load"] = "ok"
        report["steps"]["transcribe_silence"] = "ok"
        report["silence_text"] = text  # expected: empty or near-empty

        report["ok"] = True
    except Exception as e:  # any failure fails the gate — that's the point
        report["error"] = f"{type(e).__name__}: {e}"

    print(json.dumps(report, indent=2))
    return 0 if report["ok"] else 1


if __name__ == "__main__":
    sys.exit(run_smoke())
