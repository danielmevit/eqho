"""Probe: paste key per OS, safe clipboard restore, and AppImage autostart.

Covers Task EQ-INJECT (review findings F6 + F12):
1. Darwin pastes with Key.cmd; Windows/Linux with Key.ctrl; modifiers released.
2. Old text clipboard is restored after the paste.
3. Old clipboard "" / None -> not restored (dictation stays).
4. Clipboard changed by someone else before the restore -> not overwritten.
5. pyperclip.copy raising -> falls back to typing, no exception.
6. autostart_command() returns the $APPIMAGE path when set to an existing file,
   else the old behavior.
7. Module-level threading lock guards the paste sequence.
8. EQ-CLIP: Windows path writes dictation + restore via OsKit
   set_clipboard_text(private=True); falls back to pyperclip when it fails;
   non-Windows path never touches the OsKit clipboard.
9. EQ-CLIP real check (Windows only): private write, read back, the three
   opt-out formats present, then the user's clipboard is restored. This one
   check touches the real clipboard (and puts it back).

Runs standalone on any OS, never presses real keys; only check 9 touches the real clipboard.
Exit 0 on success, 1 on failure.
"""

import os
import sys
import tempfile
import threading
import time
from pathlib import Path
from types import ModuleType

# Ensure repo root is on sys.path
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import _probe_env  # noqa: F401,E402  isolated EQHO_CONFIG_DIR before any src import

# If running in a minimal Python environment without pynput / pyperclip installed,
# provide stub modules so src.injector can be imported and tested.
if "pyperclip" not in sys.modules:
    try:
        import pyperclip
    except ImportError:
        fake_pyperclip = ModuleType("pyperclip")
        fake_pyperclip.copy = lambda text: None
        fake_pyperclip.paste = lambda: ""
        sys.modules["pyperclip"] = fake_pyperclip

if "pynput" not in sys.modules:
    try:
        import pynput
    except ImportError:
        fake_pynput = ModuleType("pynput")
        fake_keyboard = ModuleType("pynput.keyboard")

        class Key:
            ctrl = "<Key.ctrl>"
            ctrl_l = "<Key.ctrl_l>"
            ctrl_r = "<Key.ctrl_r>"
            shift = "<Key.shift>"
            shift_l = "<Key.shift_l>"
            shift_r = "<Key.shift_r>"
            alt = "<Key.alt>"
            alt_l = "<Key.alt_l>"
            alt_r = "<Key.alt_r>"
            cmd = "<Key.cmd>"
            cmd_l = "<Key.cmd_l>"
            cmd_r = "<Key.cmd_r>"
            backspace = "<Key.backspace>"

        class Controller:
            def press(self, key):
                pass

            def release(self, key):
                pass

            def type(self, text):
                pass

        fake_keyboard.Key = Key
        fake_keyboard.Controller = Controller
        fake_pynput.keyboard = fake_keyboard
        sys.modules["pynput"] = fake_pynput
        sys.modules["pynput.keyboard"] = fake_keyboard

from pynput.keyboard import Key

import src.injector as injector
import src.oskit as oskit
from src.oskit.base import autostart_command


class FakeController:
    """Fake keyboard controller that records presses, releases, and character typing."""

    def __init__(self):
        self.events = []  # list of ('press', key), ('release', key), ('type', text)
        self.on_press = None
        self.on_release = None
        self.on_type = None

    def press(self, key):
        self.events.append(("press", key))
        if self.on_press:
            self.on_press(key)

    def release(self, key):
        self.events.append(("release", key))
        if self.on_release:
            self.on_release(key)

    def type(self, text):
        self.events.append(("type", text))
        if self.on_type:
            self.on_type(text)


class FakeClipboard:
    """In-memory fake clipboard that records history and supports simulating failures."""

    def __init__(self, initial=""):
        self.content = initial
        self.history = [initial] if initial is not None else []
        self.raise_on_copy = None
        self.raise_on_paste = None

    def paste(self):
        if self.raise_on_paste:
            raise self.raise_on_paste
        return self.content

    def copy(self, text):
        if self.raise_on_copy:
            raise self.raise_on_copy
        self.content = text
        self.history.append(text)


