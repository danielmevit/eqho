"""Eqho Dashboard — main settings window.

A modern, themed settings dashboard with sidebar navigation.
Built on customtkinter for rounded widgets and native theme support.
Responsive grid layout: 1→2→3 columns based on window width.
"""

import logging
import os
import sys
import threading
import tkinter as tk
from pathlib import Path
from typing import Callable, Optional

import customtkinter as ctk

from .audio import list_input_devices, device_name
from .fonts import FONT_FAMILY
from .settings import (
    Settings, SUPPORTED_LANGUAGES, WHISPER_MODELS, HOTKEY_MODES,
    VOLUME_DUCK_OPTIONS, OVERLAY_POSITIONS, MODEL_CACHE_DIR,
)
from .theme import (
    get_colors, get_system_theme, ThemeColors, MODEL_INFO,
    RADIUS_SM, RADIUS_MD, RADIUS_LG, RADIUS_XL,
    FONT_SIZES, SPACING, ACCENT,
)
from .version import __version__

log = logging.getLogger(__name__)

# Window dimensions
WIN_W, WIN_H = 720, 520
SIDEBAR_W = 170

# Responsive breakpoints (content area width, excluding sidebar)
BP_2COL = 560   # 2 columns when content >= 560px
BP_3COL = 900   # 3 columns when content >= 900px

# -- Icons (clean Unicode symbols) ---------------------------------------------
TAB_ICONS = {
    "general":  "\u2630",   # ☰ (hamburger/settings)
    "overlay":  "\u25A1",   # □
    "models":   "\u25CE",   # ◎
    "history":  "\u25F7",   # ◷
    "about":    "\u2139",   # ℹ
}

SECTION_ICONS = {
    "AUDIO INPUT":      "\u25B6",   # ▶
    "HOTKEY":           "\u25B6",   # ▶
    "MODEL":            "\u25B6",   # ▶
    "BEHAVIOR":         "\u25B6",   # ▶
    "VISIBILITY":       "\u25B6",   # ▶
    "POSITION":         "\u25B6",   # ▶
    "APPEARANCE":       "\u25B6",   # ▶
    "ENGLISH OPTIMIZED":"\u25B6",   # ▶
    "MULTILINGUAL":     "\u25B6",   # ▶
    "PLANNED FEATURES": "\u25B6",   # ▶
    "POWERED BY":       "\u25B6",   # ▶
}


