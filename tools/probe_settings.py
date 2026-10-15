"""Probe: settings resilience, concurrency, type coercion, and atomic writes.

Covers finding F9 (Task EQ-SETTINGS):
1. Corrupt file (truncated, invalid JSON, non-object, bad encoding, empty) -> .bad created
   and defaults used; newest .bad is kept; never crash.
2. Wrong types coerced to default types (bool, int, float, str, dict); uncoercible
   values dropped/defaulted; unknown keys ignored.
3. Concurrent saves from 3 threads leave a valid, uncorrupted file under a threading.Lock.
4. Save is atomic (temp file in same folder + flush + fsync + os.replace); no partial
   file or leftover temp files.

Standard library + src only. Runs with plain python:
    python3 tools/probe_settings.py
Exit 0 on success, 1 on failure.
"""

import json
import os
import sys
import tempfile
import threading
import time
from pathlib import Path

# Ensure repo root is on sys.path
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import src.settings as settings_mod
from src.settings import Settings


def test_corrupt_files():
    """Verify corrupt files are renamed to .bad, defaults are used, and newest .bad is kept."""
    print("Testing corrupt files handling...")
    with tempfile.TemporaryDirectory() as td:
        test_file = Path(td) / "settings.json"
        bad_file = Path(td) / "settings.json.bad"
        settings_mod.CONFIG_FILE = test_file

        # Scenario 1: Truncated JSON syntax
        test_file.write_text('{"language": "fr", "auto_paste":', encoding="utf-8")
        s = Settings.load()
        assert s.language == "en", f"Expected default 'en', got {s.language}"
        assert not test_file.exists(), "Corrupt file should have been renamed"
        assert bad_file.exists(), ".bad file should have been created"
        assert bad_file.read_text(encoding="utf-8") == '{"language": "fr", "auto_paste":'

        # Scenario 2: Newest .bad is kept when a second corruption occurs
        test_file.write_text('{"truncated": true', encoding="utf-8")
        s = Settings.load()
        assert s.language == "en"
        assert bad_file.read_text(encoding="utf-8") == '{"truncated": true', "Newest .bad should be kept"

        # Scenario 3: Non-object JSON (e.g. array, number, string, null)
        for non_object in ('["a", "b", "c"]', '"just a string"', '12345', 'null'):
            test_file.write_text(non_object, encoding="utf-8")
            s = Settings.load()
            assert s.language == "en"
            assert bad_file.exists()
            assert bad_file.read_text(encoding="utf-8") == non_object

        # Scenario 4: Bad encoding / non-UTF-8 bytes
        bad_bytes = b"\xff\xfe\x00\x12\x80\x90\xa0"
        test_file.write_bytes(bad_bytes)
        s = Settings.load()
        assert s.language == "en"
        assert bad_file.exists()
        assert bad_file.read_bytes() == bad_bytes

        # Scenario 5: Empty file (0 bytes)
        test_file.write_text("", encoding="utf-8")
        s = Settings.load()
        assert s.language == "en"
        assert bad_file.exists()

        # Scenario 6: Non-existent file uses defaults without creating .bad
        if bad_file.exists():
            bad_file.unlink()
        assert not test_file.exists()
        s = Settings.load()
        assert s.language == "en"
        assert not bad_file.exists(), ".bad should not be created if file did not exist"

    print("  -> Corrupt file tests passed.")


def test_type_coercion_and_unknown_keys():
    """Verify known fields are coerced to expected types and unknown keys are ignored."""
    print("Testing type coercion and unknown keys...")
    with tempfile.TemporaryDirectory() as td:
        test_file = Path(td) / "settings.json"
        bad_file = Path(td) / "settings.json.bad"
        settings_mod.CONFIG_FILE = test_file

        data = {
            # Coercible bools
            "auto_paste": "false",
            "history_enabled": 0,
            "voice_commands": "yes",
            "sound_feedback": 1,
            # Coercible ints
            "overlay_font_size": "22",
            # Coercible floats
            "overlay_opacity": "0.75",
            "vad_sensitivity": 2,
            # Coercible string
            "hotkey": 1234,
            # Coercible optional int
            "audio_device": "5",
            # Unknown keys that must be ignored
            "unknown_feature_flag": "ignored_value",
            "another_random_key": {"a": [1, 2]},
            # Uncoercible values that must fall back to defaults
            "remove_fillers": "unrecognized_boolean",
            "ui_scale": "not_a_float",
            "model_size": ["not", "a", "string"],
            "replacements": "not_a_dict",
        }
        test_file.write_text(json.dumps(data), encoding="utf-8")

        s = Settings.load()
        assert not bad_file.exists(), "Valid JSON object should not create .bad file"

        # Check coercions
        assert s.auto_paste is False, f"Expected False, got {s.auto_paste}"
        assert s.history_enabled is False, f"Expected False, got {s.history_enabled}"
        assert s.voice_commands is True, f"Expected True, got {s.voice_commands}"
        assert s.sound_feedback is True, f"Expected True, got {s.sound_feedback}"
        assert s.overlay_font_size == 22, f"Expected 22, got {s.overlay_font_size}"
        assert isinstance(s.overlay_font_size, int)
        assert s.overlay_opacity == 0.75, f"Expected 0.75, got {s.overlay_opacity}"
        assert isinstance(s.overlay_opacity, float)
        assert s.vad_sensitivity == 2.0, f"Expected 2.0, got {s.vad_sensitivity}"
        assert isinstance(s.vad_sensitivity, float)
        assert s.hotkey == "1234", f"Expected '1234', got {s.hotkey}"
        assert s.audio_device == 5, f"Expected 5, got {s.audio_device}"

        # Check uncoercible defaults
        assert s.remove_fillers is False  # default
        assert s.ui_scale == 1.25  # default
        assert s.model_size == "distil-large-v3"  # default
        assert s.replacements == {}  # default

        # Check unknown keys ignored
        assert not hasattr(s, "unknown_feature_flag")
        assert not hasattr(s, "another_random_key")

    print("  -> Type coercion and unknown keys tests passed.")


