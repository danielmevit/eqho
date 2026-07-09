"""Shared layout builders for dashboard tabs.

TabBase carries the DashboardContext and provides the card/row/label/dropdown
builders that every tab uses. Method bodies are moved verbatim from the old
monolithic dashboard.py — colors/settings resolve through the context so a
theme switch is picked up on rebuild exactly as before.
"""

import customtkinter as ctk

from ..fonts import FONT_FAMILY
from ..theme import (
    MODEL_INFO, FONT_SIZES, SPACING, RADIUS_SM, RADIUS_MD,
)
from .context import DashboardContext
from .widgets import ThemedDropdown

SECTION_ICONS = {
    "AUDIO INPUT":      "▶",   # ▶
    "HOTKEY":           "▶",   # ▶
    "MODEL":            "▶",   # ▶
    "BEHAVIOR":         "▶",   # ▶
    "VISIBILITY":       "▶",   # ▶
    "POSITION":         "▶",   # ▶
    "APPEARANCE":       "▶",   # ▶
    "ENGLISH OPTIMIZED":"▶",   # ▶
    "MULTILINGUAL":     "▶",   # ▶
    "PLANNED FEATURES": "▶",   # ▶
    "POWERED BY":       "▶",   # ▶
}


class TabBase:
    """Base class for dashboard tabs: context access + shared builders."""

    KEY = ""

    def __init__(self, ctx: DashboardContext):
        self.ctx = ctx

    def build(self, tab: ctk.CTkScrollableFrame) -> None:
        raise NotImplementedError

    # -- Context accessors (keep moved code reading naturally) -------------------

    @property
    def _settings(self):
        return self.ctx.settings

    @property
    def _colors(self):
        return self.ctx.colors

    def _apply_settings(self, reload_model: bool = False) -> None:
        self.ctx.apply_settings(reload_model)

    def _get_col_count(self) -> int:
        return self.ctx.get_col_count()

    def _is_model_cached(self, model_key: str) -> bool:
        return self.ctx.is_model_cached(model_key)

    # -- Shared builders (moved verbatim from dashboard.py) ----------------------

    def _add_bottom_padding(self, tab) -> None:
        """Add breathing room at the bottom of a tab."""
        ctk.CTkFrame(tab, fg_color="transparent", height=SPACING["2xl"]).pack(fill="x")

    def _format_hotkey(self, combo: str) -> str:
        return " + ".join(p.strip().title() for p in combo.split("+"))

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
        from ..settings import SUPPORTED_LANGUAGES
        model_name = MODEL_INFO.get(self._settings.model_size, {}).get("name", self._settings.model_size)
        lang = SUPPORTED_LANGUAGES.get(self._settings.language, self._settings.language)
        hotkey = self._format_hotkey(self._settings.hotkey)
        status = f"{model_name}  ·  {hotkey}  ·  {lang}  ·  {self._settings.hotkey_mode}"

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
        icon = SECTION_ICONS.get(text, "▶")
        lbl = ctk.CTkLabel(
            parent, text=f"{icon}  {text}",
            font=(FONT_FAMILY, FONT_SIZES["sm"], "bold"),
            text_color=self._colors.fg_secondary,
            anchor="w",
        )
        lbl.pack(fill="x", padx=SPACING["xl"], pady=(SPACING["lg"], 4))
        return lbl

    def _dropdown_colors(self) -> dict:
        """Common styling kwargs for themed dropdowns."""
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
