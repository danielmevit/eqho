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
import _probe_env  # noqa: F401,E402  isolated EQHO_CONFIG_DIR before any src import

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


def test_bom_file_loads():
    """A settings.json saved as "UTF-8 with BOM" loads its values (not corrupt)."""
    print("Testing settings.json with a UTF-8 BOM...")
    with tempfile.TemporaryDirectory() as td:
        test_file = Path(td) / "settings.json"
        settings_mod.CONFIG_FILE = test_file
        test_file.write_bytes(b"\xef\xbb\xbf" + b'{"language": "fr"}')
        s = Settings.load()
        assert s.language == "fr", f"Expected 'fr' from BOM file, got {s.language}"
        assert not (Path(td) / "settings.json.bad").exists(), "BOM file wrongly called corrupt"
        print("  ok: BOM settings.json loads its values")


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
                barrier.wait(timeout=10.0)
                for i in range(40):
                    s.hotkey = f"ctrl+{thread_id}+{i}"
                    s.overlay_font_size = 10 + (i % 20)
                    s.overlay_opacity = 0.5 + (i % 50) / 100.0
                    s.save()
                    time.sleep(0.001)
            except Exception as e:
                errors.append((thread_id, e))

        threads = [threading.Thread(target=worker, args=(i,), name=f"worker-{i}") for i in range(3)]
        for t in threads:
            t.start()
        for t in threads:
            t.join(timeout=10.0)
            assert not t.is_alive(), f"Worker thread {t.name} timed out (stuck)"

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

        # Reader thread reading constantly during rapid saves.
        # Models Windows realistically: open, read and CLOSE quickly.
        def reader():
            while not stop_event.is_set():
                if test_file.exists():
                    try:
                        with open(test_file, "r", encoding="utf-8") as f:
                            raw = f.read()
                        data = json.loads(raw)
                        assert isinstance(data, dict), f"Read non-dict: {type(data)}"
                        assert "language" in data, "Missing 'language' key"
                        read_count[0] += 1
                    except (FileNotFoundError, PermissionError):
                        # On Windows, opening a file while os.replace is in-flight
                        # can briefly raise PermissionError / WinError 32 or FileNotFoundError.
                        pass
                    except Exception as e:
                        read_errors.append(e)
                time.sleep(0.001)

        r_thread = threading.Thread(target=reader, name="atomic-reader")
        r_thread.start()

        try:
            # Perform 50 rapid saves with varying sizes
            for i in range(50):
                s.initial_prompt = "x" * (i * 200)  # variable content size
                s.overlay_font_size = 12 + (i % 10)
                s.save()
                time.sleep(0.001)
        finally:
            stop_event.set()
            r_thread.join(timeout=10.0)
            assert not r_thread.is_alive(), "Reader thread timed out (stuck)"

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


def test_save_reader_holds_file():
    """Verify that when a reader HOLDS the file open for ~0.3 s during a save,
    save() succeeds after the reader releases (retry), or degrades gracefully
    (logs warning, no exception).
    """
    print("Testing save while reader holds file open (~0.3 s)...")
    with tempfile.TemporaryDirectory() as td:
        test_file = Path(td) / "settings.json"
        settings_mod.CONFIG_FILE = test_file

        s = Settings(hotkey="alt+q", overlay_font_size=14)
        s.save()
        assert test_file.exists()

        holder_started = threading.Event()
        holder_error = []

        def lock_holder():
            try:
                with open(test_file, "r", encoding="utf-8") as f:
                    _ = f.read()
                    holder_started.set()
                    time.sleep(0.3)
            except Exception as e:
                holder_error.append(e)
                holder_started.set()

        h_thread = threading.Thread(target=lock_holder, name="lock-holder")
        h_thread.start()

        try:
            assert holder_started.wait(timeout=5.0), "Lock holder thread did not start in time"
            assert not holder_error, f"Error in lock holder thread: {holder_error}"

            s.hotkey = "ctrl+shift+p"
            s.overlay_font_size = 28
            # Must succeed after reader releases (retry), or degrade gracefully (never raise)
            s.save()
        finally:
            h_thread.join(timeout=10.0)
            assert not h_thread.is_alive(), "Lock holder thread timed out (stuck)"

        # Check no temporary files were left behind
        tmp_files = list(Path(td).glob("*.tmp")) + list(Path(td).glob("settings_*.tmp"))
        assert not tmp_files, f"Found leftover temp files: {tmp_files}"

        # Verify final file is valid JSON and loadable
        loaded = Settings.load()
        assert isinstance(loaded, Settings)
        assert loaded.hotkey in ("alt+q", "ctrl+shift+p")

    print("  -> Save with held file tests passed.")