class FakeOsKit:
    """Minimal fake OsKit returning a specified paste_modifier."""

    def __init__(self, modifier: str = "ctrl"):
        self._modifier = modifier

    def paste_modifier(self) -> str:
        return self._modifier

    def get_foreground_window(self):
        return None

    def set_foreground_window(self, hwnd):
        pass


def test_paste_modifier_per_os():
    """Verify Darwin pastes with Key.cmd, Windows/Linux with Key.ctrl, and modifiers are released."""
    fake_kb = FakeController()
    fake_clip = FakeClipboard(initial="existing text")
    sleep_calls = []

    orig_kb = injector._kb
    orig_copy = injector.pyperclip.copy
    orig_paste = injector.pyperclip.paste
    orig_sleep = injector.time.sleep
    orig_get = oskit.get

    try:
        injector._kb = fake_kb
        injector.pyperclip.copy = fake_clip.copy
        injector.pyperclip.paste = fake_clip.paste
        injector.time.sleep = lambda d: sleep_calls.append(d)

        cases = [
            ("darwin", Key.cmd),
            ("windows", Key.ctrl),
            ("linux", Key.ctrl),
        ]

        for os_name, expected_mod in cases:
            fake_kb.events.clear()
            oskit.get = lambda m=("cmd" if os_name == "darwin" else "ctrl"): FakeOsKit(modifier=m)

            injector.type_text(f"text for {os_name}")

            # Verify paste sequence in events
            v_press_idx = [i for i, ev in enumerate(fake_kb.events) if ev == ("press", "v")]
            assert len(v_press_idx) == 1, f"Expected 1 'v' press on {os_name}, got {len(v_press_idx)}"
            v_idx = v_press_idx[0]

            # The modifier must be pressed immediately before 'v'
            mod_press = fake_kb.events[v_idx - 1]
            assert mod_press == ("press", expected_mod), (
                f"Expected modifier ('press', {expected_mod}) before 'v' on {os_name}, got {mod_press}"
            )

            # The modifier must be released immediately after 'v' release
            v_rel_idx = fake_kb.events.index(("release", "v"))
            mod_release = fake_kb.events[v_rel_idx + 1]
            assert mod_release == ("release", expected_mod), (
                f"Expected modifier ('release', {expected_mod}) after 'v' on {os_name}, got {mod_release}"
            )

            # Ensure the opposite modifier was NOT used for paste
            wrong_mod = Key.ctrl if expected_mod == Key.cmd else Key.cmd
            paste_slice = fake_kb.events[v_idx - 1 : v_rel_idx + 2]
            assert ("press", wrong_mod) not in paste_slice, (
                f"Unexpected {wrong_mod} found in paste sequence for {os_name}"
            )

            # Check that Key.cmd, Key.cmd_l, Key.cmd_r were released in modifier cleanup
            early_releases = [ev for ev in fake_kb.events[: v_idx - 1] if ev[0] == "release"]
            released_keys = {ev[1] for ev in early_releases}
            assert Key.cmd in released_keys, f"Key.cmd was not released in cleanup loop on {os_name}"
            assert Key.cmd_l in released_keys, f"Key.cmd_l was not released in cleanup loop on {os_name}"
            assert Key.cmd_r in released_keys, f"Key.cmd_r was not released in cleanup loop on {os_name}"
            assert Key.ctrl in released_keys, f"Key.ctrl was not released in cleanup loop on {os_name}"
    finally:
        injector._kb = orig_kb
        injector.pyperclip.copy = orig_copy
        injector.pyperclip.paste = orig_paste
        injector.time.sleep = orig_sleep
        oskit.get = orig_get


