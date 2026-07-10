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
from .tabs import TAB_CLASSES

log = logging.getLogger(__name__)

# Window dimensions
WIN_W, WIN_H = 720, 520
SIDEBAR_W = 170

# Responsive breakpoints (content area width, excluding sidebar)
BP_2COL = 560   # 2 columns when content >= 560px
BP_3COL = 900   # 3 columns when content >= 900px

# -- Icons (clean Unicode symbols) ---------------------------------------------
TAB_ICONS = {
    "general":  "☰",   # ☰ (hamburger/settings)
    "overlay":  "□",   # □
    "models":   "◎",   # ◎
    "history":  "◷",   # ◷
    "about":    "ℹ",   # ℹ
}


class Dashboard(ctk.CTkToplevel):
    """Main Eqho settings dashboard window."""

    def __init__(
        self,
        settings: Settings,
        on_settings_changed: Callable,
        parent: Optional[ctk.CTk] = None,
    ):
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
        self._tab_bottom_spacers: dict[str, ctk.CTkFrame] = {}
        self._assets = Path(__file__).resolve().parent.parent.parent / "assets"

        self._ctx = DashboardContext(
            settings=settings,
            colors_getter=lambda: self._colors,
            apply_settings=self._apply_settings,
            get_col_count=self._get_col_count,
            rebuild_tab=self.rebuild_tab,
        )

        self._setup_window()
        self._build_sidebar()
        self._build_content_area()
        self._build_all_tabs()
        self._show_tab("general")

        # Responsive resize handler
        self._content.bind("<Configure>", self._on_content_resize)

        # Register as the active singleton BEFORE mainloop blocks
        global _active_dashboard
        _active_dashboard = self

        if self._own_root:
            self.protocol("WM_DELETE_WINDOW", self._on_close)
            self._own_root.mainloop()

    def _setup_window(self) -> None:
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

        # Center on screen
        self.update_idletasks()
        sw = self.winfo_screenwidth()
        sh = self.winfo_screenheight()
        x = (sw - WIN_W) // 2
        y = (sh - WIN_H) // 2
        self.geometry(f"{WIN_W}x{WIN_H}+{x}+{y}")

    def _set_window_icon(self) -> None:
        """Set the window title bar icon from the new logo PNGs."""
        try:
            from PIL import Image, ImageTk
            # Use 32px logo for title bar (standard Windows icon size)
            logo_path = self._assets / "logo_32_dark.png"
            if not logo_path.exists():
                logo_path = self._assets / "logo_32.png"
            if logo_path.exists():
                img = Image.open(logo_path).convert("RGBA")
                # Create multiple sizes for Windows (16, 32, 48)
                sizes = []
                for s in (16, 32, 48):
                    sizes.append(img.resize((s, s), Image.LANCZOS))
                tk_root = self._own_root if self._own_root else self
                self._icon_photos = [ImageTk.PhotoImage(s, master=tk_root) for s in sizes]
                self.iconphoto(False, *self._icon_photos)
                return
            # Fallback to legacy .ico
            ico_path = self._assets / "eqho.ico"
            if ico_path.exists():
                self.after(200, lambda: self.iconbitmap(str(ico_path)))
        except Exception as e:
            log.debug("Failed to set window icon: %s", e)

    # -- Sidebar ---------------------------------------------------------------

    def _build_sidebar(self) -> None:
        self._sidebar = ctk.CTkFrame(
            self, width=SIDEBAR_W, corner_radius=0,
            fg_color=self._colors.bg_secondary,
            border_width=0,
        )
        self._sidebar.pack(side="left", fill="y")
        self._sidebar.pack_propagate(False)

        # Logo
        title_frame = ctk.CTkFrame(self._sidebar, fg_color="transparent")
        title_frame.pack(fill="x", padx=SPACING["lg"], pady=(SPACING["xl"], SPACING["lg"]))

        logo_light = self._assets / "logo_horizontal_dark.png"
        logo_dark = self._assets / "logo_horizontal_light.png"
        if logo_light.exists() and logo_dark.exists():
            from PIL import Image, ImageTk
            pil_light = Image.open(logo_light)
            pil_dark = Image.open(logo_dark)
            # Scale to fit sidebar width with padding
            max_w = SIDEBAR_W - 2 * SPACING["lg"]
            ratio = max_w / pil_light.width
            new_h = int(pil_light.height * ratio)
            pil_light = pil_light.resize((max_w, new_h), Image.LANCZOS)
            pil_dark = pil_dark.resize((max_w, new_h), Image.LANCZOS)
            # Pick the right variant for current theme
            resolved = self._settings.theme
            if resolved == "system":
                resolved = get_system_theme()
            pil_img = pil_light if resolved == "light" else pil_dark
            # Bind PhotoImage to this window's Tk instance
            tk_root = self._own_root if self._own_root else self
            self._logo_tk = ImageTk.PhotoImage(pil_img, master=tk_root)
            logo_label = tk.Label(
                title_frame, image=self._logo_tk,
                bg=self._colors.bg_secondary, borderwidth=0,
            )
            logo_label.pack(anchor="w")
        else:
            ctk.CTkLabel(
                title_frame, text="Eqho",
                font=font("xl", "bold"),
                text_color=self._colors.fg_primary,
            ).pack(anchor="w")

        # Nav items with icons
        self._nav_buttons: dict[str, ctk.CTkButton] = {}
        tabs = [
            ("general", "General"),
            ("overlay", "Overlay"),
            ("models", "Models"),
            ("history", "History"),
            ("about", "About"),
        ]

        for key, label in tabs:
            icon = TAB_ICONS.get(key, "")
            btn = ctk.CTkButton(
                self._sidebar,
                text=f"  {icon}  {label}",
                font=font("base"),
                height=36,
                corner_radius=RADIUS_SM,
                fg_color="transparent",
                text_color=self._colors.fg_secondary,
                hover_color=self._colors.bg_hover,
                anchor="w",
                command=lambda k=key: self._show_tab(k),
            )
            btn.pack(fill="x", padx=SPACING["sm"], pady=2)
            self._nav_buttons[key] = btn

        # Spacer
        ctk.CTkFrame(self._sidebar, fg_color="transparent", height=1).pack(expand=True)

        # Theme switcher at bottom
        self._build_theme_switcher()

    def _build_theme_switcher(self) -> None:
        frame = ctk.CTkFrame(self._sidebar, fg_color="transparent")
        frame.pack(fill="x", padx=SPACING["sm"], pady=(0, SPACING["lg"]))

        ctk.CTkLabel(
            frame, text="Theme",
            font=font("xs"),
            text_color=self._colors.fg_muted,
        ).pack(anchor="w", padx=SPACING["sm"], pady=(0, 4))

        pill = ctk.CTkFrame(
            frame, corner_radius=RADIUS_MD,
            fg_color=self._colors.bg_tertiary,
            height=32,
        )
        pill.pack(fill="x", padx=4)
        pill.pack_propagate(False)

        # Three segments: Light, Dark, Auto — clean text labels
        self._theme_buttons: dict[str, ctk.CTkButton] = {}
        for mode, label in [("light", "Light"), ("dark", "Dark"), ("system", "System")]:
            is_active = self._settings.theme == mode
            btn = ctk.CTkButton(
                pill,
                text=label,
                width=40,
                height=26,
                corner_radius=RADIUS_SM,
                font=font("xs"),
                fg_color=self._colors.accent if is_active else "transparent",
                text_color=self._colors.on_accent if is_active else self._colors.fg_secondary,
                hover_color=self._colors.bg_hover,
                command=lambda m=mode: self._set_theme(m),
            )
            btn.pack(side="left", expand=True, fill="both", padx=2, pady=2)
            self._theme_buttons[mode] = btn

    def _set_theme(self, mode: str) -> None:
        self._settings.theme = mode
        self._settings.save()

        # Update appearance mode and colors
        resolved = mode if mode != "system" else get_system_theme()
        ctk.set_appearance_mode(resolved)
        self._colors = get_colors(mode)

        # Rebuild entire UI with new colors
        self._rebuild_ui()
        self._apply_settings(reload_model=False)

    def _rebuild_ui(self) -> None:
        """Destroy and rebuild sidebar + content to apply new theme colors."""
        current_tab = self._current_tab

        # Destroy existing UI
        if hasattr(self, "_sidebar"):
            self._sidebar.destroy()
        if hasattr(self, "_content"):
            self._content.destroy()

        # Clear tab frame references
        self._tab_frames.clear()
        self._tabs.clear()
        self._nav_buttons.clear()
        self._last_col_count = 0

        # Update window bg
        self.configure(fg_color=self._colors.bg_primary)

        # Rebuild
        self._build_sidebar()
        self._build_content_area()
        self._build_all_tabs()
        self._show_tab(current_tab)

        # Re-bind resize
        self._content.bind("<Configure>", self._on_content_resize)

    # -- Content area ----------------------------------------------------------

    def _build_content_area(self) -> None:
        self._content = ctk.CTkFrame(
            self, fg_color=self._colors.bg_primary,
            corner_radius=0, border_width=0,
        )
        self._content.pack(side="right", fill="both", expand=True)

    def _show_tab(self, key: str) -> None:
        self._current_tab = key
        # Update nav highlight
        for k, btn in self._nav_buttons.items():
            if k == key:
                btn.configure(
                    fg_color=self._colors.accent_muted,
                    text_color=self._colors.accent,
                )
            else:
                btn.configure(
                    fg_color="transparent",
                    text_color=self._colors.fg_secondary,
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

    def _build_all_tabs(self) -> None:
        for key in TAB_CLASSES:
            self._build_tab(key)

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
        """Determine column count from content area width."""
        try:
            w = self._content.winfo_width()
        except Exception:
            w = WIN_W - SIDEBAR_W
        if w >= BP_3COL:
            return 3
        elif w >= BP_2COL:
            return 2
        return 1

    def _on_content_resize(self, event=None) -> None:
        """Rebuild the current tab if column count changed."""
        new_cols = self._get_col_count()
        if new_cols != self._last_col_count:
            self._last_col_count = new_cols
            self.rebuild_tab(self._current_tab)

    def _make_tab_frame(self, key: str) -> ctk.CTkScrollableFrame:
        frame = ctk.CTkScrollableFrame(
            self._content,
            fg_color="transparent",
            corner_radius=0,
            scrollbar_button_color=self._colors.bg_tertiary,
            scrollbar_button_hover_color=self._colors.bg_hover,
        )
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

    def _on_close(self) -> None:
        global _active_dashboard
        _active_dashboard = None
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


def open_dashboard(settings: Settings, on_settings_changed: Callable) -> None:
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
            Dashboard(settings, on_settings_changed)
        finally:
            _opening = False

    threading.Thread(target=_run, daemon=True).start()
