"""Dictation overlay: the iridescent pill + the live-transcription panel.

Two frameless tkinter windows (one tk thread, marshaled via `after`):

- **The pill** — a small capsule at the work-area edge (bottom-center by
  default) running the iridescent fluid animation from ``pillfx``: it breathes,
  its ribbons ride the live mic level, and it doubles as the "listening"
  indicator. On Windows the window is color-keyed transparent so the capsule
  floats free of any plate.
- **The panel** — a rounded rectangle stacked toward screen center from the
  pill (above it for bottom anchors), filling *upward* with the words as the
  partial transcription grows — the Gemini-style dictation look. Hidden until
  there are real words.

Theme-aware via theme.py; rounded corners via the Windows 11 DWM API.
Public surface is unchanged: start/show/update_text/set_level/hide/shutdown.
"""

import ctypes
import logging
import sys
import threading
import time
import tkinter as tk
from typing import Optional

from . import pillfx
from .fonts import FONT_FAMILY
from .settings import Settings
from .theme import get_colors

log = logging.getLogger(__name__)

_PADDING_X = 18
_PADDING_Y = 12
_MARGIN = 24            # distance from the work-area edge
_GAP = 10               # space between the pill and the panel
_PILL_W = 400
_PILL_H = 64
_PANEL_MAX_W = 640
_WRAP_LENGTH = 560
_FADE_STEPS = 5
_FADE_INTERVAL = 25     # ms between fade steps
_FRAME_MS = 40          # pill animation cadence (~25 fps)
_MAX_TEXT_CHARS = 220   # show only the TAIL of long dictations (latest words win)
_PLACEHOLDER = "Listening..."
_TRANSPARENT_KEY = "#010203"  # color-key for the pill window (Windows only)


def _tail_text(text: str) -> str:
    if len(text) <= _MAX_TEXT_CHARS:
        return text
    return "…" + text[-(_MAX_TEXT_CHARS - 1):]


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


def _round_window(widget: tk.Misc) -> None:
    try:
        hwnd = ctypes.windll.user32.GetParent(widget.winfo_id())
        _apply_rounded_corners(hwnd)
    except Exception:
        pass