def test_old_text_clipboard_restored():
    """Verify old text clipboard is restored after the paste."""
    fake_kb = FakeController()
    fake_clip = FakeClipboard(initial="my existing clipboard text")
    sleep_calls = []

    orig_kb = injector._kb
    orig_copy = injector.pyperclip.copy
    orig_paste = injector.pyperclip.paste
    orig_sleep = injector.time.sleep
    orig_get = oskit.get

    try:
        injector._kb = fake_kb
        injector.pyperclip.copy = fake_clip.copy
        injector.pyperclip.paste = fake_clip.paste
        injector.time.sleep = lambda d: sleep_calls.append(d)
        oskit.get = lambda: FakeOsKit("ctrl")

        injector.type_text("dictation to paste")

        # After paste, clipboard should be restored to old content
        assert fake_clip.content == "my existing clipboard text", (
            f"Expected clipboard to be restored to 'my existing clipboard text', got {fake_clip.content!r}"
        )
        assert fake_clip.history == [
            "my existing clipboard text",
            "dictation to paste",
            "my existing clipboard text",
        ], f"Unexpected clipboard history: {fake_clip.history}"
        assert injector._RESTORE_DELAY_S in sleep_calls, (
            f"Expected _RESTORE_DELAY_S ({injector._RESTORE_DELAY_S}) in sleep calls, got {sleep_calls}"
        )
    finally:
        injector._kb = orig_kb
        injector.pyperclip.copy = orig_copy
        injector.pyperclip.paste = orig_paste
        injector.time.sleep = orig_sleep
        oskit.get = orig_get


def test_empty_old_clipboard_not_restored():
    """Verify old clipboard '' or None is not restored (dictation stays)."""
    fake_kb = FakeController()
    fake_clip = FakeClipboard(initial="")
    sleep_calls = []

    orig_kb = injector._kb
    orig_copy = injector.pyperclip.copy
    orig_paste = injector.pyperclip.paste
    orig_sleep = injector.time.sleep
    orig_get = oskit.get

    try:
        injector._kb = fake_kb
        injector.pyperclip.copy = fake_clip.copy
        injector.pyperclip.paste = fake_clip.paste
        injector.time.sleep = lambda d: sleep_calls.append(d)
        oskit.get = lambda: FakeOsKit("ctrl")

        # Case A: initial clipboard is ""
        injector.type_text("dictation kept")
        assert fake_clip.content == "dictation kept", (
            f"Expected dictation to stay on clipboard, got {fake_clip.content!r}"
        )
        assert fake_clip.history == ["", "dictation kept"], (
            f"Expected no restore, history: {fake_clip.history}"
        )

        # Case B: initial clipboard is None (e.g. non-text clipboard)
        fake_clip.content = None
        fake_clip.history = []
        fake_kb.events.clear()
        injector.type_text("dictation kept 2")
        assert fake_clip.content == "dictation kept 2", (
            f"Expected dictation to stay on clipboard when old was None, got {fake_clip.content!r}"
        )
        assert fake_clip.history == ["dictation kept 2"], (
            f"Expected no restore when old was None, history: {fake_clip.history}"
        )
    finally:
        injector._kb = orig_kb
        injector.pyperclip.copy = orig_copy
        injector.pyperclip.paste = orig_paste
        injector.time.sleep = orig_sleep
        oskit.get = orig_get


def test_clipboard_changed_before_restore_not_overwritten():
    """Verify clipboard changed by someone else before the restore is not overwritten."""
    fake_kb = FakeController()
    fake_clip = FakeClipboard(initial="initial clipboard")
    sleep_calls = []

    orig_kb = injector._kb
    orig_copy = injector.pyperclip.copy
    orig_paste = injector.pyperclip.paste
    orig_sleep = injector.time.sleep
    orig_get = oskit.get

    try:
        injector._kb = fake_kb
        injector.pyperclip.copy = fake_clip.copy
        injector.pyperclip.paste = fake_clip.paste
        oskit.get = lambda: FakeOsKit("ctrl")

        def on_sleep_hook(d):
            sleep_calls.append(d)
            if d == injector._RESTORE_DELAY_S:
                # Interfere with clipboard during delay before restore
                fake_clip.content = "user copied something else in target app"
                fake_clip.history.append(fake_clip.content)

        injector.time.sleep = on_sleep_hook

        injector.type_text("dictated words")

        # Since clipboard was changed by someone else, injector must NOT overwrite it
        assert fake_clip.content == "user copied something else in target app", (
            f"Expected user copy to remain untouched, got {fake_clip.content!r}"
        )
        assert "initial clipboard" not in fake_clip.history[2:], (
            f"Initial clipboard was erroneously restored: {fake_clip.history}"
        )
    finally:
        injector._kb = orig_kb
        injector.pyperclip.copy = orig_copy
        injector.pyperclip.paste = orig_paste
        injector.time.sleep = orig_sleep
        oskit.get = orig_get


