"""Inject transcribed text into the currently focused application."""

import ctypes
import logging
import time

import pyperclip
from pynput.keyboard import Controller, Key

log = logging.getLogger(__name__)

_kb = Controller()
_user32 = ctypes.windll.user32


def get_foreground_window() -> int:
    """Return the handle of the currently focused window."""
    return _user32.GetForegroundWindow()


def set_foreground_window(hwnd: int) -> None:
    """Bring the given window to the foreground."""
    if hwnd:
        _user32.SetForegroundWindow(hwnd)


def type_text(text: str, *, use_clipboard: bool = True) -> None:
    """Send *text* to the active window.

    When *use_clipboard* is True (default), the text is pasted via the
    clipboard which is faster and handles Unicode reliably. Otherwise
    keystrokes are simulated character-by-character (slower, ASCII-safe).
    """
    if not text:
        return

    if use_clipboard:
        _paste_via_clipboard(text)
    else:
        _type_chars(text)


def _paste_via_clipboard(text: str) -> None:
    old = None
    try:
        old = pyperclip.paste()
    except Exception:
        pass

    # Ensure all modifier keys are released before pasting
    for key in (Key.ctrl, Key.ctrl_l, Key.ctrl_r, Key.shift, Key.shift_l, Key.shift_r, Key.alt, Key.alt_l, Key.alt_r):
        try:
            _kb.release(key)
        except Exception:
            pass
    time.sleep(0.05)

    try:
        pyperclip.copy(text)
    except Exception as e:
        # Clipboard unavailable (e.g. missing xclip/xsel on Linux, or another
        # app holding it open) — the text must still land somewhere.
        log.warning("Clipboard unavailable (%s), falling back to simulated typing.", e)
        _type_chars(text)
        return
    time.sleep(0.05)

    _kb.press(Key.ctrl)
    _kb.press("v")
    _kb.release("v")
    _kb.release(Key.ctrl)
    time.sleep(0.1)

    if old is not None:
        try:
            pyperclip.copy(old)
        except Exception:
            pass


def _type_chars(text: str) -> None:
    for ch in text:
        try:
            _kb.type(ch)
            time.sleep(0.01)
        except Exception:
            log.debug("Could not type char %r", ch)
