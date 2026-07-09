"""Models tab: Whisper model cards with selection."""

import customtkinter as ctk

from ...fonts import FONT_FAMILY
from ...theme import MODEL_INFO, FONT_SIZES, SPACING, RADIUS_SM, RADIUS_MD
from ..layout import TabBase


class ModelsTab(TabBase):
    KEY = "models"

    def build(self, tab) -> None:
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
            name_text += "  ●"

        ctk.CTkLabel(
            top, text=name_text,
            font=(FONT_FAMILY, FONT_SIZES["base"], "bold"),
            text_color=self._colors.accent if is_selected else self._colors.fg_primary,
            anchor="w",
        ).pack(side="left")

        status_text = "✓ Ready" if cached else "↓ Download"
        status_color = self._colors.success if cached else self._colors.fg_muted
        ctk.CTkLabel(
            top, text=status_text,
            font=(FONT_FAMILY, FONT_SIZES["xs"]),
            text_color=status_color,
        ).pack(side="right")

        detail = f"{info.get('lang', '')} · {info.get('size', '')} · {info.get('device', '')}"
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
        # Let other tabs (General) sync their own widgets.
        self.ctx.emit("model_changed", key)
        # Rebuild this tab to update the selection highlight.
        self.ctx.rebuild_tab(self.KEY)
        self._apply_settings(reload_model=True)
