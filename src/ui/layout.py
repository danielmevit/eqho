"""Shared layout builders for dashboard tabs.

TabBase carries the DashboardContext and provides the card/row/label/dropdown
builders that every tab uses. Colors/settings resolve through the context so a
theme switch is picked up on rebuild.

Spacing law (applies to every tab, both 1-col and grid layouts):
- Card/section/header horizontal inset from the tab edge = SPACING["md"].
  Grid containers and column frames add NO extra horizontal padding, so
  content aligns identically at every breakpoint.
- Between columns the gap is column_gap(cols) (10 px at 3, 8 px at 2): a
  column frame records its (left, right) inset and every card / section
  label packed into it uses _hpad(parent), so labels stay on the card edges.
- Row padding inside a card = SPACING["md"]; gap between sections = SPACING["lg"].
"""

import customtkinter as ctk

from ..theme import (
    MODEL_INFO, SPACING, RADIUS_SM, RADIUS_LG, font,
)
from .context import DashboardContext


def column_gap(cols: int) -> int:
    """Gap between dashboard columns on the 2/4/6/8/10 scale: tighter with
    fewer columns, none for a single column."""
    return {1: 0, 2: 4}.get(cols, 6)  # owner 2026-10-17: clearly smaller than the 12 px outer margin


def column_inset(col: int, cols: int) -> tuple[int, int]:
    """(left, right) padding of column `col` of `cols`: the outer edges keep
    the md margin, inner edges take half the column gap each."""
    half = column_gap(cols) // 2
    left = SPACING["md"] if col == 0 else half
    right = SPACING["md"] if col == cols - 1 else half
    return left, right
from .widgets import ThemedDropdown, auto_wrap_label



def status_summary(settings) -> str:
    """The model · hotkey · language · mode one-liner (top bar, right side).
    Compact on purpose — it shares the bar with the centered pill nav."""
    model_name = MODEL_INFO.get(settings.model_size, {}).get("name", settings.model_size)
    hotkey = "+".join(p.strip().title() for p in settings.hotkey.split("+"))
    lang = settings.language.upper()
    return f"{model_name} · {hotkey} · {lang} · {settings.hotkey_mode}"


def fit_window(want_w: int, want_h: int, min_w: int, min_h: int,
               scale: float, area: tuple, chrome_w: int = 0,
               chrome_h: int = 0) -> tuple:
    """Starting size and position that fit the work area (pure maths).

    want/min sizes are logical px; `scale` is the total window scaling
    (Windows scaling x Eqho zoom); `area` = (left, top, right, bottom) of the
    work area and chrome_w/chrome_h = title bar + borders, all physical px.
    Returns (w, h, x, y): logical client size (shrunk to fit, never below
    the minimum) and the physical top-left of the outer frame, centred in
    the work area (clamped to its top-left when even the minimum is bigger).
    """
    left, top, right, bottom = area
    area_w, area_h = right - left, bottom - top
    w = max(min_w, min(want_w, int((area_w - chrome_w) / scale)))
    h = max(min_h, min(want_h, int((area_h - chrome_h) / scale)))
    outer_w = round(w * scale) + chrome_w
    outer_h = round(h * scale) + chrome_h
    x = left + max(0, (area_w - outer_w) // 2)
    y = top + max(0, (area_h - outer_h) // 2)
    return w, h, x, y


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
        lbl.pack(fill="x", padx=self._hpad(parent), pady=(SPACING["md"], 3))
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

    @staticmethod
    def _hpad(parent):
        """Horizontal inset for a card/label packed into `parent`: the
        column's inset inside a column frame, else md on both sides."""
        return getattr(parent, "_eqho_hpad", SPACING["md"])

    def _column_frame(self, grid, col: int, cols: int) -> ctk.CTkFrame:
        """A transparent column frame in `grid` that carries its inset."""
        frame = ctk.CTkFrame(grid, fg_color="transparent")
        frame.grid(row=0, column=col, sticky="nsew", padx=0)
        frame._eqho_hpad = column_inset(col, cols)
        return frame

    def _card(self, parent) -> ctk.CTkFrame:
        card = ctk.CTkFrame(
            parent,
            corner_radius=RADIUS_LG,
            fg_color=self._colors.bg_secondary,
            border_width=1,
            border_color=self._colors.border_subtle,
        )
        card.pack(fill="x", padx=self._hpad(parent), pady=(SPACING["xs"], 0))
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
                  padx=column_inset(col, self._get_col_count()), pady=SPACING["xs"])
        return card

    def _setting_row(self, parent, label: str, description: str = "") -> ctk.CTkFrame:
        """A stacked block inside a card: label + description with the control
        BELOW them, left-aligned. (The old side-by-side layout crushed
        controls into labels at narrow widths.)"""
        row = ctk.CTkFrame(parent, fg_color="transparent")
        row.pack(fill="x", padx=SPACING["md"], pady=SPACING["sm"])

        title = ctk.CTkLabel(
            row, text=label,
            font=font("base"),
            text_color=self._colors.fg_primary,
            anchor="w",
        )
        title.pack(anchor="w", fill="x")
        auto_wrap_label(title, row, padding=4)

        if description:
            desc = ctk.CTkLabel(
                row, text=description,
                font=font("xs"),
                text_color=self._colors.fg_muted,
                anchor="w",
            )
            desc.pack(anchor="w", fill="x")
            auto_wrap_label(desc, row, padding=4)

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
            frames.append(self._column_frame(grid, c, cols))
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