def test_concurrent_saves():
    """Verify concurrent saves from 3 threads leave a valid, uncorrupted file."""
    print("Testing concurrent saves from 3 threads...")
    with tempfile.TemporaryDirectory() as td:
        test_file = Path(td) / "settings.json"
        settings_mod.CONFIG_FILE = test_file

        s = Settings()
        assert hasattr(s, "_lock"), "Settings object must have a _lock"
        assert isinstance(s._lock, type(threading.Lock())), "Settings._lock must be a threading.Lock"

        barrier = threading.Barrier(3)
        errors = []

        def worker(thread_id: int):
            try:
                barrier.wait()
                for i in range(40):
                    s.hotkey = f"ctrl+{thread_id}+{i}"
                    s.overlay_font_size = 10 + (i % 20)
                    s.overlay_opacity = 0.5 + (i % 50) / 100.0
                    s.save()
                    time.sleep(0.001)
            except Exception as e:
                errors.append((thread_id, e))

        threads = [threading.Thread(target=worker, args=(i,)) for i in range(3)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()

        assert not errors, f"Errors in concurrent save threads: {errors}"
        assert test_file.exists(), "Settings file must exist after concurrent saves"

        # Verify file is valid JSON and loads cleanly
        content = test_file.read_text(encoding="utf-8")
        parsed = json.loads(content)
        assert isinstance(parsed, dict)
        loaded = Settings.load()
        assert isinstance(loaded, Settings)
        assert 10 <= loaded.overlay_font_size <= 30

    print("  -> Concurrent saves tests passed.")


def test_save_is_atomic():
    """Verify save is atomic via tempfile, flush, fsync, and replace; no partial files."""
    print("Testing atomic save (no partial files, no leftover temp files)...")
    with tempfile.TemporaryDirectory() as td:
        test_file = Path(td) / "settings.json"
        settings_mod.CONFIG_FILE = test_file

        s = Settings()
        stop_event = threading.Event()
        read_errors = []
        read_count = [0]

        # Reader thread reading constantly during rapid saves
        def reader():
            while not stop_event.is_set():
                if test_file.exists():
                    try:
                        raw = test_file.read_text(encoding="utf-8")
                        if raw:
                            data = json.loads(raw)
                            assert isinstance(data, dict)
                            assert "language" in data
                            read_count[0] += 1
                    except Exception as e:
                        read_errors.append(e)
                time.sleep(0.0005)

        r_thread = threading.Thread(target=reader)
        r_thread.start()

        # Perform 50 rapid saves with varying sizes
        for i in range(50):
            s.initial_prompt = "x" * (i * 200)  # variable content size
            s.overlay_font_size = 12 + (i % 10)
            s.save()
            time.sleep(0.001)

        stop_event.set()
        r_thread.join()

        assert not read_errors, f"Reader saw corrupted/partial file during save: {read_errors}"
        assert read_count[0] > 0, "Reader should have successfully read the file multiple times"

        # Check no temporary files were left behind
        tmp_files = list(Path(td).glob("*.tmp")) + list(Path(td).glob("settings_*.tmp"))
        assert not tmp_files, f"Found leftover temp files: {tmp_files}"

        # Verify final file is valid and loadable
        loaded = Settings.load()
        assert loaded.overlay_font_size == s.overlay_font_size
        assert loaded.initial_prompt == s.initial_prompt

    print("  -> Atomic save tests passed.")


def main():
    print("Running settings probe (EQ-SETTINGS)...")
    try:
        test_corrupt_files()
        test_type_coercion_and_unknown_keys()
        test_concurrent_saves()
        test_save_is_atomic()
        print("\nALL SETTINGS PROBE TESTS PASSED.")
        return 0
    except Exception as e:
        import traceback
        print(f"\nPROBE FAILED: {e}", file=sys.stderr)
        traceback.print_exc()
        return 1


if __name__ == "__main__":
    sys.exit(main())
