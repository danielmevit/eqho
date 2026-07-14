"""Eqho Dashboard — main settings window (orchestrator).

Owns the window, singleton state, sidebar navigation, theme switching, and
responsive rebuild logic. Tab content lives in src/ui/tabs/*; shared builders
in src/ui/layout.py; custom widgets in src/ui/widgets.py.
"""

import logging
import threading
import tkinter as tk
from pathlib import Path
from typing import Callable, Optional

import customtkinter as ctk

from ..settings import Settings
from ..theme import (
    get_colors, get_system_theme, ThemeColors,
    RADIUS_SM, RADIUS_MD, SPACING, font,
)
from .context import DashboardContext
from .icons import icon, icon_font
from .tabs import TAB_CLASSES
from .widgets import ghost_button, primary_button
from .win32 import apply_dark_title_bar

log = logging.getLogger(__name__)

# Window dimensions (wider default since the top bar now hosts nav + status)
WIN_W, WIN_H = 940, 560
TOPBAR_H = 64
# Below this top-bar width (unscaled units) the status text would slide under
# the centered pill — hide it instead of colliding.
STATUS_MIN_W = 910

# Responsive breakpoints (content area width — full window now, no sidebar)
BP_2COL = 560   # 2 columns when content >= 560px
BP_3COL = 900   # 3 columns when content >= 900px

# Old tab keys that folded into the Settings view (gear icon, top right).
TAB_ALIASES = {"overlay": "settings", "about": "settings"}


