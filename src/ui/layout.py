"""Shared layout builders for dashboard tabs.

TabBase carries the DashboardContext and provides the card/row/label/dropdown
builders that every tab uses. Colors/settings resolve through the context so a
theme switch is picked up on rebuild.

Spacing law (applies to every tab, both 1-col and grid layouts):
- Card/section/header horizontal inset from the tab edge = SPACING["md"].
  Grid containers and column frames add NO extra horizontal padding, so
  content aligns identically at every breakpoint.
- Row padding inside a card = SPACING["md"]; gap between sections = SPACING["lg"].
"""

import customtkinter as ctk

from ..theme import (
    MODEL_INFO, SPACING, RADIUS_SM, RADIUS_LG, font,
)
from .context import DashboardContext
from .widgets import ThemedDropdown



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

    # tk Variables MUST bind to the dashboard's own root. Without an explicit
    # master they bind to tkinter's default root — which can be a window on
    # ANOTHER THREAD, making every .set() a cross-thread Tcl call that can
    # deadlock (the model-switch freeze).
    def _string_var(self, value: str = "") -> ctk.StringVar:
        return ctk.StringVar(master=self.ctx.master(), value=value)

    def _bool_var(self, value: bool = False) -> ctk.BooleanVar:
        return ctk.BooleanVar(master=self.ctx.master(), value=value)

    # -- Shared builders ----------------------------------------------------------

    def _add_bottom_padding(self, tab) -> None:
        """Add breathing room at the bottom of a tab."""
        ctk.CTkFrame(tab, fg_color="transparent", height=SPACING["2xl"]).pack(fill="x")

    def _format_hotkey(self, combo: str) -> str:
        return " + ".join(p.strip().title() for p in combo.split("+"))

    def _header_status_text(self) -> str:
        from ..settings import SUPPORTED_LANGUAGES
        model_name = MODEL_INFO.get(self._settings.model_size, {}).get("name", self._settings.model_size)
        lang = SUPPORTED_LANGUAGES.get(self._settings.language, self._settings.language)
        hotkey = self._format_hotkey(self._settings.hotkey)
        return f"{model_name}  ·  {hotkey}  ·  {lang}  ·  {self._settings.hotkey_mode}"

    def refresh_header_status(self) -> None:
        """Update the header's model·hotkey·language line in place (settings
        changes no longer rebuild tabs, so headers must refresh themselves)."""
        try:
            self._header_status.configure(text=self._header_status_text())
        except Exception:
            pass

    def _tab_header(self, parent, title: str, subtitle: str, icon: str | None = None) -> None:
        """Standard tab header with title and status info; `icon` is a Phosphor
        glyph name rendered in the accent color left of the title."""
        header = ctk.CTkFrame(parent, fg_color="transparent")
        header.pack(fill="x", padx=SPACING["md"], pady=(SPACING["md"], 2))

        title_row = ctk.CTkFrame(header, fg_color="transparent")
        title_row.pack(anchor="w")
        if icon:
            from .icons import icon as _glyph, icon_font
            ctk.CTkLabel(
                title_row, text=_glyph(icon),
                font=icon_font("2xl"),
                text_color=self._colors.accent, fg_color="transparent",
            ).pack(side="left", padx=(0, 8))
        ctk.CTkLabel(
            title_row, text=title,
            font=font("2xl", "bold"),
            text_color=self._colors.fg_primary, anchor="w",
        ).pack(side="left")

        self._header_status = ctk.CTkLabel(
            header, text=self._header_status_text(),
            font=font("xs"),
            text_color=self._colors.fg_muted, anchor="w",
        )
        self._header_status.pack(anchor="w", pady=(1, 0))

        ctk.CTkLabel(
            parent, text=subtitle,
            font=font("sm"),
            text_color=self._colors.fg_secondary, anchor="w",
        ).pack(fill="x", padx=SPACING["md"], pady=(0, SPACING["sm"]))

    def _section_label(self, parent, text: str) -> ctk.CTkLabel:
        lbl = ctk.CTkLabel(
            parent, text=text,
            font=font("sm", "bold"),
            text_color=self._colors.fg_secondary,
            anchor="w",
        )
        lbl.pack(fill="x", padx=SPACING["md"], pady=(SPACING["md"], 3))
        return lbl

    def _dropdown_colors(self) -> dict:
        """Common styling kwargs for themed dropdowns."""
        return {
            "button_color": self._colors.bg_tertiary,
            "button_hover_color": self._colors.bg_hover,
            "border_color": self._colors.border,
            "dropdown_fg_color": self._colors.bg_secondary,
            "dropdown_hover_color": self._colors.bg_hover,
            "dropdown_text_color": self._colors.fg_primary,
            "fg_color": self._colors.bg_tertiary,
            "text_color": self._colors.fg_primary,
        }

    def _dropdown(self, parent, **kwargs) -> ThemedDropdown:
        """Create a themed dropdown with hairline border and full theme control."""
        merged = {**self._dropdown_colors(), **kwargs}
        return ThemedDropdown(parent, **merged)

    def _card(self, parent) -> ctk.CTkFrame:
        card = ctk.CTkFrame(
            parent,
            corner_radius=RADIUS_LG,
            fg_color=self._colors.bg_secondary,
            border_width=1,
            border_color=self._colors.border_subtle,
        )
        card.pack(fill="x", padx=SPACING["md"], pady=(SPACING["xs"], 0))
        return card

    def _card_grid(self, parent, col: int, row: int) -> ctk.CTkFrame:
        """A card placed into a grid layout."""
        card = ctk.CTkFrame(
            parent,
            corner_radius=RADIUS_LG,
            fg_color=self._colors.bg_secondary,
            border_width=1,
            border_color=self._colors.border_subtle,
        )
        card.grid(row=row, column=col, sticky="nsew",
                  padx=SPACING["md"], pady=SPACING["xs"])
        return card

    def _setting_row(self, parent, label: str, description: str = "") -> ctk.CTkFrame:
        """A stacked block inside a card: label + description with the control
        BELOW them, left-aligned. (The old side-by-side layout crushed
        controls into labels at narrow widths.)"""
        row = ctk.CTkFrame(parent, fg_color="transparent")
        row.pack(fill="x", padx=SPACING["md"], pady=SPACING["sm"])

        ctk.CTkLabel(
            row, text=label,
            font=font("base"),
            text_color=self._colors.fg_primary,
            anchor="w",
        ).pack(anchor="w", fill="x")

        if description:
            ctk.CTkLabel(
                row, text=description,
                font=font("xs"),
                text_color=self._colors.fg_muted,
                anchor="w",
            ).pack(anchor="w", fill="x")

        control = ctk.CTkFrame(row, fg_color="transparent")
        control.pack(anchor="w", pady=(3, 0))
        return control

    def _make_grid_container(self, parent) -> ctk.CTkFrame:
        """Grid container distributing columns evenly. Adds no horizontal
        padding of its own — cards carry the md inset (see spacing law)."""
        grid = ctk.CTkFrame(parent, fg_color="transparent")
        grid.pack(fill="both", expand=True, padx=0, pady=SPACING["xs"])
        cols = self._get_col_count()
        for c in range(cols):
            grid.columnconfigure(c, weight=1, uniform="col")
        return grid
