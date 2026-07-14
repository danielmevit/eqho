"""Settings view (gear icon): appearance + everything that isn't one of the
three main sections — the Overlay controls and the About panel fold in here
as sections (ref/ui structure: General/Models/History up top, the rest behind
the gear). No repeated titles: sections carry the labels, pages don't."""

import customtkinter as ctk

from ...theme import SPACING, font
from ..layout import TabBase
from ..icons import icon, icon_font
from ..widgets import segmented
from .about import AboutTab
from .overlay import OverlayTab


class SettingsTab(TabBase):
    KEY = "settings"

    def build(self, tab) -> None:
        self._tab_status_line(tab)

        # -- Appearance: Theme | UI Zoom side by side ----------------------------
        self._icon_section_label(tab, "theme", "APPEARANCE")
        if self._get_col_count() >= 2:
            grid = ctk.CTkFrame(tab, fg_color="transparent")
            grid.pack(fill="x", pady=SPACING["xs"])
            grid.columnconfigure(0, weight=1, uniform="appearance")
            grid.columnconfigure(1, weight=1, uniform="appearance")
            col0 = ctk.CTkFrame(grid, fg_color="transparent")
            col0.grid(row=0, column=0, sticky="nsew")
            col1 = ctk.CTkFrame(grid, fg_color="transparent")
            col1.grid(row=0, column=1, sticky="nsew")
            self._build_theme_card(col0)
            self._build_zoom_card(col1)
        else:
            self._build_theme_card(tab)
            self._build_zoom_card(tab)

        # -- Overlay (moved from its old tab; options only, no inner titles) ------
        self._icon_section_label(tab, "overlay", "OVERLAY")
        OverlayTab(self.ctx).build(tab, embedded=True)

        # -- About (moved from its old tab) ----------------------------------------
        self._icon_section_label(tab, "about", "ABOUT EQHO")
        AboutTab(self.ctx).build(tab, embedded=True)

        self._add_bottom_padding(tab)

    def _build_theme_card(self, parent) -> None:
        card = self._card(parent)
        right = self._setting_row(card, "Theme", "Light, dark, or follow Windows")
        labels = {"light": "Light", "dark": "Dark", "system": "System"}
        self._theme_var = self._string_var(
            value=labels.get(self._settings.theme, "System"),
        )
        segmented(
            right, self._colors,
            values=list(labels.values()),
            variable=self._theme_var,
            command=self._on_theme_changed,
        ).pack()

    def _build_zoom_card(self, parent) -> None:
        card = self._card(parent)
        control = self._setting_row(card, "UI Zoom", "Scale the whole dashboard")
        levels = ["100%", "125%", "150%", "175%", "200%"]
        current = f"{int(round(self._settings.ui_scale * 100))}%"
        self._zoom_var = self._string_var(value=current if current in levels else "150%")
        segmented(
            control, self._colors,
            values=levels,
            variable=self._zoom_var,
            command=self._on_zoom_changed,
        ).pack(anchor="w")

    def _icon_section_label(self, parent, icon_name: str, text: str) -> None:
        """Section divider with an accent Phosphor glyph — the warmth accents."""
        row = ctk.CTkFrame(parent, fg_color="transparent")
        row.pack(fill="x", padx=SPACING["md"], pady=(SPACING["lg"], 3))
        ctk.CTkLabel(
            row, text=icon(icon_name),
            font=icon_font("sm", 3),
            text_color=self._colors.accent, fg_color="transparent",
        ).pack(side="left", padx=(0, 6))
        ctk.CTkLabel(
            row, text=text,
            font=font("sm", "bold"),
            text_color=self._colors.fg_secondary, anchor="w",
        ).pack(side="left")

    def _on_theme_changed(self, val: str) -> None:
        mode = {"Light": "light", "Dark": "dark", "System": "system"}.get(val, "system")
        if mode != self._settings.theme:
            self.ctx.set_theme(mode)

    def _on_zoom_changed(self, value: str) -> None:
        scale = int(value.rstrip("%")) / 100.0
        if abs(scale - self._settings.ui_scale) > 0.01:
            self.ctx.set_ui_scale(scale)