class Dashboard(ctk.CTkToplevel):
    """Main Eqho settings dashboard window."""

    def __init__(
        self,
        settings: Settings,
        on_settings_changed: Callable,
        parent: Optional[ctk.CTk] = None,
        on_restart: Optional[Callable] = None,
        initial_tab: Optional[str] = None,
    ):
        self._on_restart = on_restart
        initial_tab = TAB_ALIASES.get(initial_tab, initial_tab)
        self._initial_tab = initial_tab if initial_tab in TAB_CLASSES else "general"
        # Create a hidden root if needed
        self._own_root = None
        if parent is None:
            self._own_root = ctk.CTk()
            self._own_root.withdraw()
            parent = self._own_root

        super().__init__(parent)
        self._settings = settings
        self._on_settings_changed = on_settings_changed
        self._colors: ThemeColors = get_colors(self._settings.theme)
        self._current_tab = "general"
        self._tab_frames: dict[str, ctk.CTkFrame] = {}
        self._tabs: dict[str, object] = {}
        self._last_col_count = 0  # track responsive state
        self._tab_built_cols: dict[str, int] = {}  # col count each tab was built with
        self._tab_bottom_spacers: dict[str, ctk.CTkFrame] = {}
        self._assets = Path(__file__).resolve().parent.parent.parent / "assets"

        self._ctx = DashboardContext(
            settings=settings,
            colors_getter=lambda: self._colors,
            apply_settings=self._apply_settings,
            get_col_count=self._get_col_count,
            rebuild_tab=self.rebuild_tab,
            set_ui_scale=self.set_ui_scale,
            master_getter=lambda: self,
            change_model=self._change_model,
            set_theme=self._set_theme,
            refresh_status=self._refresh_topbar_status,
        )
        # Non-visible tabs rebuild on next show after a model change, so
        # their headers/cards never show a stale active model
        self._ctx.subscribe("model_changed", "dashboard", lambda *_: self._mark_hidden_tabs_stale())

        self._setup_window()
        self._build_topbar()
        self._build_content_area()
        # Tabs build lazily on first show — keeps window open and theme
        # switching fast (only the visible tab is rebuilt synchronously)
        self._show_tab(self._initial_tab)

        # Responsive resize handler
        self._content.bind("<Configure>", self._on_content_resize)

        # Feed the freeze watchdog from inside the Tk loop — if this stops
        # beating, the watchdog dumps all thread stacks to the log
        self._schedule_heartbeat()

        # Register as the active singleton BEFORE mainloop blocks
        global _active_dashboard
        _active_dashboard = self

        if self._own_root:
            self.protocol("WM_DELETE_WINDOW", self._on_close)
            self._own_root.mainloop()

    def _setup_window(self) -> None:
        # UI zoom (Daniel: default 150%) — must be set before widgets/geometry
        self._ui_scale = max(1.0, min(2.0, float(getattr(self._settings, "ui_scale", 1.5) or 1.5)))
        ctk.set_widget_scaling(self._ui_scale)
        ctk.set_window_scaling(self._ui_scale)

        self.title("Eqho Dashboard")
        self.geometry(f"{WIN_W}x{WIN_H}")
        self.minsize(600, 420)
        self.resizable(True, True)
        self.attributes("-topmost", False)

        # Set window icon from new logo
        self._set_window_icon()

        # Apply theme
        mode = self._settings.theme
        if mode == "system":
            mode = get_system_theme()
        ctk.set_appearance_mode(mode)
        ctk.set_default_color_theme("blue")

        self.configure(fg_color=self._colors.bg_primary)
        apply_dark_title_bar(self, mode == "dark")

        # Center on screen (geometry sizes get multiplied by window scaling)
        self.update_idletasks()
        sw = self.winfo_screenwidth()
        sh = self.winfo_screenheight()
        x = max(0, (sw - int(WIN_W * self._ui_scale)) // 2)
        y = max(0, (sh - int(WIN_H * self._ui_scale)) // 2)
        self.geometry(f"{WIN_W}x{WIN_H}+{x}+{y}")

    def _set_window_icon(self) -> None:
        """Set the title-bar icon, theme-matched.

        Deferred past CTkToplevel's own delayed iconbitmap call (~200 ms),
        which would otherwise overwrite whatever we set at construction —
        that race was why the old icon kept reappearing.
        """
        def _apply() -> None:
            try:
                from PIL import Image, ImageTk
                resolved = self._settings.theme
                if resolved == "system":
                    resolved = get_system_theme()
                name = "logo_32_dark.png" if resolved == "dark" else "logo_32_white.png"
                logo_path = self._assets / name
                if not logo_path.exists():
                    logo_path = self._assets / "logo_32.png"
                if logo_path.exists():
                    img = Image.open(logo_path).convert("RGBA")
                    tk_root = self._own_root if self._own_root else self
                    self._icon_photos = [
                        ImageTk.PhotoImage(img.resize((s, s), Image.LANCZOS), master=tk_root)
                        for s in (16, 32, 48)
                    ]
                    self.iconphoto(False, *self._icon_photos)
                    return
                ico_path = self._assets / "eqho.ico"
                if ico_path.exists():
                    self.iconbitmap(str(ico_path))
            except Exception as e:
                log.debug("Failed to set window icon: %s", e)

        self.after(260, _apply)

    # -- Top bar (ref/ui: centered pill nav, icon actions right) -----------------

    def _build_topbar(self) -> None:
        self._topbar = ctk.CTkFrame(
            self, height=TOPBAR_H, corner_radius=0,
            fg_color=self._colors.bg_primary,
        )
        self._topbar.pack(side="top", fill="x")
        self._topbar.pack_propagate(False)

        # Logo (left)
        title_frame = ctk.CTkFrame(self._topbar, fg_color="transparent")
        title_frame.pack(side="left", padx=(SPACING["lg"], 0))

        # Asset names indicate the THEME they serve (Daniel's convention):
        # *_dark.png is shown in dark mode, *_light.png in light mode.
        wordmark_for_light_theme = self._assets / "logo_horizontal_light.png"
        wordmark_for_dark_theme = self._assets / "logo_horizontal_dark.png"
        if wordmark_for_light_theme.exists() and wordmark_for_dark_theme.exists():
            from PIL import Image, ImageTk
            pil_for_light = Image.open(wordmark_for_light_theme)
            pil_for_dark = Image.open(wordmark_for_dark_theme)
            max_w = 110
            ratio = max_w / pil_for_light.width
            new_h = int(pil_for_light.height * ratio)
            pil_for_light = pil_for_light.resize((max_w, new_h), Image.LANCZOS)
            pil_for_dark = pil_for_dark.resize((max_w, new_h), Image.LANCZOS)
            resolved = self._settings.theme
            if resolved == "system":
                resolved = get_system_theme()
            pil_img = pil_for_light if resolved == "light" else pil_for_dark
            # Bind PhotoImage to this window's Tk instance
            tk_root = self._own_root if self._own_root else self
            self._logo_tk = ImageTk.PhotoImage(pil_img, master=tk_root)
            logo_label = tk.Label(
                title_frame, image=self._logo_tk,
                bg=self._colors.bg_primary, borderwidth=0,
            )
            logo_label.pack(anchor="w")
        else:
            ctk.CTkLabel(
                title_frame, text="Eqho",
                font=font("xl", "bold"),
                text_color=self._colors.fg_primary,
            ).pack(anchor="w")

        # Right cluster: status line + theme toggle + gear — icons hug each
        # other and the window edge (Daniel).
        actions = ctk.CTkFrame(self._topbar, fg_color="transparent")
        actions.pack(side="right", padx=(0, SPACING["sm"]))

        from .layout import status_summary
        self._status_lbl = ctk.CTkLabel(
            actions, text=status_summary(self._settings),
            font=font("xs"), text_color=self._colors.fg_muted,
            fg_color="transparent", anchor="e",
        )
        self._status_lbl.pack(side="left", padx=(0, SPACING["sm"]))

        # Small CIRCLE buttons, tight together (Daniel: not pill-shaped).
        # CTkButton pads text width, so pin width == height and zero padding.
        resolved = self._settings.theme
        if resolved == "system":
            resolved = get_system_theme()
        toggle_glyph = icon("moon") if resolved == "light" else icon("sun")
        toggle_target = "dark" if resolved == "light" else "light"

        def _circle_button(parent, glyph: str, command) -> ctk.CTkButton:
            btn = ctk.CTkButton(
                parent, text=glyph, width=28, height=28, corner_radius=14,
                font=icon_font("sm", 3), fg_color="transparent",
                text_color=self._colors.fg_secondary,
                hover_color=self._colors.bg_hover,
                border_spacing=0,
                command=command,
            )
            btn.pack(side="left", padx=0)
            return btn

        self._theme_btn = _circle_button(
            actions, toggle_glyph, lambda t=toggle_target: self._set_theme(t),
        )
        self._gear_btn = _circle_button(
            actions, icon("settings"), lambda: self._show_tab("settings"),
        )

        # Centered pill nav — placed, not packed, so it stays truly centered
        # regardless of the logo/actions widths (like the reference).
        #
        # Geometry: fully-round capsules (corner_radius=999 — CTk clamps to
        # height/2) with a UNIFORM gap between segments and the pill border.
        # The gap must keep the segments' rectangular canvases inside the
        # outer curve: corner (g,g) is inside radius R iff (R-g)·√2 ≤ R →
        # g ≥ 0.293·R.  seg 34 + gap 8 → pill 50, R 25: need g ≥ 7.33. ✓
        self._nav_pill = ctk.CTkFrame(
            self._topbar, corner_radius=999, height=50,
            fg_color=self._colors.bg_secondary,
            border_width=1, border_color=self._colors.border_subtle,
        )
        self._nav_pill.place(relx=0.5, rely=0.5, anchor="center")

        self._nav_segments: dict[str, tuple] = {}
        nav_items = (("general", "General"), ("models", "Models"), ("history", "History"))
        for i, (key, label) in enumerate(nav_items):
            pad_l = 8 if i == 0 else 4
            pad_r = 8 if i == len(nav_items) - 1 else 4
            self._nav_segments[key] = self._build_nav_segment(
                self._nav_pill, key, label, padx=(pad_l, pad_r),
            )

        # Hairline under the bar
        self._topbar_rule = ctk.CTkFrame(
            self, height=1, corner_radius=0, fg_color=self._colors.border_subtle,
        )
        self._topbar_rule.pack(side="top", fill="x")

        # Auto-hide the status line when the bar is too narrow for it to
        # coexist with the centered pill.
        self._topbar.bind("<Configure>", self._on_topbar_resize)

    def _on_topbar_resize(self, event=None) -> None:
        lbl = getattr(self, "_status_lbl", None)
        if lbl is None:
            return
        try:
            width_units = self._topbar.winfo_width() / max(self._ui_scale, 0.01)
            visible = bool(lbl.winfo_manager())
            if width_units < STATUS_MIN_W and visible:
                lbl.pack_forget()
            elif width_units >= STATUS_MIN_W and not visible:
                lbl.pack(side="left", padx=(0, SPACING["sm"]), before=self._theme_btn)
        except Exception:
            pass

    def _build_nav_segment(self, parent, key: str, label: str, padx=(4, 4)) -> tuple:
        # Fully-round capsule (999 → clamped to height/2); inner padding
        # doubled per Daniel — content gets real breathing room.
        seg = ctk.CTkFrame(parent, corner_radius=999, fg_color=self._colors.bg_secondary, height=34)
        seg.pack(side="left", padx=padx, pady=8)
        icon_lbl = ctk.CTkLabel(
            seg, text=icon(key), font=icon_font("sm", 4),
            text_color=self._colors.fg_secondary, fg_color=self._colors.bg_secondary,
        )
        icon_lbl.pack(side="left", padx=(24, 10), pady=4)
        text_lbl = ctk.CTkLabel(
            seg, text=label, font=font("sm", "bold"),
            text_color=self._colors.fg_secondary, fg_color=self._colors.bg_secondary,
        )
        text_lbl.pack(side="left", padx=(0, 24), pady=4)

        def _hover(on: bool) -> None:
            if self._current_tab != key:
                self._paint_segment(key, active=False, hover=on)

        for w in (seg, icon_lbl, text_lbl):
            w.bind("<Button-1>", lambda e, k=key: self._show_tab(k))
            w.bind("<Enter>", lambda e: _hover(True))
            w.bind("<Leave>", lambda e: _hover(False))
            try:
                w.configure(cursor="hand2")
            except Exception:
                pass
        return (seg, icon_lbl, text_lbl)

    def _refresh_topbar_status(self) -> None:
        """Update the top bar's model·hotkey·language line (tabs call this via
        ctx after settings changes)."""
        try:
            from .layout import status_summary
            self._status_lbl.configure(text=status_summary(self._settings))
        except Exception:
            pass

    def _paint_segment(self, key: str, active: bool, hover: bool = False) -> None:
        """Paint a nav segment consistently. Labels get EXPLICIT backgrounds —
        CTk 'transparent' labels don't repaint when the parent's fill changes,
        which left square patches breaking the pill's rounded corners."""
        seg, icon_lbl, text_lbl = self._nav_segments[key]
        if active:
            bg = self._colors.accent
            fg = self._colors.on_accent
        elif hover:
            bg = self._colors.bg_hover
            fg = self._colors.fg_primary
        else:
            bg = self._colors.bg_secondary
            fg = self._colors.fg_secondary
        seg.configure(fg_color=bg)
        for lbl in (icon_lbl, text_lbl):
            lbl.configure(fg_color=bg, text_color=fg)

    def _set_theme(self, mode: str) -> None:
        self._settings.theme = mode
        self._settings.save()

        # Update appearance mode and colors
        resolved = mode if mode != "system" else get_system_theme()
        ctk.set_appearance_mode(resolved)
        self._colors = get_colors(mode)

        # Rebuild entire UI with new colors
        self._rebuild_ui()
        apply_dark_title_bar(self, resolved == "dark")
        self._set_window_icon()
        self._apply_settings(reload_model=False)

    def _rebuild_ui(self) -> None:
        """Destroy and rebuild top bar + content to apply new theme colors."""
        current_tab = self._current_tab

        # Destroy existing UI
        for attr in ("_topbar", "_topbar_rule", "_content"):
            if hasattr(self, attr):
                getattr(self, attr).destroy()

        # Clear tab frame references
        self._tab_frames.clear()
        self._tabs.clear()
        self._nav_segments = {}
        self._last_col_count = 0
        self._tab_built_cols.clear()

        # Update window bg
        self.configure(fg_color=self._colors.bg_primary)

        # Rebuild — only the current tab; others rebuild lazily when shown
        self._build_topbar()
        self._build_content_area()
        self._show_tab(current_tab)

        # Re-bind resize
        self._content.bind("<Configure>", self._on_content_resize)

    # -- Content area ----------------------------------------------------------

    def _build_content_area(self) -> None:
        self._content = ctk.CTkFrame(
            self, fg_color=self._colors.bg_primary,
            corner_radius=0, border_width=0,
        )
        self._content.pack(side="top", fill="both", expand=True)

    def _show_tab(self, key: str) -> None:
        self._current_tab = key
        # Lazy build on first visit; rebuild if the tab was built for a
        # different column count (the old stale-layout-until-resize glitch)
        if key not in self._tab_frames:
            self._build_tab(key)
        elif self._tab_built_cols.get(key) != self._get_col_count():
            self.rebuild_tab(key)
        # Update nav highlight: accent-filled pill for the active segment,
        # accent-tinted gear when the Settings view is open.
        for k in getattr(self, "_nav_segments", {}):
            self._paint_segment(k, active=(k == key))
        if hasattr(self, "_gear_btn"):
            gear_active = key == "settings"
            self._gear_btn.configure(
                fg_color=self._colors.accent_muted if gear_active else "transparent",
                text_color=self._colors.accent if gear_active else self._colors.fg_secondary,
            )
        # Show the right frame
        for k, frame in self._tab_frames.items():
            if k == key:
                frame.pack(fill="both", expand=True)
            else:
                frame.pack_forget()

    def _build_tab(self, key: str) -> None:
        frame = self._make_tab_frame(key)
        tab_obj = TAB_CLASSES[key](self._ctx)
        self._tabs[key] = tab_obj
        tab_obj.build(frame)
        self._tab_built_cols[key] = self._get_col_count()

    def _change_model(self, new_model: str) -> None:
        """Switch models seamlessly — the model host swaps in the background (a
        fresh child process loads the new model), no app restart needed. The
        overlay shows 'Loading model…' on the next dictation until it's ready."""
        if new_model == self._settings.model_size:
            return
        self._settings.model_size = new_model
        self._settings.save()
        self.ctx.emit("model_changed", new_model)
        self._apply_settings(reload_model=True)

    def _mark_hidden_tabs_stale(self) -> None:
        for key in list(self._tab_built_cols):
            if key != self._current_tab:
                self._tab_built_cols[key] = -1  # forces rebuild on next show

    def rebuild_tab(self, key: str) -> None:
        """Tear down and rebuild one tab (responsive change or content refresh)."""
        if key not in self._tab_frames:
            return
        self._tab_frames[key].pack_forget()
        self._tab_frames.pop(key)
        self._build_tab(key)
        if self._current_tab == key:
            self._tab_frames[key].pack(fill="both", expand=True)

    # -- Responsive grid helpers -----------------------------------------------

    def _get_col_count(self) -> int:
        """Column count from content width (breakpoints scale with UI zoom —
        a zoomed column needs proportionally more real pixels)."""
        try:
            w = self._content.winfo_width()
        except Exception:
            w = WIN_W
        scale = getattr(self, "_ui_scale", 1.0)
        if w >= BP_3COL * scale:
            return 3
        elif w >= BP_2COL * scale:
            return 2
        return 1

    def set_ui_scale(self, scale: float) -> None:
        """Apply a new UI zoom factor and rebuild (called from the General tab)."""
        scale = max(1.0, min(2.0, scale))
        self._settings.ui_scale = scale
        self._settings.save()
        self._ui_scale = scale
        ctk.set_widget_scaling(scale)
        ctk.set_window_scaling(scale)
        self._rebuild_ui()

    def _on_content_resize(self, event=None) -> None:
        """Rebuild the current tab if column count changed — debounced, so a
        live drag-resize doesn't rebuild on every breakpoint crossing."""
        new_cols = self._get_col_count()
        if new_cols != self._last_col_count:
            self._last_col_count = new_cols
            if getattr(self, "_resize_job", None):
                try:
                    self.after_cancel(self._resize_job)
                except Exception:
                    pass
            self._resize_job = self.after(
                150, lambda: self.rebuild_tab(self._current_tab))

    def _make_tab_frame(self, key: str) -> ctk.CTkScrollableFrame:
        frame = ctk.CTkScrollableFrame(
            self._content,
            fg_color="transparent",
            corner_radius=0,
            scrollbar_button_color=self._colors.bg_tertiary,
            scrollbar_button_hover_color=self._colors.bg_hover,
        )
        try:
            # Smaller scroll increment — the default feels jumpy at UI zoom
            frame._parent_canvas.configure(yscrollincrement=5)
        except Exception:
            pass
        self._tab_frames[key] = frame
        # Add bottom spacer so content doesn't cut off at the edge
        self._tab_bottom_spacers[key] = frame
        return frame

    # -- Settings propagation ----------------------------------------------------

    def _apply_settings(self, reload_model: bool = False) -> None:
        """Run settings callback in background to avoid freezing the dashboard."""
        threading.Thread(
            target=self._on_settings_changed,
            kwargs={"reload_model": reload_model},
            daemon=True,
        ).start()

    # -- Lifecycle -------------------------------------------------------------

    def _schedule_heartbeat(self) -> None:
        try:
            from ..watchdog import beat
            beat("dashboard-ui")
            self.after(1000, self._schedule_heartbeat)
        except Exception:
            pass

    def _on_close(self) -> None:
        global _active_dashboard
        _active_dashboard = None
        try:
            from ..watchdog import clear
            clear("dashboard-ui")
        except Exception:
            pass
        general = self._tabs.get("general")
        if general is not None and getattr(general, "_hotkey_capturing", False):
            general._stop_hotkey_capture(cancelled=True)
        try:
            self.destroy()
        except Exception:
            pass
        if self._own_root:
            try:
                self._own_root.quit()
                self._own_root.destroy()
            except Exception:
                pass


# Singleton tracking
_active_dashboard: Optional[Dashboard] = None
_dashboard_lock = threading.Lock()


def shutdown_dashboard() -> None:
    """Close the active dashboard cleanly. Call before app exit."""
    global _active_dashboard
    with _dashboard_lock:
        dash = _active_dashboard
        if dash is not None:
            try:
                dash.after(0, dash._on_close)
            except Exception:
                pass
            _active_dashboard = None


def _focus_existing(dash: Dashboard) -> None:
    """Bring an existing dashboard window to front."""
    try:
        dash.deiconify()
        dash.lift()
        dash.focus_force()
        # Temporarily set topmost to steal focus, then disable
        dash.attributes("-topmost", True)
        dash.after(100, lambda: dash.attributes("-topmost", False))
    except Exception:
        pass


_opening = False  # prevents multiple threads launching at once


def open_dashboard(settings: Settings, on_settings_changed: Callable,
                   on_restart: Optional[Callable] = None,
                   initial_tab: Optional[str] = None) -> None:
    """Open the dashboard, or focus the existing one if already open."""
    global _active_dashboard, _opening

    with _dashboard_lock:
        if _active_dashboard is not None:
            try:
                _active_dashboard.after(0, lambda: _focus_existing(_active_dashboard))
                return
            except Exception:
                _active_dashboard = None

        if _opening:
            return
        _opening = True

    def _run():
        global _opening
        try:
            Dashboard(settings, on_settings_changed, on_restart=on_restart,
                      initial_tab=initial_tab)
        finally:
            _opening = False

    threading.Thread(target=_run, daemon=True).start()