def test_save_retries_and_degrades_gracefully():
    """Explicitly verify retry loop and graceful degradation on PermissionError/OSError."""
    print("Testing save() retry loop and graceful degradation...")
    with tempfile.TemporaryDirectory() as td:
        test_file = Path(td) / "settings.json"
        settings_mod.CONFIG_FILE = test_file

        s = Settings(hotkey="alt+q", overlay_font_size=14)
        s.save()
        assert test_file.exists()

        # Case 1: Transient PermissionError (fails 3 times, then succeeds)
        orig_replace = os.replace
        replace_attempts = [0]

        def transient_replace(src, dst):
            replace_attempts[0] += 1
            if replace_attempts[0] <= 3:
                raise PermissionError("[WinError 5] Access is denied")
            return orig_replace(src, dst)

        os.replace = transient_replace
        try:
            s.hotkey = "ctrl+alt+t"
            s.save()
            assert replace_attempts[0] == 4, f"Expected 4 attempts, got {replace_attempts[0]}"
            loaded = Settings.load()
            assert loaded.hotkey == "ctrl+alt+t"
        finally:
            os.replace = orig_replace

        # Case 2: Persistent PermissionError (fails all 10 times)
        persistent_attempts = [0]

        def persistent_fail_replace(src, dst):
            persistent_attempts[0] += 1
            raise PermissionError("[WinError 5] Access is denied")

        os.replace = persistent_fail_replace
        try:
            s.hotkey = "ctrl+alt+fail"
            # Must NOT raise to caller; logs ONE warning, deletes temp file, keeps in-memory settings
            s.save()
            assert persistent_attempts[0] == 10, f"Expected 10 attempts, got {persistent_attempts[0]}"
            assert s.hotkey == "ctrl+alt+fail", "In-memory settings must be kept"
            # No leftover temp files
            tmp_files = list(Path(td).glob("*.tmp")) + list(Path(td).glob("settings_*.tmp"))
            assert not tmp_files, f"Temp file was not cleaned up: {tmp_files}"
            # File on disk should still be the previous valid file
            loaded = Settings.load()
            assert loaded.hotkey == "ctrl+alt+t"
        finally:
            os.replace = orig_replace

    print("  -> Retry and graceful degradation tests passed.")


def test_pill_options():
    """EQ-PILLOPTS: pill size maths; old files get pill 0.8 and a text-box
    opacity derived from their saved overlay_opacity."""
    print("Testing pill size / text-box opacity...")
    from src.overlay import pill_size
    for scale, want in ((0.6, (168, 31)), (0.8, (224, 41)), (1.0, (280, 51)), (1.4, (392, 71))):
        assert pill_size(scale) == want, f"pill_size({scale}) = {pill_size(scale)}, want {want}"
    assert pill_size(5) == pill_size(1.4) and pill_size(0) == pill_size(0.6)
    assert pill_size("junk") == (224, 41)
    s = Settings()
    assert s.overlay_pill_scale == 0.8 and s.overlay_panel_opacity == s.overlay_opacity == 0.85
    with tempfile.TemporaryDirectory() as td:
        test_file = Path(td) / "settings.json"
        settings_mod.CONFIG_FILE = test_file
        test_file.write_text(json.dumps({"overlay_opacity": 0.6}), encoding="utf-8")
        s = Settings.load()
        assert s.overlay_pill_scale == 0.8, s.overlay_pill_scale
        assert s.overlay_panel_opacity == 0.6, s.overlay_panel_opacity
        test_file.write_text(json.dumps({"language": "de"}), encoding="utf-8")
        s = Settings.load()
        assert s.overlay_panel_opacity == 0.85
        test_file.write_text(json.dumps({"overlay_opacity": 0.6, "overlay_panel_opacity": 0.95,
                                         "overlay_pill_scale": 9}), encoding="utf-8")
        s = Settings.load()
        assert s.overlay_panel_opacity == 0.95 and s.overlay_pill_scale == 1.4
        s.save()
        assert Settings.load().overlay_panel_opacity == 0.95
    print("  pill sizes 168x31 / 224x41 / 280x51 / 392x71; old file -> 0.8, box = saved opacity")


def main():
    print("Running settings probe (EQ-SETTINGS)...")
    try:
        test_corrupt_files()
        test_type_coercion_and_unknown_keys()
        test_concurrent_saves()
        test_bom_file_loads()
        test_save_is_atomic()
        test_save_reader_holds_file()
        test_save_retries_and_degrades_gracefully()
        test_pill_options()
        print("\nALL SETTINGS PROBE TESTS PASSED.")
        return 0
    except Exception as e:
        import traceback
        print(f"\nPROBE FAILED: {e}", file=sys.stderr)
        traceback.print_exc()
        return 1


if __name__ == "__main__":
    sys.exit(main())
