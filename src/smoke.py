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

        from src.textproc import (
            apply_replacements, clean_text, is_delete_command, match_command, smart_join,
        )
        assert match_command("Period.") == "."
        assert match_command("New line") == "\n"
        assert match_command("hello world") is None
        assert is_delete_command("Delete that")
        assert apply_replacements("hello eqho app", {"eqho": "Eqho"}) == "hello Eqho app"
        assert smart_join(["hello", ".", "world"]) == "hello. world"
        assert smart_join(["one", "\n", "two"]) == "one\ntwo"
        # clean_text: casing + spacing, idempotent, never reorders words
        assert clean_text("hello world") == "Hello world"
        assert clean_text("i think i'm right . yes") == "I think I'm right. Yes"
        assert clean_text("hello  ,  world") == "Hello, world"
        assert clean_text("um so uh hi", remove_fillers=True) == "So hi"
        assert clean_text("um so uh hi", remove_fillers=False) == "Um so uh hi"
        assert clean_text("one\ntwo") == "One\nTwo"
        assert clean_text(clean_text("i said hi")) == clean_text("i said hi")  # idempotent
        report["steps"]["textproc"] = "ok"

        import tempfile
        from pathlib import Path
        from src.history import TranscriptHistory
        with tempfile.TemporaryDirectory() as td:
            hist = TranscriptHistory(Path(td) / "h.jsonl", max_entries=3)
            for i in range(5):
                hist.append(f"entry {i}")
            entries = hist.read_all()
            assert len(entries) == 3 and entries[0]["text"] == "entry 4"
            assert len(hist.search("entry 3")) == 1
        report["steps"]["history"] = "ok"

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
