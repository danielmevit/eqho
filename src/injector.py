"""Inject transcribed text into the currently focused application."""

import logging
import sys
import threading
import time

import pyperclip
from pynput.keyboard import Controller, Key

from . import oskit

log = logging.getLogger(__name__)

_kb = Controller()
_paste_lock = threading.Lock()
_RESTORE_DELAY_S = 0.4
# Windows: clipboard writes go through OsKit so the dictation carries the
# "keep out of clipboard history / cloud clipboard / monitors" formats.
_IS_WINDOWS = sys.platform == "win32"


def get_foreground_window():
    """Handle of the currently focused window (None where unsupported)."""
    return oskit.get().get_foreground_window()


def set_foreground_window(hwnd) -> None:
    """Bring the given window to the foreground (no-op where unsupported)."""
    if hwnd:
        oskit.get().set_foreground_window(hwnd)


def type_text(text: str, *, use_clipboard: bool = True) -> None:
    """Send *text* to the active window.

    When *use_clipboard* is True (default), the text is pasted via the
    clipboard which is faster and handles Unicode reliably. Otherwise
    keystrokes are simulated character-by-character (slower, ASCII-safe).
    """
    if not text:
        return

    try:
        if use_clipboard:
            _paste_via_clipboard(text)
        else:
            _type_chars(text)
    except Exception as e:
        log.warning("type_text failed (%s), falling back to simulated typing.", e)
        try:
            _type_chars(text)
        except Exception:
            pass


def _clip_read():
    """Clipboard text (OsKit on Windows, pyperclip elsewhere / as fallback)."""
    if _IS_WINDOWS:
        try:
            value = getattr(oskit.get(), "get_clipboard_text", lambda: None)()
        except Exception:
            value = None
        if value is not None:
            return value
    return pyperclip.paste()


def _clip_write(text: str) -> None:
    """Put *text* on the clipboard; on Windows kept out of history (private).
    Raises like pyperclip.copy when every route fails."""
    if _IS_WINDOWS:
        try:
            ok = getattr(oskit.get(), "set_clipboard_text", lambda t, private=True: False)(
                text, private=True)
        except Exception:
            ok = False
        if ok:
            return
        log.debug("OsKit clipboard write failed, using pyperclip.")
    pyperclip.copy(text)


def _paste_via_clipboard(text: str) -> None:
    with _paste_lock:
        old = None
        try:
            old = _clip_read()
        except Exception:
            old = None

        _release_held_modifiers()
        time.sleep(0.05)

        try:
            _clip_write(text)
        except Exception as e:
            # Clipboard unavailable (e.g. missing xclip/xsel on Linux, or another
            # app holding it open) — the text must still land somewhere.
            log.warning("Clipboard unavailable (%s), falling back to simulated typing.", e)
            _type_chars(text)
            return
        time.sleep(0.1)

        mod_name = oskit.get().paste_modifier()
        mod_key = Key.cmd if mod_name == "cmd" else Key.ctrl

        _kb.press(mod_key)
        _kb.press("v")
        _kb.release("v")
        _kb.release(mod_key)
        time.sleep(_RESTORE_DELAY_S)

        # Restore only if previous clipboard held non-empty text, and the clipboard
        # still holds our dictated text (if changed by user/app in the meantime, leave it alone).
        # The restore is private too: the old text is already in the user's history.
        if old:
            try:
                current = _clip_read()
            except Exception:
                current = None
            if current == text:
                try:
                    _clip_write(old)
                except Exception:
                    pass


_ALL_MODIFIERS = (
    Key.ctrl, Key.ctrl_l, Key.ctrl_r,
    Key.shift, Key.shift_l, Key.shift_r,
    Key.alt, Key.alt_l, Key.alt_r,
    Key.cmd, Key.cmd_l, Key.cmd_r,
)


def _release_held_modifiers() -> None:
    """Release modifiers still held from the hotkey so Ctrl+V isn't mangled.

    Only keys that are really down get a key-up: a stray Alt key-up puts some
    windows (Notepad-style) into menu mode and the following Ctrl+V is lost.
    Where the OS can't tell what's held, release every modifier as before.
    """
    try:
        held = getattr(oskit.get(), "held_modifiers", lambda: None)()
    except Exception:
        held = None
    if held is None:
        keys = _ALL_MODIFIERS
    else:
        keys = tuple(getattr(Key, name) for name in sorted(held) if hasattr(Key, name))
        if keys:
            log.debug("Releasing held modifiers before paste: %s", ", ".join(sorted(held)))
    for key in keys:
        try:
            _kb.release(key)
        except Exception:
            pass


def _type_chars(text: str) -> None:
    for ch in text:
        try:
            _kb.type(ch)
            time.sleep(0.01)
        except Exception:
            log.debug("Could not type char %r", ch)


def send_backspaces(count: int) -> None:
    """Delete the last *count* characters from the active app ("delete that")."""
    for _ in range(max(0, count)):
        try:
            _kb.press(Key.backspace)
            _kb.release(Key.backspace)
            time.sleep(0.005)
        except Exception:
            break