class TranscriptionOverlay:
    """The floating pill + upward-filling live transcription panel."""

    def __init__(self, settings: Settings):
        self._settings = settings
        self._root: Optional[tk.Tk] = None           # the pill window
        self._panel: Optional[tk.Toplevel] = None    # the text panel
        self._panel_label: Optional[tk.Label] = None
        self._pill_canvas: Optional[tk.Canvas] = None
        self._pill_item = None
        self._pill_img: Optional[tk.PhotoImage] = None
        self._thread: Optional[threading.Thread] = None
        self._visible = False
        self._panel_shown = False
        self._ready = threading.Event()
        self._fade_job = None
        self._render_job = None
        self._anim_t0 = 0.0
        self._level_target = 0.0
        self._level_current = 0.0
        self._transparent_ok = False
        self._light = 0.0
        self._pill_bg = (0.0, 0.0, 0.0)
        self._pill_x = 0
        self._pill_y = 0

    def start(self) -> None:
        if self._thread and self._thread.is_alive():
            return
        self._thread = threading.Thread(target=self._run_tk, daemon=True)
        self._thread.start()
        self._ready.wait(timeout=3)

    def _run_tk(self) -> None:
        self._root = tk.Tk()
        # CRITICAL: release tkinter's process-wide "default root" claim.
        # The overlay's Tk is created first (on THIS thread); if it stays the
        # default root, every Variable/CTkFont the dashboard creates without
        # an explicit master binds to THIS interpreter — and the dashboard
        # thread then makes cross-thread Tcl calls that intermittently
        # DEADLOCK (the model-switch freeze; see eqho.log thread dumps).
        # Consequence here: every tk object below needs an explicit master.
        tk._default_root = None
        self._root.title("Eqho")
        self._root.overrideredirect(True)
        self._root.attributes("-topmost", True)
        self._root.attributes("-alpha", self._settings.overlay_opacity)

        # Color-key transparency lets the capsule float without a plate
        # (Windows layered windows only; elsewhere the pill sits on a
        # theme-colored rounded plate).
        if sys.platform == "win32":
            try:
                self._root.configure(bg=_TRANSPARENT_KEY)
                self._root.attributes("-transparentcolor", _TRANSPARENT_KEY)
                self._transparent_ok = True
            except Exception:
                self._transparent_ok = False

        self._pill_canvas = tk.Canvas(
            self._root,
            width=_PILL_W,
            height=_PILL_H,
            highlightthickness=0,
            bd=0,
        )
        self._pill_canvas.pack()
        self._pill_item = self._pill_canvas.create_image(0, 0, anchor="nw")

        self._panel = tk.Toplevel(self._root)
        self._panel.overrideredirect(True)
        self._panel.attributes("-topmost", True)
        panel_frame = tk.Frame(self._panel, padx=_PADDING_X, pady=_PADDING_Y)
        panel_frame.pack(fill=tk.BOTH, expand=True)
        self._panel_label = tk.Label(
            panel_frame,
            text="",
            anchor="w",
            justify="left",
            wraplength=_WRAP_LENGTH,
        )
        self._panel_label.pack(fill=tk.BOTH, expand=True)
        self._panel.withdraw()

        self._refresh_theme()

        self._root.update_idletasks()
        if not self._transparent_ok:
            _round_window(self._root)
        _round_window(self._panel)

        self._root.withdraw()
        self._ready.set()
        self._root.mainloop()

    def _refresh_theme(self) -> None:
        colors = get_colors(self._settings.theme)
        bg, fg = colors.overlay_bg, colors.overlay_fg
        r, g, b = pillfx.hex_rgb01(bg)
        self._light = 1.0 if (0.2126 * r + 0.7152 * g + 0.0722 * b) > 0.5 else 0.0
        if self._transparent_ok:
            self._pill_bg = pillfx.hex_rgb01(_TRANSPARENT_KEY)
            self._pill_canvas.configure(bg=_TRANSPARENT_KEY)
        else:
            self._pill_bg = (r, g, b)
            self._root.configure(bg=bg)
            self._pill_canvas.configure(bg=bg)
        self._panel.configure(bg=bg)
        self._panel_label.master.configure(bg=bg)
        self._panel_label.config(
            bg=bg,
            fg=fg,
            font=(FONT_FAMILY, self._settings.overlay_font_size),
        )

    # -- Show / hide --------------------------------------------------------------

    def show(self, text: str = _PLACEHOLDER) -> None:
        if not self._settings.overlay_enabled:
            return
        if not self._root:
            return
        try:
            self._root.after(0, self._do_show, text)
        except Exception:
            pass

    def _do_show(self, text: str) -> None:
        self._refresh_theme()

        sw = self._root.winfo_screenwidth()
        sh = self._root.winfo_screenheight()
        self._pill_x, self._pill_y = self._calc_position(_PILL_W, _PILL_H, sw, sh)
        self._root.geometry(f"{_PILL_W}x{_PILL_H}+{self._pill_x}+{self._pill_y}")

        if not self._visible:
            self._root.attributes("-alpha", 0.0)
            self._panel.attributes("-alpha", 0.0)
            self._root.deiconify()
            self._visible = True
            self._anim_t0 = time.monotonic()
            self._level_current = 0.0
            self._level_target = 0.0
            self._start_render()
            self._fade_to(self._settings.overlay_opacity)
        else:
            self._root.attributes("-alpha", self._settings.overlay_opacity)

        self._set_panel_text(text)

    def _calc_position(self, w: int, h: int, sw: int, sh: int) -> tuple[int, int]:
        """Pill x, y from the position preference, anchored to the WORK AREA
        (excludes the taskbar) so bottom-center sits exactly where it should
        instead of floating above a taskbar-sized gap."""
        left, top, right, bottom = 0, 0, sw, sh
        try:
            from . import oskit
            area = oskit.get().work_area()
            if area:
                left, top, right, bottom = area
        except Exception:
            pass
        margin = _MARGIN
        cx = left + (right - left - w) // 2
        if pos := self._settings.overlay_position:
            if pos == "top-center":
                return cx, top + margin
            if pos == "top-left":
                return left + margin, top + margin
            if pos == "top-right":
                return right - w - margin, top + margin
            if pos == "bottom-left":
                return left + margin, bottom - h - margin
            if pos == "bottom-right":
                return right - w - margin, bottom - h - margin
        return cx, bottom - h - margin  # bottom-center (default)

    def _anchored_top(self) -> bool:
        return (self._settings.overlay_position or "").startswith("top")

    def update_text(self, text: str) -> None:
        if not self._root:
            return
        try:
            self._root.after(0, self._set_panel_text, text)
        except Exception:
            pass

    def _set_panel_text(self, text: str) -> None:
        """Show/grow the panel for real words; hide it for the placeholder.
        The panel is bottom-anchored relative to the pill, so extra lines make
        it fill UPWARD — the window rises as the text grows."""
        if not self._visible or not self._panel:
            return
        content = (text or "").strip()
        if not content or content == _PLACEHOLDER:
            if self._panel_shown:
                self._panel.withdraw()
                self._panel_shown = False
            return
        try:
            self._panel_label.config(text=_tail_text(content))
            self._panel.update_idletasks()
            w = min(
                max(self._panel_label.winfo_reqwidth() + 2 * _PADDING_X, _PILL_W),
                _PANEL_MAX_W,
            )
            h = self._panel_label.winfo_reqheight() + 2 * _PADDING_Y
            pill_cx = self._pill_x + _PILL_W // 2
            x = pill_cx - w // 2
            if self._anchored_top():
                y = self._pill_y + _PILL_H + _GAP
            else:
                y = self._pill_y - _GAP - h
            self._panel.geometry(f"{w}x{h}+{x}+{y}")
            if not self._panel_shown:
                self._panel.attributes("-alpha", float(self._root.attributes("-alpha")))
                self._panel.deiconify()
                self._panel_shown = True
            else:
                self._panel.attributes("-alpha", float(self._root.attributes("-alpha")))
        except Exception:
            pass

    def hide(self) -> None:
        if not self._root:
            return
        try:
            self._root.after(0, self._do_hide)
        except Exception:
            pass

    def _do_hide(self) -> None:
        self._visible = False
        self._stop_render()
        self._level_target = 0.0
        self._level_current = 0.0

        def _finish() -> None:
            self._root.withdraw()
            self._panel.withdraw()
            self._panel_shown = False
            self._panel_label.config(text="")

        self._fade_to(0.0, on_done=_finish)

    # -- Audio level --------------------------------------------------------------

    def set_level(self, level: float) -> None:
        """Feed the live mic level (0..1); the pill's ribbons ride it."""
        self._level_target = max(0.0, min(1.0, level))

    # -- Pill animation -----------------------------------------------------------

    def _start_render(self) -> None:
        self._stop_render()
        self._render_tick()

    def _render_tick(self) -> None:
        if not self._visible or not self._pill_canvas:
            return
        # Fast attack, gentle release — reads as "alive" instead of a twitch
        # (kept from the old level bar; updates arrive at ~2 Hz).
        self._level_current += (self._level_target - self._level_current) * 0.5
        self._level_target *= 0.90
        level = self._level_current
        if not getattr(self._settings, "overlay_show_level", True):
            level = 0.0
        t = time.monotonic() - self._anim_t0
        try:
            data = pillfx.render_ppm(_PILL_W, _PILL_H, t, level, self._light, self._pill_bg)
            # Explicit master: the default root is deliberately released above.
            img = tk.PhotoImage(master=self._root, data=data)
            self._pill_img = img  # keep a reference; tk only borrows it
            self._pill_canvas.itemconfig(self._pill_item, image=img)
        except Exception:
            return
        self._render_job = self._root.after(_FRAME_MS, self._render_tick)

    def _stop_render(self) -> None:
        if self._render_job is not None:
            try:
                self._root.after_cancel(self._render_job)
            except Exception:
                pass
            self._render_job = None

    # -- Fade ---------------------------------------------------------------------

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

        def _apply(alpha: float) -> None:
            self._root.attributes("-alpha", alpha)
            if self._panel_shown:
                self._panel.attributes("-alpha", alpha)

        def _step(i: int = 1) -> None:
            self._fade_job = None
            try:
                if i >= _FADE_STEPS:
                    _apply(target)
                    if on_done:
                        on_done()
                    return
                _apply(current + delta * i)
            except Exception:
                return
            self._fade_job = self._root.after(_FADE_INTERVAL, _step, i + 1)

        _step()

    def shutdown(self) -> None:
        root = self._root
        if not root:
            return

        def _destroy() -> None:
            # Free every Tcl-backed object ON the tk thread. If the last
            # PhotoImage/widget reference instead dies with this overlay
            # object on another thread, CPython runs their __del__ against a
            # dead/foreign interpreter → "main thread is not in main loop" +
            # Tcl_AsyncDelete noise at app exit.
            self._stop_render()
            if self._pill_canvas is not None:
                try:
                    self._pill_canvas.itemconfig(self._pill_item, image="")
                except Exception:
                    pass
            self._pill_img = None
            self._pill_canvas = None
            self._pill_item = None
            self._panel_label = None
            self._panel = None
            self._root = None
            try:
                root.destroy()
            except Exception:
                pass

        try:
            root.after(0, _destroy)
        except Exception:
            pass
