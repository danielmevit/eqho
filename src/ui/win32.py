"""Win32/DWM helpers for dashboard windows. Every call is a no-op off Windows."""

import ctypes
import logging

log = logging.getLogger(__name__)


def apply_dark_title_bar(tk_window, dark: bool) -> None:
    """Match the native title bar to the app theme (Windows 10 1809+ / 11).

    Attribute 20 is DWMWA_USE_IMMERSIVE_DARK_MODE on current builds; very old
    Windows 10 releases used 19, so fall back once.
    """
    try:
        tk_window.update_idletasks()
        hwnd = ctypes.windll.user32.GetParent(tk_window.winfo_id())
        value = ctypes.c_int(1 if dark else 0)
        for attribute in (20, 19):
            result = ctypes.windll.dwmapi.DwmSetWindowAttribute(
                hwnd, attribute, ctypes.byref(value), ctypes.sizeof(value),
            )
            if result == 0:
                return
    except Exception as e:
        log.debug("Dark title bar not applied: %s", e)