def test_copy_failure_falls_back_to_typing():
    """Verify pyperclip.copy raising falls back to typing without exception."""
    fake_kb = FakeController()
    fake_clip = FakeClipboard(initial="pre-existing")
    fake_clip.raise_on_copy = RuntimeError("Clipboard locked by another process")
    sleep_calls = []

    orig_kb = injector._kb
    orig_copy = injector.pyperclip.copy
    orig_paste = injector.pyperclip.paste
    orig_sleep = injector.time.sleep
    orig_get = oskit.get

    try:
        injector._kb = fake_kb
        injector.pyperclip.copy = fake_clip.copy
        injector.pyperclip.paste = fake_clip.paste
        injector.time.sleep = lambda d: sleep_calls.append(d)
        oskit.get = lambda: FakeOsKit("ctrl")

        # Must not raise exception to caller
        injector.type_text("fallback text")

        # Verify fallback to typing occurred
        typed_events = [ev for ev in fake_kb.events if ev[0] == "type"]
        typed_string = "".join(ev[1] for ev in typed_events)
        assert typed_string == "fallback text", (
            f"Expected typed characters 'fallback text', got {typed_string!r}"
        )

        # Verify paste keys (Ctrl+V / Cmd+V) were NOT pressed
        v_presses = [ev for ev in fake_kb.events if ev == ("press", "v")]
        assert len(v_presses) == 0, f"Paste key was pressed despite copy error: {fake_kb.events}"
    finally:
        injector._kb = orig_kb
        injector.pyperclip.copy = orig_copy
        injector.pyperclip.paste = orig_paste
        injector.time.sleep = orig_sleep
        oskit.get = orig_get


def test_autostart_command_appimage():
    """Verify autostart_command() returns $APPIMAGE path when set to existing file, else old behaviour."""
    orig_env = os.environ.get("APPIMAGE")
    orig_frozen = getattr(sys, "frozen", False)
    try:
        with tempfile.NamedTemporaryFile(suffix=".AppImage", delete=False) as tf:
            temp_path = tf.name

        try:
            # 1. APPIMAGE set to existing file
            os.environ["APPIMAGE"] = temp_path
            cmd = autostart_command()
            assert cmd == f'"{temp_path}"', f"Expected quoted AppImage path {temp_path!r}, got {cmd!r}"

            # 2. APPIMAGE set to non-existent file
            non_existent = temp_path + ".doesnotexist"
            os.environ["APPIMAGE"] = non_existent
            cmd_fallback = autostart_command()
            assert f'"{non_existent}"' != cmd_fallback, "Should not return non-existent AppImage"
            assert sys.executable in cmd_fallback, (
                f"Expected sys.executable fallback, got {cmd_fallback!r}"
            )

            # 3. APPIMAGE unset
            if "APPIMAGE" in os.environ:
                del os.environ["APPIMAGE"]
            cmd_unset = autostart_command()
            assert sys.executable in cmd_unset, (
                f"Expected sys.executable when APPIMAGE unset, got {cmd_unset!r}"
            )

            # 4. sys.frozen = True with APPIMAGE pointing to existing file
            sys.frozen = True
            os.environ["APPIMAGE"] = temp_path
            cmd_frozen_appimage = autostart_command()
            assert cmd_frozen_appimage == f'"{temp_path}"', (
                f"Expected AppImage path even when frozen, got {cmd_frozen_appimage!r}"
            )

            # 5. sys.frozen = True without APPIMAGE
            del os.environ["APPIMAGE"]
            cmd_frozen_no_appimage = autostart_command()
            assert cmd_frozen_no_appimage == f'"{sys.executable}"', (
                f"Expected quoted sys.executable when frozen and no AppImage, got {cmd_frozen_no_appimage!r}"
            )
        finally:
            if os.path.exists(temp_path):
                os.remove(temp_path)
    finally:
        if orig_env is not None:
            os.environ["APPIMAGE"] = orig_env
        elif "APPIMAGE" in os.environ:
            del os.environ["APPIMAGE"]
        if hasattr(sys, "frozen"):
            if orig_frozen:
                sys.frozen = orig_frozen
            else:
                delattr(sys, "frozen")


