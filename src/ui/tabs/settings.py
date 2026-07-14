"""Settings view (gear icon): appearance + everything that isn't one of the
three main sections — the Overlay controls and the About panel fold in here
as sections (ref/ui structure: General/Models/History up top, the rest behind
the gear)."""

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
        self._tab_header(tab, "Settings", "Appearance, overlay, and app info", icon="settings")

        # -- Appearance ---------------------------------------------------------
        self._icon_section_label(tab, "theme", "APPEARANCE")
        card = self._card(tab)
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

        # -- Overlay (moved from its old tab) ------------------------------------
        self._icon_section_label(tab, "overlay", "OVERLAY")
        OverlayTab(self.ctx).build(tab, embedded=True)

        # -- About (moved from its old tab) ---------------------------------------
        self._icon_section_label(tab, "about", "ABOUT EQHO")
        AboutTab(self.ctx).build(tab, embedded=True)

        self._add_bottom_padding(tab)

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
