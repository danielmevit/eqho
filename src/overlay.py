"""Floating transparent overlay showing real-time partial transcription.

Uses a frameless tkinter window. Theme-aware colors from theme.py.
Rounded corners via Windows 11 DWM API (DWMWA_WINDOW_CORNER_PREFERENCE).
"""

import ctypes
import logging
import sys
import threading
import tkinter as tk
from typing import Optional

from .fonts import FONT_FAMILY
from .settings import Settings
from .theme import get_colors

log = logging.getLogger(__name__)

_PADDING_X = 18
_PADDING_Y = 10
_MARGIN = 60
_MIN_WIDTH = 300
_PULSE_MS = 650      # recording-dot pulse interval
_FADE_STEPS = 5      # fade in/out animation steps
_FADE_INTERVAL = 25  # ms between fade steps


def _blend_hex(c1: str, c2: str, t: float) -> str:
    """Linear blend of two #rrggbb colors (t=0 → c1, t=1 → c2)."""
    a = [int(c1[i:i + 2], 16) for i in (1, 3, 5)]
    b = [int(c2[i:i + 2], 16) for i in (1, 3, 5)]
    return "#%02x%02x%02x" % tuple(round(x + (y - x) * t) for x, y in zip(a, b))


def _apply_rounded_corners(hwnd: int) -> None:
    """Apply rounded corners via Windows 11 DWM API. No-op on older Windows."""
    try:
        DWMWA_WINDOW_CORNER_PREFERENCE = 33
        DWMWCP_ROUND = 2
        preference = ctypes.c_int(DWMWCP_ROUND)
        ctypes.windll.dwmapi.DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            ctypes.byref(preference),
            ctypes.sizeof(preference),
        )
    except Exception:
        pass