def test_paste_lock():
    """Verify module-level threading lock guards paste sequence."""
    assert hasattr(injector, "_paste_lock"), "injector must define _paste_lock"
    assert isinstance(injector._paste_lock, type(threading.Lock())), "_paste_lock must be a threading.Lock"

    fake_kb = FakeController()
    fake_clip = FakeClipboard(initial="text")
    was_locked_during_paste = []

    def check_lock(key):
        if key == "v":
            was_locked_during_paste.append(injector._paste_lock.locked())

    fake_kb.on_press = check_lock

    orig_kb = injector._kb
    orig_copy = injector.pyperclip.copy
    orig_paste = injector.pyperclip.paste
    orig_sleep = injector.time.sleep
    orig_get = oskit.get

    try:
        injector._kb = fake_kb
        injector.pyperclip.copy = fake_clip.copy
        injector.pyperclip.paste = fake_clip.paste
        injector.time.sleep = lambda d: None
        oskit.get = lambda: FakeOsKit("ctrl")

        injector.type_text("lock test")
        assert was_locked_during_paste == [True], (
            f"Expected _paste_lock to be locked during paste, got {was_locked_during_paste}"
        )
        assert not injector._paste_lock.locked(), "_paste_lock must be released after paste"
    finally:
        injector._kb = orig_kb
        injector.pyperclip.copy = orig_copy
        injector.pyperclip.paste = orig_paste
        injector.time.sleep = orig_sleep
        oskit.get = orig_get


class HeldOsKit(FakeOsKit):
    """Fake OsKit that reports a fixed set of held modifiers (None = unknown)."""

    def __init__(self, held):
        super().__init__("ctrl")
        self._held = held

    def held_modifiers(self):
        return self._held


def _releases_before_paste(held):
    fake_kb = FakeController()
    fake_clip = FakeClipboard("")
    orig = (injector._kb, injector.pyperclip.copy, injector.pyperclip.paste, injector.time.sleep, oskit.get)
    try:
        injector._kb = fake_kb
        injector.pyperclip.copy = fake_clip.copy
        injector.pyperclip.paste = fake_clip.paste
        injector.time.sleep = lambda d: None
        oskit.get = lambda: HeldOsKit(held)
        injector.type_text("held test")
    finally:
        (injector._kb, injector.pyperclip.copy, injector.pyperclip.paste,
         injector.time.sleep, oskit.get) = orig
    first_press = next(i for i, ev in enumerate(fake_kb.events) if ev[0] == "press")
    return [ev[1] for ev in fake_kb.events[:first_press] if ev[0] == "release"]


def test_only_held_modifiers_released():
    """Nothing held -> no key-ups; ctrl_l+alt_r held -> exactly those; None -> full list."""
    assert _releases_before_paste(set()) == [], "No modifier held must send no key-ups"
    got = _releases_before_paste({"ctrl_l", "alt_r"})
    assert got == [Key.alt_r, Key.ctrl_l], f"Expected ctrl_l+alt_r released, got {got}"
    got = _releases_before_paste(None)
    assert got == list(injector._ALL_MODIFIERS), f"Unknown state must release all modifiers, got {got}"


class ClipOsKit(FakeOsKit):
    """FakeOsKit with an OsKit clipboard (records private writes)."""

    def __init__(self, initial="", ok=True):
        super().__init__("ctrl")
        self.content = initial
        self.ok = ok
        self.writes = []

    def set_clipboard_text(self, text, private=True):
        self.writes.append((text, private))
        if not self.ok:
            return False
        self.content = text
        return True

    def get_clipboard_text(self):
        return self.content if self.ok else None


def _run_paste(is_windows, kit, clip, text):
    orig = (injector._kb, injector.pyperclip.copy, injector.pyperclip.paste,
            injector.time.sleep, oskit.get, injector._IS_WINDOWS)
    sleeps = []
    try:
        injector._kb = FakeController()
        injector.pyperclip.copy = clip.copy
        injector.pyperclip.paste = clip.paste
        injector.time.sleep = lambda d: sleeps.append(d)
        oskit.get = lambda: kit
        injector._IS_WINDOWS = is_windows
        injector.type_text(text)
    finally:
        (injector._kb, injector.pyperclip.copy, injector.pyperclip.paste,
         injector.time.sleep, oskit.get, injector._IS_WINDOWS) = orig
    return sleeps


