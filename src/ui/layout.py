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



def status_summary(settings) -> str:
    """The model · hotkey · language · mode one-liner (top bar, right side).
    Compact on purpose — it shares the bar with the centered pill nav."""
    model_name = MODEL_INFO.get(settings.model_size, {}).get("name", settings.model_size)
    hotkey = "+".join(p.strip().title() for p in settings.hotkey.split("+"))
    lang = settings.language.upper()
    return f"{model_name} · {hotkey} · {lang} · {settings.hotkey_mode}"


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
        return status_summary(self._settings)

    def refresh_header_status(self) -> None:
        """The model·hotkey·language line lives in the dashboard's TOP BAR now
        — tabs keep calling this after settings changes and it routes there."""
        try:
            self.ctx.refresh_status()
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

    def _columns(self, tab, count: int | None = None) -> list:
        """THE shared responsive grid: every tab that lays out columns goes
        through here, so column widths, gutters and label offsets align
        pixel-identically across all sections."""
        cols = count if count is not None else self._get_col_count()
        grid = ctk.CTkFrame(tab, fg_color="transparent")
        grid.pack(fill="both", expand=True, padx=0, pady=SPACING["xs"])
        frames = []
        for c in range(cols):
            grid.columnconfigure(c, weight=1, uniform="col")
            frame = ctk.CTkFrame(grid, fg_color="transparent")
            frame.grid(row=0, column=c, sticky="nsew", padx=0)
            frames.append(frame)
        return frames

    def _build_switch_row(self, card, label: str, desc: str, value: bool, on_change):
        """Label + description + themed switch — shared by General/Settings."""
        from .widgets import themed_switch
        right = self._setting_row(card, label, desc)
        var = self._bool_var(value=value)
        themed_switch(
            right, self._colors, variable=var,
            command=lambda: on_change(var.get()),
        ).pack()
        return var