class TranscriptionOverlay:
    """A small floating bar showing live transcription text."""

    def __init__(self, settings: Settings):
        self._settings = settings
        self._root: Optional[tk.Tk] = None
        self._label: Optional[tk.Label] = None
        self._status_dot: Optional[tk.Canvas] = None
        self._thread: Optional[threading.Thread] = None
        self._visible = False
        self._ready = threading.Event()
        self._pulse_job = None
        self._pulse_phase = 0
        self._fade_job = None

    def start(self) -> None:
        if self._thread and self._thread.is_alive():
            return
        self._thread = threading.Thread(target=self._run_tk, daemon=True)
        self._thread.start()
        self._ready.wait(timeout=3)

    def _get_theme_colors(self) -> tuple[str, str, str]:
        """Return (bg, fg, accent) based on current theme setting."""
        colors = get_colors(self._settings.theme)
        return colors.overlay_bg, colors.overlay_fg, colors.overlay_accent

    def _run_tk(self) -> None:
        bg, fg, accent = self._get_theme_colors()

        self._root = tk.Tk()
        self._root.title("Eqho")
        self._root.overrideredirect(True)
        self._root.attributes("-topmost", True)
        self._root.attributes("-alpha", self._settings.overlay_opacity)

        self._root.configure(bg=bg)

        # Apply rounded corners (Windows 11+; no-op elsewhere)
        self._root.update_idletasks()
        try:
            hwnd = ctypes.windll.user32.GetParent(self._root.winfo_id())
            _apply_rounded_corners(hwnd)
        except Exception:
            pass

        frame = tk.Frame(self._root, bg=bg, padx=_PADDING_X, pady=_PADDING_Y)
        frame.pack(fill=tk.BOTH, expand=True)

        self._status_dot = tk.Canvas(
            frame, width=10, height=10, bg=bg, highlightthickness=0,
        )
        self._status_dot.create_oval(1, 1, 9, 9, fill=accent, outline="", tags="dot")
        self._status_dot.pack(side=tk.LEFT, padx=(0, 8))

        self._label = tk.Label(
            frame,
            text="Listening...",
            font=(FONT_FAMILY, self._settings.overlay_font_size),
            fg=fg,
            bg=bg,
            anchor="w",
            wraplength=600,
        )
        self._label.pack(side=tk.LEFT, fill=tk.X, expand=True)

        self._root.withdraw()
        self._ready.set()
        self._root.mainloop()

    def show(self, text: str = "Listening...") -> None:
        if not self._settings.overlay_enabled:
            return
        if not self._root:
            return
        try:
            self._root.after(0, self._do_show, text)
        except Exception:
            pass

    def _do_show(self, text: str) -> None:
        # Update theme colors on each show (in case theme changed)
        bg, fg, accent = self._get_theme_colors()
        self._root.configure(bg=bg)
        self._label.config(
            text=text if text else "Listening...",
            fg=fg, bg=bg,
            font=(FONT_FAMILY, self._settings.overlay_font_size),
        )
        self._status_dot.configure(bg=bg)
        self._status_dot.itemconfig("dot", fill=accent)
        self._label.master.configure(bg=bg)

        self._root.update_idletasks()

        w = max(_MIN_WIDTH, self._label.winfo_reqwidth() + 2 * _PADDING_X + 26)
        h = self._label.winfo_reqheight() + 2 * _PADDING_Y
        sw = self._root.winfo_screenwidth()
        sh = self._root.winfo_screenheight()
        x, y = self._calc_position(w, h, sw, sh)
        self._root.geometry(f"{w}x{h}+{x}+{y}")

        if not self._visible:
            self._root.attributes("-alpha", 0.0)
            self._root.deiconify()
            self._visible = True
            self._fade_to(self._settings.overlay_opacity)
            self._start_pulse()
        else:
            # Update opacity in case the setting changed
            self._root.attributes("-alpha", self._settings.overlay_opacity)

    def _calc_position(self, w: int, h: int, sw: int, sh: int) -> tuple[int, int]:
        """Calculate overlay x, y based on the position preference."""
        pos = self._settings.overlay_position
        margin = _MARGIN
        if pos == "top-center":
            return (sw - w) // 2, margin
        elif pos == "top-left":
            return margin, margin
        elif pos == "top-right":
            return sw - w - margin, margin
        elif pos == "bottom-left":
            return margin, sh - h - margin
        elif pos == "bottom-right":
            return sw - w - margin, sh - h - margin
        else:  # bottom-center (default)
            return (sw - w) // 2, sh - h - margin

    def update_text(self, text: str) -> None:
        if not self._root:
            return
        try:
            self._root.after(0, self._do_update, text)
        except Exception:
            pass

    def _do_update(self, text: str) -> None:
        if self._label:
            self._label.config(text=text if text else "Listening...")

    def hide(self) -> None:
        if not self._root:
            return
        try:
            self._root.after(0, self._do_hide)
        except Exception:
            pass

    def _do_hide(self) -> None:
        self._stop_pulse()
        self._visible = False
        self._fade_to(0.0, on_done=self._root.withdraw)

    # -- Animations -------------------------------------------------------------

    def _start_pulse(self) -> None:
        self._stop_pulse()
        self._pulse_phase = 0
        self._pulse_tick()

    def _pulse_tick(self) -> None:
        if not self._visible or not self._status_dot:
            return
        bg, _fg, accent = self._get_theme_colors()
        self._pulse_phase = (self._pulse_phase + 1) % 2
        color = accent if self._pulse_phase == 0 else _blend_hex(accent, bg, 0.55)
        try:
            self._status_dot.itemconfig("dot", fill=color)
        except Exception:
            return
        self._pulse_job = self._root.after(_PULSE_MS, self._pulse_tick)

    def _stop_pulse(self) -> None:
        if self._pulse_job is not None:
            try:
                self._root.after_cancel(self._pulse_job)
            except Exception:
                pass
            self._pulse_job = None

    def _fade_to(self, target: float, on_done=None) -> None:
        if self._fade_job is not None:
            try:
                self._root.after_cancel(self._fade_job)
            except Exception:
                pass
            self._fade_job = None
        try:
            current = float(self._root.attributes("-alpha"))
        except Exception:
            current = target
        delta = (target - current) / _FADE_STEPS

        def _step(i: int = 1) -> None:
            self._fade_job = None
            try:
                if i >= _FADE_STEPS:
                    self._root.attributes("-alpha", target)
                    if on_done:
                        on_done()
                    return
                self._root.attributes("-alpha", current + delta * i)
            except Exception:
                return
            self._fade_job = self._root.after(_FADE_INTERVAL, _step, i + 1)

        _step()

    def shutdown(self) -> None:
        if self._root:
            try:
                self._root.after(0, self._root.destroy)
            except Exception:
                pass