class ThemedDropdown(ctk.CTkFrame):
    """Custom dropdown menu that replaces native tkinter.Menu with themed popups.

    Renders as a button that opens a floating CTkFrame with selectable items.
    Supports rounded corners and full theme control.
    """

    def __init__(
        self,
        parent,
        values: list[str],
        variable: Optional[ctk.StringVar] = None,
        command=None,
        width: int = 160,
        height: int = 30,
        corner_radius: int = RADIUS_SM,
        font=None,
        dropdown_font=None,
        fg_color=None,
        text_color=None,
        button_color=None,
        button_hover_color=None,
        dropdown_fg_color=None,
        dropdown_hover_color=None,
        dropdown_text_color=None,
        **kwargs,
    ):
        super().__init__(parent, fg_color="transparent", width=width, height=height)
        self._values = values
        self._variable = variable
        self._command = command
        self._dropdown_open = False
        self._popup = None
        self._font = font or (FONT_FAMILY, FONT_SIZES["sm"])
        self._dropdown_font = dropdown_font or self._font
        self._fg_color = button_color or fg_color or "#333"
        self._hover_color = button_hover_color or "#444"
        self._dd_fg = dropdown_fg_color or "#222"
        self._dd_hover = dropdown_hover_color or "#444"
        self._dd_text = dropdown_text_color or "#eee"
        self._text_color = text_color or "#eee"
        self._corner_radius = corner_radius
        self._btn_width = width
        self._btn_height = height

        current = variable.get() if variable else (values[0] if values else "")

        self._button = ctk.CTkButton(
            self,
            text=f"{current}  \u25BE",
            width=width,
            height=height,
            corner_radius=corner_radius,
            font=self._font,
            fg_color=self._fg_color,
            text_color=self._text_color,
            hover_color=self._hover_color,
            anchor="w",
            command=self._toggle_popup,
        )
        self._button.pack()

        if self._variable:
            self._variable.trace_add("write", self._on_var_changed)

    def _on_var_changed(self, *args) -> None:
        val = self._variable.get()
        self._button.configure(text=f"{val}  \u25BE")

    def _toggle_popup(self) -> None:
        if self._dropdown_open:
            self._close_popup()
        else:
            self._open_popup()

    def _open_popup(self) -> None:
        if self._popup is not None:
            self._close_popup()

        self._dropdown_open = True

        # Create a toplevel for the dropdown
        self._popup = tk.Toplevel(self.winfo_toplevel())
        self._popup.withdraw()
        self._popup.overrideredirect(True)
        self._popup.attributes("-topmost", True)

        # Container frame with rounded appearance
        container = ctk.CTkFrame(
            self._popup,
            fg_color=self._dd_fg,
            corner_radius=self._corner_radius,
            border_width=1,
            border_color=self._hover_color,
        )
        container.pack(fill="both", expand=True, padx=1, pady=1)

        # Scrollable if many items
        max_visible = 8
        item_h = 28
        visible_count = min(len(self._values), max_visible)
        popup_h = visible_count * item_h + 8

        if len(self._values) > max_visible:
            scroll = ctk.CTkScrollableFrame(
                container, fg_color="transparent",
                height=popup_h,
                scrollbar_button_color=self._hover_color,
            )
            scroll.pack(fill="both", expand=True, padx=2, pady=4)
            items_parent = scroll
        else:
            items_parent = container

        current_val = self._variable.get() if self._variable else ""

        for val in self._values:
            is_selected = val == current_val
            btn = ctk.CTkButton(
                items_parent,
                text=val,
                font=self._dropdown_font,
                height=item_h,
                corner_radius=4,
                fg_color=self._dd_hover if is_selected else "transparent",
                text_color=self._dd_text,
                hover_color=self._dd_hover,
                anchor="w",
                command=lambda v=val: self._select_value(v),
            )
            btn.pack(fill="x", padx=4, pady=1)

        # Position below the button
        self.update_idletasks()
        x = self._button.winfo_rootx()
        y = self._button.winfo_rooty() + self._button.winfo_height() + 2
        popup_w = max(self._btn_width, 160)

        # Ensure popup doesn't go off-screen
        screen_h = self.winfo_screenheight()
        if y + popup_h > screen_h - 40:
            y = self._button.winfo_rooty() - popup_h - 2

        self._popup.geometry(f"{popup_w}x{popup_h}+{x}+{y}")
        self._popup.deiconify()

        # Apply Windows 11 rounded corners
        try:
            import ctypes
            hwnd = ctypes.windll.user32.GetParent(self._popup.winfo_id())
            DWMWA_WINDOW_CORNER_PREFERENCE = 33
            DWMWCP_ROUND = 2
            preference = ctypes.c_int(DWMWCP_ROUND)
            ctypes.windll.dwmapi.DwmSetWindowAttribute(
                hwnd, DWMWA_WINDOW_CORNER_PREFERENCE,
                ctypes.byref(preference), ctypes.sizeof(preference),
            )
        except Exception:
            pass

        # Close on click outside
        self._popup.bind("<FocusOut>", lambda e: self.after(100, self._close_popup))
        self._popup.focus_set()

    def _select_value(self, val: str) -> None:
        if self._variable:
            self._variable.set(val)
        self._button.configure(text=f"{val}  \u25BE")
        self._close_popup()
        if self._command:
            self._command(val)

    def _close_popup(self) -> None:
        self._dropdown_open = False
        if self._popup is not None:
            try:
                self._popup.destroy()
            except Exception:
                pass
            self._popup = None

    def pack(self, **kwargs):
        super().pack(**kwargs)

    def set(self, value: str) -> None:
        if self._variable:
            self._variable.set(value)
        self._button.configure(text=f"{value}  \u25BE")

    def get(self) -> str:
        return self._variable.get() if self._variable else ""


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
        self._hotkey_capturing = False
        self._hotkey_hook = None
        self._captured_keys: set[str] = set()
        self._last_col_count = 0  # track responsive state
        self._tab_bottom_spacers: dict[str, ctk.CTkFrame] = {}
        self._assets = Path(__file__).resolve().parent.parent / "assets"

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

    def _get_logo_path(self) -> Optional[Path]:
        """Return the correct horizontal logo for the current theme."""
        resolved = self._settings.theme
        if resolved == "system":
            resolved = get_system_theme()
        if resolved == "light":
            return self._assets / "logo_horizontal_dark.png"
        return self._assets / "logo_horizontal_light.png"

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
                font=(FONT_FAMILY, FONT_SIZES["xl"], "bold"),
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
                font=(FONT_FAMILY, FONT_SIZES["base"]),
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
            font=(FONT_FAMILY, FONT_SIZES["xs"]),
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
                font=(FONT_FAMILY, FONT_SIZES["xs"]),
                fg_color=self._colors.accent if is_active else "transparent",
                text_color="#ffffff" if is_active else self._colors.fg_secondary,
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

    def _build_all_tabs(self) -> None:
        self._build_general_tab()
        self._build_overlay_tab()
        self._build_models_tab()
        self._build_history_tab()
        self._build_about_tab()

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
            self._rebuild_current_tab()

    def _rebuild_current_tab(self) -> None:
        """Rebuild only the active tab for responsiveness."""
        key = self._current_tab
        if key not in self._tab_frames:
            return
        self._tab_frames[key].pack_forget()
        self._tab_frames.pop(key)

        builders = {
            "general": self._build_general_tab,
            "overlay": self._build_overlay_tab,
            "models": self._build_models_tab,
            "history": self._build_history_tab,
            "about": self._build_about_tab,
        }
        builders[key]()
        self._tab_frames[key].pack(fill="both", expand=True)

    # -- Helpers for building UI -----------------------------------------------

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

    def _add_bottom_padding(self, tab) -> None:
        """Add breathing room at the bottom of a tab."""
        ctk.CTkFrame(tab, fg_color="transparent", height=SPACING["2xl"]).pack(fill="x")

    def _tab_header(self, parent, title: str, subtitle: str) -> None:
        """Standard tab header with title and status info."""
        header = ctk.CTkFrame(parent, fg_color="transparent")
        header.pack(fill="x", padx=SPACING["xl"], pady=(SPACING["xl"], SPACING["xs"]))

        ctk.CTkLabel(
            header, text=title,
            font=(FONT_FAMILY, FONT_SIZES["2xl"], "bold"),
            text_color=self._colors.fg_primary, anchor="w",
        ).pack(anchor="w")

        # Status bar: model + hotkey + language
        from .settings import SUPPORTED_LANGUAGES
        model_name = MODEL_INFO.get(self._settings.model_size, {}).get("name", self._settings.model_size)
        lang = SUPPORTED_LANGUAGES.get(self._settings.language, self._settings.language)
        hotkey = self._format_hotkey(self._settings.hotkey)
        status = f"{model_name}  \u00b7  {hotkey}  \u00b7  {lang}  \u00b7  {self._settings.hotkey_mode}"

        ctk.CTkLabel(
            header, text=status,
            font=(FONT_FAMILY, FONT_SIZES["xs"]),
            text_color=self._colors.fg_muted, anchor="w",
        ).pack(anchor="w", pady=(2, 0))

        ctk.CTkLabel(
            parent, text=subtitle,
            font=(FONT_FAMILY, FONT_SIZES["sm"]),
            text_color=self._colors.fg_secondary, anchor="w",
        ).pack(fill="x", padx=SPACING["xl"], pady=(0, SPACING["md"]))

    def _section_label(self, parent, text: str) -> ctk.CTkLabel:
        icon = SECTION_ICONS.get(text, "\u25B6")
        lbl = ctk.CTkLabel(
            parent, text=f"{icon}  {text}",
            font=(FONT_FAMILY, FONT_SIZES["sm"], "bold"),
            text_color=self._colors.fg_secondary,
            anchor="w",
        )
        lbl.pack(fill="x", padx=SPACING["xl"], pady=(SPACING["lg"], 4))
        return lbl

    def _dropdown_colors(self) -> dict:
        """Common styling kwargs for CTkOptionMenu dropdowns."""
        return {
            "button_color": self._colors.bg_tertiary,
            "button_hover_color": self._colors.bg_hover,
            "dropdown_fg_color": self._colors.bg_secondary,
            "dropdown_hover_color": self._colors.bg_hover,
            "dropdown_text_color": self._colors.fg_primary,
            "fg_color": self._colors.bg_tertiary,
            "text_color": self._colors.fg_primary,
        }

    def _dropdown(self, parent, **kwargs) -> ThemedDropdown:
        """Create a themed dropdown with rounded corners and full theme control."""
        merged = {**self._dropdown_colors(), **kwargs}
        return ThemedDropdown(parent, **merged)

    def _card(self, parent) -> ctk.CTkFrame:
        card = ctk.CTkFrame(
            parent,
            corner_radius=RADIUS_MD,
            fg_color=self._colors.bg_secondary,
            border_width=1,
            border_color=self._colors.border_subtle,
        )
        card.pack(fill="x", padx=SPACING["xl"], pady=(SPACING["xs"], 0))
        return card

    def _card_grid(self, parent, col: int, row: int) -> ctk.CTkFrame:
        """A card placed into a grid layout."""
        card = ctk.CTkFrame(
            parent,
            corner_radius=RADIUS_MD,
            fg_color=self._colors.bg_secondary,
            border_width=1,
            border_color=self._colors.border_subtle,
        )
        card.grid(row=row, column=col, sticky="nsew",
                  padx=SPACING["xs"], pady=SPACING["xs"])
        return card

    def _setting_row(self, parent, label: str, description: str = "") -> ctk.CTkFrame:
        """A row inside a card: label on the left, widget slot on the right."""
        row = ctk.CTkFrame(parent, fg_color="transparent")
        row.pack(fill="x", padx=SPACING["lg"], pady=SPACING["sm"])

        left = ctk.CTkFrame(row, fg_color="transparent")
        left.pack(side="left", fill="x", expand=True)

        ctk.CTkLabel(
            left, text=label,
            font=(FONT_FAMILY, FONT_SIZES["base"]),
            text_color=self._colors.fg_primary,
            anchor="w",
        ).pack(anchor="w")

        if description:
            ctk.CTkLabel(
                left, text=description,
                font=(FONT_FAMILY, FONT_SIZES["xs"]),
                text_color=self._colors.fg_muted,
                anchor="w",
            ).pack(anchor="w")

        right = ctk.CTkFrame(row, fg_color="transparent")
        right.pack(side="right")
        return right

    def _make_grid_container(self, parent) -> ctk.CTkFrame:
        """Create a grid container that distributes columns evenly."""
        grid = ctk.CTkFrame(parent, fg_color="transparent")
        grid.pack(fill="both", expand=True, padx=SPACING["lg"], pady=SPACING["xs"])
        cols = self._get_col_count()
        for c in range(cols):
            grid.columnconfigure(c, weight=1, uniform="col")
        return grid

    # ==========================================================================
    # GENERAL TAB
    # ==========================================================================

    def _build_general_tab(self) -> None:
        tab = self._make_tab_frame("general")
        self._tab_header(tab, "General", "Core settings for dictation")

        cols = self._get_col_count()

        if cols == 1:
            self._build_general_1col(tab)
        else:
            self._build_general_multi(tab, cols)
        self._add_bottom_padding(tab)

    def _build_general_1col(self, tab) -> None:
        """Single-column layout (narrow window)."""
        # Audio Input
        self._section_label(tab, "AUDIO INPUT")
        card = self._card(tab)
        self._build_mic_setting(card)

        # Hotkey
        self._section_label(tab, "HOTKEY")
        card = self._card(tab)
        self._build_hotkey_setting(card)
        self._build_hotkey_mode_setting(card)

        # Model
        self._section_label(tab, "MODEL")
        card = self._card(tab)
        self._build_model_setting(card)

        # Behavior
        self._section_label(tab, "BEHAVIOR")
        card = self._card(tab)
        self._build_behavior_settings(card)

    def _build_general_multi(self, tab, cols: int) -> None:
        """Multi-column grid layout."""
        grid = self._make_grid_container(tab)

        # Distribute sections across columns
        # Col 0: Audio + Hotkey
        col0 = ctk.CTkFrame(grid, fg_color="transparent")
        col0.grid(row=0, column=0, sticky="nsew", padx=SPACING["xs"])

        self._section_label(col0, "AUDIO INPUT")
        card = self._card(col0)
        self._build_mic_setting(card)

        self._section_label(col0, "HOTKEY")
        card = self._card(col0)
        self._build_hotkey_setting(card)
        self._build_hotkey_mode_setting(card)

        # Col 1: Model + Behavior
        col1 = ctk.CTkFrame(grid, fg_color="transparent")
        col1.grid(row=0, column=1, sticky="nsew", padx=SPACING["xs"])

        self._section_label(col1, "MODEL")
        card = self._card(col1)
        self._build_model_setting(card)

        self._section_label(col1, "BEHAVIOR")
        card = self._card(col1)
        self._build_behavior_settings(card)

        # Col 2 (if 3-col): pull behavior out, add language/startup separately
        if cols >= 3:
            col2 = ctk.CTkFrame(grid, fg_color="transparent")
            col2.grid(row=0, column=2, sticky="nsew", padx=SPACING["xs"])

            self._section_label(col2, "BEHAVIOR")
            card = self._card(col2)
            self._build_language_setting(card)
            self._build_startup_setting(card)

    # -- General tab building blocks (reusable in any layout) ------------------

    def _build_mic_setting(self, card) -> None:
        right = self._setting_row(card, "Microphone", "Select your input device")

        devices = list_input_devices()
        device_names = ["System Default"] + [name for _, name in devices]
        device_indices = [None] + [idx for idx, _ in devices]

        current_idx = self._settings.audio_device
        current_name = "System Default"
        for idx, name in devices:
            if idx == current_idx:
                current_name = name
                break

        self._mic_var = ctk.StringVar(value=current_name)
        self._dropdown(
            right,
            variable=self._mic_var,
            values=device_names,
            width=200, height=30,
            corner_radius=RADIUS_SM,
            font=(FONT_FAMILY, FONT_SIZES["sm"]),
            dropdown_font=(FONT_FAMILY, FONT_SIZES["sm"]),
            command=lambda val: self._on_mic_changed(val, device_names, device_indices),
        ).pack()

    def _build_hotkey_setting(self, card) -> None:
        hotkey_row = ctk.CTkFrame(card, fg_color="transparent")
        hotkey_row.pack(fill="x", padx=SPACING["lg"], pady=SPACING["sm"])

        left = ctk.CTkFrame(hotkey_row, fg_color="transparent")
        left.pack(side="left", fill="x", expand=True)
        ctk.CTkLabel(
            left, text="Global Hotkey",
            font=(FONT_FAMILY, FONT_SIZES["base"]),
            text_color=self._colors.fg_primary, anchor="w",
        ).pack(anchor="w")
        ctk.CTkLabel(
            left, text="Press to activate/deactivate dictation",
            font=(FONT_FAMILY, FONT_SIZES["xs"]),
            text_color=self._colors.fg_muted, anchor="w",
        ).pack(anchor="w")

        hotkey_right = ctk.CTkFrame(hotkey_row, fg_color="transparent")
        hotkey_right.pack(side="right")

        self._hotkey_display = ctk.CTkButton(
            hotkey_right,
            text=self._format_hotkey(self._settings.hotkey),
            font=(FONT_FAMILY, FONT_SIZES["lg"], "bold"),
            width=160, height=36,
            corner_radius=RADIUS_SM,
            fg_color=self._colors.bg_tertiary,
            text_color=self._colors.accent,
            hover_color=self._colors.bg_hover,
            border_width=1,
            border_color=self._colors.border,
            command=self._start_hotkey_capture,
        )
        self._hotkey_display.pack()

        self._hotkey_hint = ctk.CTkLabel(
            hotkey_right, text="Click to change",
            font=(FONT_FAMILY, FONT_SIZES["xs"]),
            text_color=self._colors.fg_muted,
        )
        self._hotkey_hint.pack(pady=(2, 0))

    def _build_hotkey_mode_setting(self, card) -> None:
        right = self._setting_row(card, "Hotkey Mode", "Toggle on/off or hold to talk")
        self._mode_var = ctk.StringVar(value=self._settings.hotkey_mode)
        ctk.CTkSegmentedButton(
            right,
            values=["toggle", "hold"],
            variable=self._mode_var,
            font=(FONT_FAMILY, FONT_SIZES["sm"]),
            corner_radius=RADIUS_SM,
            command=self._on_mode_changed,
        ).pack()

    def _build_model_setting(self, card) -> None:
        model_row = ctk.CTkFrame(card, fg_color="transparent")
        model_row.pack(fill="x", padx=SPACING["lg"], pady=SPACING["sm"])

        model_left = ctk.CTkFrame(model_row, fg_color="transparent")
        model_left.pack(side="left", fill="x", expand=True)

        ctk.CTkLabel(
            model_left, text="Whisper Model",
            font=(FONT_FAMILY, FONT_SIZES["base"]),
            text_color=self._colors.fg_primary, anchor="w",
        ).pack(anchor="w")

        self._model_info_label = ctk.CTkLabel(
            model_left, text=self._get_model_info_text(self._settings.model_size),
            font=(FONT_FAMILY, FONT_SIZES["xs"]),
            text_color=self._colors.fg_muted, anchor="w",
        )
        self._model_info_label.pack(anchor="w")

        model_right = ctk.CTkFrame(model_row, fg_color="transparent")
        model_right.pack(side="right")

        model_display_names = [MODEL_INFO[k]["name"] for k in WHISPER_MODELS]
        model_keys = list(WHISPER_MODELS.keys())
        current_model_name = MODEL_INFO.get(self._settings.model_size, {}).get("name", "Unknown")

        self._model_var = ctk.StringVar(value=current_model_name)
        self._model_keys = model_keys
        self._model_display_names = model_display_names

        self._dropdown(
            model_right,
            variable=self._model_var,
            values=model_display_names,
            width=180, height=30,
            corner_radius=RADIUS_SM,
            font=(FONT_FAMILY, FONT_SIZES["sm"]),
            dropdown_font=(FONT_FAMILY, FONT_SIZES["sm"]),
            command=self._on_model_changed,
        ).pack()

    def _build_behavior_settings(self, card) -> None:
        # Volume while speaking
        right = self._setting_row(card, "Volume While Speaking", "System volume during dictation")
        duck_labels = {"off": "Off", "50%": "50%", "25%": "25%", "10%": "10%", "mute": "Mute"}
        self._duck_var = ctk.StringVar(value=duck_labels.get(self._settings.volume_duck, "Mute"))
        self._dropdown(
            right,
            variable=self._duck_var,
            values=list(duck_labels.values()),
            width=100, height=30,
            corner_radius=RADIUS_SM,
            font=(FONT_FAMILY, FONT_SIZES["sm"]),
            dropdown_font=(FONT_FAMILY, FONT_SIZES["sm"]),
            command=lambda val: self._on_duck_changed(val, duck_labels),
        ).pack()

        # Paste mode
        right = self._setting_row(card, "Paste Mode", "How text is injected into apps")
        self._paste_var = ctk.StringVar(value="Clipboard" if self._settings.auto_paste else "Typing")
        ctk.CTkSegmentedButton(
            right,
            values=["Clipboard", "Typing"],
            variable=self._paste_var,
            font=(FONT_FAMILY, FONT_SIZES["sm"]),
            corner_radius=RADIUS_SM,
            command=self._on_paste_changed,
        ).pack()

        # Language
        self._build_language_setting(card)

        # Start with Windows
        self._build_startup_setting(card)

    def _build_language_setting(self, card) -> None:
        right = self._setting_row(card, "Language", "Transcription language")
        lang_names = list(SUPPORTED_LANGUAGES.values())
        lang_codes = list(SUPPORTED_LANGUAGES.keys())
        current_lang = SUPPORTED_LANGUAGES.get(self._settings.language, "English")
        self._lang_var = ctk.StringVar(value=current_lang)
        self._lang_codes = lang_codes
        self._lang_names = lang_names

        self._dropdown(
            right,
            variable=self._lang_var,
            values=lang_names,
            width=140, height=30,
            corner_radius=RADIUS_SM,
            font=(FONT_FAMILY, FONT_SIZES["sm"]),
            dropdown_font=(FONT_FAMILY, FONT_SIZES["sm"]),
            command=self._on_lang_changed,
        ).pack()

    def _build_startup_setting(self, card) -> None:
        right = self._setting_row(card, "Start with Windows", "Launch Eqho on login")
        self._startup_var = ctk.BooleanVar(value=self._settings.start_with_windows)
        ctk.CTkSwitch(
            right, text="", variable=self._startup_var,
            onvalue=True, offvalue=False,
            command=self._on_startup_changed,
            width=44, height=22,
        ).pack()

    # -- General tab callbacks -------------------------------------------------

    def _format_hotkey(self, combo: str) -> str:
        return " + ".join(p.strip().title() for p in combo.split("+"))

    def _get_model_info_text(self, model_key: str) -> str:
        info = MODEL_INFO.get(model_key, {})
        icon = info.get("icon", "")
        lang = info.get("lang", "")
        size = info.get("size", "")
        device = info.get("device", "")
        rec = info.get("rec", "")
        cached = self._is_model_cached(model_key)
        status = "Downloaded" if cached else "Not downloaded"
        return f"{icon} {lang} \u00b7 {size} \u00b7 {device}\n{rec} \u00b7 {status}"

    def _is_model_cached(self, model_key: str) -> bool:
        for candidate in [
            MODEL_CACHE_DIR / model_key,
            MODEL_CACHE_DIR / f"models--Systran--faster-whisper-{model_key}",
            MODEL_CACHE_DIR / f"models--ctranslate2-4you--distil-whisper-{model_key}",
            MODEL_CACHE_DIR / "huggingface" / f"models--Systran--faster-whisper-{model_key}",
            MODEL_CACHE_DIR / "huggingface" / f"models--ctranslate2-4you--distil-whisper-{model_key}",
        ]:
            if candidate.exists():
                return True
        return False

    def _apply_settings(self, reload_model: bool = False) -> None:
        """Run settings callback in background to avoid freezing the dashboard."""
        threading.Thread(
            target=self._on_settings_changed,
            kwargs={"reload_model": reload_model},
            daemon=True,
        ).start()

    def _on_mic_changed(self, val, names, indices) -> None:
        idx = indices[names.index(val)] if val in names else None
        self._settings.audio_device = idx
        self._settings.save()
        self._apply_settings(reload_model=True)

    def _on_mode_changed(self, val) -> None:
        self._settings.hotkey_mode = val
        self._settings.save()
        self._apply_settings(reload_model=False)

    def _on_model_changed(self, display_name) -> None:
        idx = self._model_display_names.index(display_name)
        key = self._model_keys[idx]
        if key != self._settings.model_size:
            self._settings.model_size = key
            self._settings.save()
            self._model_info_label.configure(text=self._get_model_info_text(key))
            self._apply_settings(reload_model=True)

    def _on_duck_changed(self, val, labels) -> None:
        reverse = {v: k for k, v in labels.items()}
        key = reverse.get(val, "mute")
        self._settings.volume_duck = key
        self._settings.save()

    def _on_paste_changed(self, val) -> None:
        self._settings.auto_paste = val == "Clipboard"
        self._settings.save()

    def _on_lang_changed(self, val) -> None:
        idx = self._lang_names.index(val)
        code = self._lang_codes[idx]
        self._settings.language = code
        self._settings.save()
        self._apply_settings(reload_model=True)

    def _on_startup_changed(self) -> None:
        enabled = self._startup_var.get()
        self._settings.start_with_windows = enabled
        self._settings.save()
        try:
            import winreg
            key_path = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run"
            with winreg.OpenKey(winreg.HKEY_CURRENT_USER, key_path, 0,
                                winreg.KEY_SET_VALUE | winreg.KEY_READ) as key:
                if enabled:
                    if getattr(sys, "frozen", False):
                        cmd = f'"{sys.executable}"'
                    else:
                        cmd = f'"{sys.executable}" "{Path(sys.argv[0]).resolve()}"'
                    winreg.SetValueEx(key, "Eqho", 0, winreg.REG_SZ, cmd)
                else:
                    try:
                        winreg.DeleteValue(key, "Eqho")
                    except FileNotFoundError:
                        pass
        except Exception as e:
            log.error("Failed to update startup registry: %s", e)

    # -- Hotkey capture --------------------------------------------------------

    def _start_hotkey_capture(self) -> None:
        if self._hotkey_capturing:
            return
        import keyboard
        self._hotkey_capturing = True
        self._captured_keys.clear()
        self._hotkey_display.configure(
            text="Press a key combo...",
            fg_color=self._colors.accent_muted,
            text_color=self._colors.danger,
        )
        self._hotkey_hint.configure(text="Esc to cancel")
        self._hotkey_hook = keyboard.hook(self._on_hotkey_event, suppress=False)

    def _on_hotkey_event(self, event) -> None:
        import keyboard as kb
        name = event.name.lower() if event.name else ""
        if name == "esc":
            self._stop_hotkey_capture(cancelled=True)
            return

        if event.event_type == kb.KEY_DOWN:
            self._captured_keys.add(name)
            combo = self._keys_to_combo(self._captured_keys)
            try:
                self.after(0, lambda: self._hotkey_display.configure(
                    text=self._format_hotkey(combo), text_color=self._colors.accent))
            except Exception:
                pass
        elif event.event_type == kb.KEY_UP:
            if self._captured_keys:
                combo = self._keys_to_combo(self._captured_keys)
                self._settings.hotkey = combo
                self._settings.save()
                self._stop_hotkey_capture(cancelled=False)
                self._apply_settings(reload_model=False)

    def _keys_to_combo(self, keys: set[str]) -> str:
        mod_names = {"ctrl", "left ctrl", "right ctrl", "alt", "left alt", "right alt",
                     "shift", "left shift", "right shift", "left windows", "right windows"}
        mod_map = {
            "left ctrl": "ctrl", "right ctrl": "ctrl",
            "left alt": "alt", "right alt": "alt",
            "left shift": "shift", "right shift": "shift",
            "left windows": "win", "right windows": "win",
        }
        modifiers, regular = [], []
        for k in keys:
            if k in mod_names:
                canonical = mod_map.get(k, k)
                if canonical not in modifiers:
                    modifiers.append(canonical)
            else:
                regular.append(k)
        mod_order = {"ctrl": 0, "alt": 1, "shift": 2, "win": 3}
        modifiers.sort(key=lambda m: mod_order.get(m, 9))
        return "+".join(modifiers + regular)

    def _stop_hotkey_capture(self, cancelled: bool) -> None:
        import keyboard
        self._hotkey_capturing = False
        if self._hotkey_hook is not None:
            try:
                keyboard.unhook(self._hotkey_hook)
            except Exception:
                pass
            self._hotkey_hook = None

        combo = self._settings.hotkey
        try:
            self.after(0, lambda: [
                self._hotkey_display.configure(
                    text=self._format_hotkey(combo),
                    fg_color=self._colors.bg_tertiary,
                    text_color=self._colors.accent,
                ),
                self._hotkey_hint.configure(text="Click to change"),
            ])
        except Exception:
            pass

    # ==========================================================================
    # OVERLAY TAB
    # ==========================================================================

    def _build_overlay_tab(self) -> None:
        tab = self._make_tab_frame("overlay")
        self._tab_header(tab, "Overlay", "Floating transcription preview bar")

        cols = self._get_col_count()

        if cols >= 2:
            grid = self._make_grid_container(tab)

            col0 = ctk.CTkFrame(grid, fg_color="transparent")
            col0.grid(row=0, column=0, sticky="nsew", padx=SPACING["xs"])

            self._section_label(col0, "VISIBILITY")
            card = self._card(col0)
            right = self._setting_row(card, "Show Overlay", "Display transcription text while dictating")
            self._overlay_var = ctk.BooleanVar(value=self._settings.overlay_enabled)
            ctk.CTkSwitch(
                right, text="", variable=self._overlay_var,
                onvalue=True, offvalue=False,
                command=self._on_overlay_toggle,
                width=44, height=22,
            ).pack()

            self._section_label(col0, "POSITION")
            card = self._card(col0)
            self._build_position_setting(card)

            col1 = ctk.CTkFrame(grid, fg_color="transparent")
            col1.grid(row=0, column=1, sticky="nsew", padx=SPACING["xs"])

            self._section_label(col1, "APPEARANCE")
            card = self._card(col1)
            self._build_opacity_setting(card)
            self._build_fontsize_setting(card)
        else:
            # Single column
            self._section_label(tab, "VISIBILITY")
            card = self._card(tab)
            right = self._setting_row(card, "Show Overlay", "Display transcription text while dictating")
            self._overlay_var = ctk.BooleanVar(value=self._settings.overlay_enabled)
            ctk.CTkSwitch(
                right, text="", variable=self._overlay_var,
                onvalue=True, offvalue=False,
                command=self._on_overlay_toggle,
                width=44, height=22,
            ).pack()

            self._section_label(tab, "POSITION")
            card = self._card(tab)
            self._build_position_setting(card)

            self._section_label(tab, "APPEARANCE")
            card = self._card(tab)
            self._build_opacity_setting(card)
            self._build_fontsize_setting(card)

        self._add_bottom_padding(tab)

    def _build_position_setting(self, card) -> None:
        right = self._setting_row(card, "Screen Position", "Where the overlay appears")
        pos_labels = {
            "bottom-center": "Bottom Center", "top-center": "Top Center",
            "top-left": "Top Left", "top-right": "Top Right",
            "bottom-left": "Bottom Left", "bottom-right": "Bottom Right",
        }
        current_pos = pos_labels.get(self._settings.overlay_position, "Bottom Center")
        self._pos_var = ctk.StringVar(value=current_pos)
        self._dropdown(
            right, variable=self._pos_var,
            values=list(pos_labels.values()),
            width=160, height=30,
            corner_radius=RADIUS_SM,
            font=(FONT_FAMILY, FONT_SIZES["sm"]),
            dropdown_font=(FONT_FAMILY, FONT_SIZES["sm"]),
            command=lambda v: self._on_pos_changed(v, pos_labels),
        ).pack()

    def _build_opacity_setting(self, card) -> None:
        opacity_row = ctk.CTkFrame(card, fg_color="transparent")
        opacity_row.pack(fill="x", padx=SPACING["lg"], pady=SPACING["sm"])

        ctk.CTkLabel(
            opacity_row, text="Opacity",
            font=(FONT_FAMILY, FONT_SIZES["base"]),
            text_color=self._colors.fg_primary, anchor="w",
        ).pack(anchor="w")

        slider_frame = ctk.CTkFrame(opacity_row, fg_color="transparent")
        slider_frame.pack(fill="x")

        self._opacity_val_label = ctk.CTkLabel(
            slider_frame,
            text=f"{int(self._settings.overlay_opacity * 100)}%",
            font=(FONT_FAMILY, FONT_SIZES["sm"]),
            text_color=self._colors.fg_secondary,
            width=40,
        )
        self._opacity_val_label.pack(side="right", padx=(SPACING["sm"], 0))

        ctk.CTkSlider(
            slider_frame,
            from_=0.3, to=1.0,
            number_of_steps=14,
            command=self._on_opacity_changed,
            height=16,
        ).pack(side="left", fill="x", expand=True)

    def _build_fontsize_setting(self, card) -> None:
        font_row = ctk.CTkFrame(card, fg_color="transparent")
        font_row.pack(fill="x", padx=SPACING["lg"], pady=SPACING["sm"])

        ctk.CTkLabel(
            font_row, text="Font Size",
            font=(FONT_FAMILY, FONT_SIZES["base"]),
            text_color=self._colors.fg_primary, anchor="w",
        ).pack(anchor="w")

        fs_frame = ctk.CTkFrame(font_row, fg_color="transparent")
        fs_frame.pack(fill="x")

        self._fontsize_val_label = ctk.CTkLabel(
            fs_frame,
            text=f"{self._settings.overlay_font_size}px",
            font=(FONT_FAMILY, FONT_SIZES["sm"]),
            text_color=self._colors.fg_secondary,
            width=40,
        )
        self._fontsize_val_label.pack(side="right", padx=(SPACING["sm"], 0))

        ctk.CTkSlider(
            fs_frame,
            from_=10, to=28,
            number_of_steps=18,
            command=self._on_fontsize_changed,
            height=16,
        ).pack(side="left", fill="x", expand=True)

    def _on_overlay_toggle(self) -> None:
        self._settings.overlay_enabled = self._overlay_var.get()
        self._settings.save()

    def _on_pos_changed(self, val, labels) -> None:
        reverse = {v: k for k, v in labels.items()}
        key = reverse.get(val, "bottom-center")
        self._settings.overlay_position = key
        self._settings.save()

    def _on_opacity_changed(self, val) -> None:
        self._settings.overlay_opacity = round(val, 2)
        self._settings.save()
        self._opacity_val_label.configure(text=f"{int(val * 100)}%")

    def _on_fontsize_changed(self, val) -> None:
        self._settings.overlay_font_size = int(val)
        self._settings.save()
        self._fontsize_val_label.configure(text=f"{int(val)}px")

    # ==========================================================================
    # MODELS TAB
    # ==========================================================================

    def _build_models_tab(self) -> None:
        tab = self._make_tab_frame("models")
        self._tab_header(tab, "Models", "Whisper models for speech recognition")

        cols = self._get_col_count()

        if cols >= 2:
            # Grid layout for model cards
            self._section_label(tab, "ENGLISH OPTIMIZED")
            en_models = ["distil-large-v3", "distil-medium.en", "distil-small.en"]
            grid = self._make_grid_container(tab)
            for i, key in enumerate(en_models):
                col = i % cols
                row = i // cols
                self._build_model_card_grid(grid, key, col, row)

            self._section_label(tab, "MULTILINGUAL")
            ml_models = ["large-v3-turbo", "medium", "small", "base", "tiny", "large-v3"]
            grid2 = self._make_grid_container(tab)
            for i, key in enumerate(ml_models):
                col = i % cols
                row = i // cols
                self._build_model_card_grid(grid2, key, col, row)
        else:
            # Single column
            self._section_label(tab, "ENGLISH OPTIMIZED")
            for key in ["distil-large-v3", "distil-medium.en", "distil-small.en"]:
                self._build_model_card(tab, key)

            self._section_label(tab, "MULTILINGUAL")
            for key in ["large-v3-turbo", "medium", "small", "base", "tiny", "large-v3"]:
                self._build_model_card(tab, key)

        self._add_bottom_padding(tab)

    def _build_model_card(self, parent, model_key: str) -> None:
        info = MODEL_INFO.get(model_key, {})
        is_selected = self._settings.model_size == model_key
        cached = self._is_model_cached(model_key)

        card = ctk.CTkFrame(
            parent,
            corner_radius=RADIUS_MD,
            fg_color=self._colors.bg_secondary,
            border_width=2 if is_selected else 1,
            border_color=self._colors.accent if is_selected else self._colors.border_subtle,
        )
        card.pack(fill="x", padx=SPACING["xl"], pady=(SPACING["xs"], 0))
        self._fill_model_card(card, info, model_key, is_selected, cached)

    def _build_model_card_grid(self, grid, model_key: str, col: int, row: int) -> None:
        info = MODEL_INFO.get(model_key, {})
        is_selected = self._settings.model_size == model_key
        cached = self._is_model_cached(model_key)

        card = ctk.CTkFrame(
            grid,
            corner_radius=RADIUS_MD,
            fg_color=self._colors.bg_secondary,
            border_width=2 if is_selected else 1,
            border_color=self._colors.accent if is_selected else self._colors.border_subtle,
        )
        card.grid(row=row, column=col, sticky="nsew",
                  padx=SPACING["xs"], pady=SPACING["xs"])
        self._fill_model_card(card, info, model_key, is_selected, cached)

    def _fill_model_card(self, card, info, model_key, is_selected, cached) -> None:
        inner = ctk.CTkFrame(card, fg_color="transparent")
        inner.pack(fill="x", padx=SPACING["lg"], pady=SPACING["sm"])

        top = ctk.CTkFrame(inner, fg_color="transparent")
        top.pack(fill="x")

        name_text = info.get("name", model_key)
        if is_selected:
            name_text += "  \u25CF"  # ●

        ctk.CTkLabel(
            top, text=name_text,
            font=(FONT_FAMILY, FONT_SIZES["base"], "bold"),
            text_color=self._colors.accent if is_selected else self._colors.fg_primary,
            anchor="w",
        ).pack(side="left")

        status_text = "\u2713 Ready" if cached else "\u2193 Download"
        status_color = self._colors.success if cached else self._colors.fg_muted
        ctk.CTkLabel(
            top, text=status_text,
            font=(FONT_FAMILY, FONT_SIZES["xs"]),
            text_color=status_color,
        ).pack(side="right")

        detail = f"{info.get('lang', '')} \u00b7 {info.get('size', '')} \u00b7 {info.get('device', '')}"
        ctk.CTkLabel(
            inner, text=detail,
            font=(FONT_FAMILY, FONT_SIZES["xs"]),
            text_color=self._colors.fg_secondary, anchor="w",
        ).pack(fill="x")

        rec = info.get("rec", "")
        if rec:
            ctk.CTkLabel(
                inner, text=rec,
                font=(FONT_FAMILY, FONT_SIZES["xs"]),
                text_color=self._colors.fg_muted, anchor="w",
            ).pack(fill="x")

        if not is_selected:
            ctk.CTkButton(
                inner, text="Select",
                font=(FONT_FAMILY, FONT_SIZES["xs"]),
                width=70, height=24,
                corner_radius=RADIUS_SM,
                command=lambda k=model_key: self._select_model_from_card(k),
            ).pack(anchor="e", pady=(SPACING["xs"], 0))

    def _select_model_from_card(self, key: str) -> None:
        self._settings.model_size = key
        self._settings.save()
        name = MODEL_INFO.get(key, {}).get("name", key)
        self._model_var.set(name)
        self._model_info_label.configure(text=self._get_model_info_text(key))
        # Rebuild models tab to update selection highlight
        self._tab_frames["models"].pack_forget()
        self._tab_frames.pop("models")
        self._build_models_tab()
        if self._current_tab == "models":
            self._tab_frames["models"].pack(fill="both", expand=True)
        self._apply_settings(reload_model=True)

    # ==========================================================================
    # HISTORY TAB (placeholder)
    # ==========================================================================

    def _build_history_tab(self) -> None:
        tab = self._make_tab_frame("history")
        self._tab_header(tab, "History", "Transcript history log")

        # Coming soon card
        card = self._card(tab)
        inner = ctk.CTkFrame(card, fg_color="transparent")
        inner.pack(fill="x", padx=SPACING["xl"], pady=SPACING["2xl"])

        ctk.CTkLabel(
            inner, text="\u29D6  Coming in Phase 3",
            font=(FONT_FAMILY, FONT_SIZES["lg"], "bold"),
            text_color=self._colors.fg_muted,
        ).pack()

        ctk.CTkLabel(
            inner, text="Your past dictations will be saved and searchable here.",
            font=(FONT_FAMILY, FONT_SIZES["sm"]),
            text_color=self._colors.fg_muted,
        ).pack(pady=(SPACING["xs"], 0))

        # Planned features
        self._section_label(tab, "PLANNED FEATURES")
        card = self._card(tab)
        planned = [
            ("\u2610", "Transcript Log", "Save all dictations to a local searchable file"),
            ("\u2610", "Voice Commands", '"New line", "period", "delete that"'),
            ("\u2610", "Sound Feedback", "Subtle chime on start/stop"),
            ("\u2610", "Per-App Paste Rules", "Some apps need typing instead of clipboard"),
        ]
        for icon, title, desc in planned:
            row = ctk.CTkFrame(card, fg_color="transparent")
            row.pack(fill="x", padx=SPACING["lg"], pady=SPACING["xs"])
            ctk.CTkLabel(
                row, text=f"{icon}  {title}",
                font=(FONT_FAMILY, FONT_SIZES["sm"]),
                text_color=self._colors.fg_muted, anchor="w",
            ).pack(anchor="w")
            ctk.CTkLabel(
                row, text=f"     {desc}",
                font=(FONT_FAMILY, FONT_SIZES["xs"]),
                text_color=self._colors.fg_muted, anchor="w",
            ).pack(anchor="w")

        self._add_bottom_padding(tab)

    # ==========================================================================
    # ABOUT TAB
    # ==========================================================================

    def _build_about_tab(self) -> None:
        tab = self._make_tab_frame("about")
        self._tab_header(tab, "About Eqho", "Your voice, everywhere.")

        cols = self._get_col_count()

        if cols >= 2:
            grid = self._make_grid_container(tab)

            col0 = ctk.CTkFrame(grid, fg_color="transparent")
            col0.grid(row=0, column=0, sticky="nsew", padx=SPACING["xs"])

            self._build_about_details(col0)

            col1 = ctk.CTkFrame(grid, fg_color="transparent")
            col1.grid(row=0, column=1, sticky="nsew", padx=SPACING["xs"])

            self._build_about_credits(col1)
        else:
            self._build_about_details(tab)
            self._build_about_credits(tab)

        self._add_bottom_padding(tab)

    def _build_about_details(self, parent) -> None:
        """About tab: version info and author."""
        self._section_label(parent, "DETAILS")
        card = self._card(parent)
        inner = ctk.CTkFrame(card, fg_color="transparent")
        inner.pack(fill="x", padx=SPACING["lg"], pady=SPACING["lg"])

        details = [
            ("Version", __version__),
            ("Engine", "faster-whisper (CTranslate2)"),
            ("Default Model", "Distil Large v3"),
            ("GPU", "CUDA (NVIDIA) with CPU fallback"),
            ("Platform", "Windows 10/11"),
            ("Font", "Inter (SIL Open Font License)"),
        ]
        for label, value in details:
            row = ctk.CTkFrame(inner, fg_color="transparent")
            row.pack(fill="x", pady=2)
            ctk.CTkLabel(
                row, text=label,
                font=(FONT_FAMILY, FONT_SIZES["sm"]),
                text_color=self._colors.fg_secondary, anchor="w",
                width=120,
            ).pack(side="left")
            ctk.CTkLabel(
                row, text=value,
                font=(FONT_FAMILY, FONT_SIZES["sm"]),
                text_color=self._colors.fg_primary, anchor="w",
            ).pack(side="left")

        # Author row with clickable GitHub link
        row = ctk.CTkFrame(inner, fg_color="transparent")
        row.pack(fill="x", pady=2)
        ctk.CTkLabel(
            row, text="Author",
            font=(FONT_FAMILY, FONT_SIZES["sm"]),
            text_color=self._colors.fg_secondary, anchor="w",
            width=120,
        ).pack(side="left")
        author_link = ctk.CTkLabel(
            row, text="Daniel Mevit",
            font=(FONT_FAMILY, FONT_SIZES["sm"]),
            text_color=self._colors.accent, anchor="w",
            cursor="hand2",
        )
        author_link.pack(side="left")
        author_link.bind("<Button-1>", lambda e: self._open_url("https://github.com/DanielMevit"))

    def _build_about_credits(self, parent) -> None:
        """About tab: powered by section."""
        self._section_label(parent, "POWERED BY")
        card = self._card(parent)
        inner = ctk.CTkFrame(card, fg_color="transparent")
        inner.pack(fill="x", padx=SPACING["lg"], pady=SPACING["sm"])

        techs = [
            "faster-whisper \u2014 Fast Whisper inference (MIT)",
            "customtkinter \u2014 Modern tkinter widgets",
            "pystray \u2014 System tray integration",
            "keyboard \u2014 Global hotkey capture",
            "pycaw \u2014 Windows audio control",
        ]
        for t in techs:
            ctk.CTkLabel(
                inner, text=f"\u00b7  {t}",
                font=(FONT_FAMILY, FONT_SIZES["xs"]),
                text_color=self._colors.fg_secondary, anchor="w",
            ).pack(anchor="w", pady=1)

    # -- Utilities -------------------------------------------------------------

    @staticmethod
    def _open_url(url: str) -> None:
        import webbrowser
        webbrowser.open(url)

    # -- Lifecycle -------------------------------------------------------------

    def _on_close(self) -> None:
        global _active_dashboard
        _active_dashboard = None
        if self._hotkey_capturing:
            self._stop_hotkey_capture(cancelled=True)
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