def test_windows_private_clipboard():
    kit = ClipOsKit(initial="old user text")
    clip = FakeClipboard(initial="pyperclip-untouched")
    sleeps = _run_paste(True, kit, clip, "dictation")
    assert kit.writes == [("dictation", True), ("old user text", True)], kit.writes
    assert kit.content == "old user text", kit.content
    assert clip.history == ["pyperclip-untouched"], f"pyperclip used: {clip.history}"
    assert 0.1 in sleeps and injector._RESTORE_DELAY_S in sleeps, sleeps

    # OsKit write fails -> pyperclip carries dictation and restore.
    kit = ClipOsKit(ok=False)
    clip = FakeClipboard(initial="old user text")
    _run_paste(True, kit, clip, "dictation")
    assert kit.writes == [("dictation", True), ("old user text", True)], kit.writes
    assert clip.history == ["old user text", "dictation", "old user text"], clip.history

    # Non-Windows: OsKit clipboard never used, pyperclip exactly as before.
    kit = ClipOsKit(initial="x")
    clip = FakeClipboard(initial="old user text")
    _run_paste(False, kit, clip, "dictation")
    assert kit.writes == [], kit.writes
    assert clip.history == ["old user text", "dictation", "old user text"], clip.history


def test_real_windows_clipboard():
    if sys.platform != "win32":
        print("    (skipped: not Windows)")
        return
    import ctypes
    from src.oskit.windows import WindowsOsKit

    kit = WindowsOsKit()
    before = kit.get_clipboard_text()
    probe = "EQ-CLIP probe \u00e4\u00f6\u00fc \u2713"
    try:
        assert kit.set_clipboard_text(probe, private=True), "set_clipboard_text returned False"
        assert kit.get_clipboard_text() == probe, "read-back mismatch"
        _, user32, _ = kit._clip_api()
        missing = []
        assert kit._open_clipboard(user32), "clipboard busy"
        try:
            for name in kit._PRIVATE_FORMATS:
                fmt = user32.RegisterClipboardFormatW(name)
                if not (fmt and user32.IsClipboardFormatAvailable(fmt)):
                    missing.append(name)
        finally:
            user32.CloseClipboard()
        assert not missing, f"opt-out formats missing: {missing}"
    finally:
        if before is not None:
            kit.set_clipboard_text(before, private=True)
        else:
            _, user32, _ = kit._clip_api()
            if kit._open_clipboard(user32):
                user32.EmptyClipboard()
                user32.CloseClipboard()
    assert kit.get_clipboard_text() == before, "clipboard not restored"


def main():
    print("Running probe_inject (EQ-INJECT)...")
    checks = [
        ("Darwin pastes with Key.cmd, Windows/Linux with Key.ctrl", test_paste_modifier_per_os),
        ("Old text clipboard is restored after the paste", test_old_text_clipboard_restored),
        ("Old clipboard '' is not restored (dictation stays)", test_empty_old_clipboard_not_restored),
        (
            "Clipboard changed by someone else before restore is not overwritten",
            test_clipboard_changed_before_restore_not_overwritten,
        ),
        ("pyperclip.copy raising falls back to typing without exception", test_copy_failure_falls_back_to_typing),
        (
            "autostart_command() returns $APPIMAGE path when set to existing file, else old behaviour",
            test_autostart_command_appimage,
        ),
        ("Threading lock guards paste sequence and serializes execution", test_paste_lock),
        ("Only modifiers that are really held get a key-up (None = all)", test_only_held_modifiers_released),
        ("Windows: private OsKit clipboard for dictation + restore, pyperclip fallback", test_windows_private_clipboard),
        ("Real Windows clipboard: private write, read-back, opt-out formats, restored", test_real_windows_clipboard),
    ]

    for idx, (desc, test_fn) in enumerate(checks, 1):
        try:
            test_fn()
            print(f"[{idx}/{len(checks)}] {desc}: OK")
        except Exception as e:
            import traceback

            print(f"[{idx}/{len(checks)}] {desc}: FAILED ({e})", file=sys.stderr)
            traceback.print_exc()
            return 1

    print("probe_inject: ALL OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())
