"""Models tab: Whisper model cards with selection, download, and progress."""

import customtkinter as ctk

from ...modelstore import is_downloading, start_download, status as download_status
from ...theme import MODEL_INFO, SPACING, RADIUS_LG, font
from ..layout import TabBase
from ..widgets import secondary_button

_EN_MODELS = ["distil-large-v3", "distil-medium.en", "distil-small.en"]
_ML_MODELS = ["large-v3-turbo", "medium", "small", "base", "tiny", "large-v3"]


class ModelsTab(TabBase):
    KEY = "models"

    def __init__(self, ctx):
        super().__init__(ctx)
        self._progress_widgets: dict = {}
        self._poll_widget = None

    def build(self, tab) -> None:
        self._tab_header(tab, "Models", "Whisper models for speech recognition")
        self._progress_widgets = {}
        self._poll_widget = tab

        cols = self._get_col_count()

        if cols >= 2:
            self._section_label(tab, "ENGLISH OPTIMIZED")
            grid = self._make_grid_container(tab)
            for i, key in enumerate(_EN_MODELS):
                self._build_model_card_grid(grid, key, i % cols, i // cols)

            self._section_label(tab, "MULTILINGUAL")
            grid2 = self._make_grid_container(tab)
            for i, key in enumerate(_ML_MODELS):
                self._build_model_card_grid(grid2, key, i % cols, i // cols)
        else:
            self._section_label(tab, "ENGLISH OPTIMIZED")
            for key in _EN_MODELS:
                self._build_model_card(tab, key)

            self._section_label(tab, "MULTILINGUAL")
            for key in _ML_MODELS:
                self._build_model_card(tab, key)

        self._add_bottom_padding(tab)

        if self._progress_widgets:
            self._schedule_poll()

    def _card_frame(self, is_selected: bool, parent) -> ctk.CTkFrame:
        return ctk.CTkFrame(
            parent,
            corner_radius=RADIUS_LG,
            fg_color=self._colors.bg_secondary,
            border_width=2 if is_selected else 1,
            border_color=self._colors.accent if is_selected else self._colors.border_subtle,
        )

    def _build_model_card(self, parent, model_key: str) -> None:
        is_selected = self._settings.model_size == model_key
        card = self._card_frame(is_selected, parent)
        card.pack(fill="x", padx=SPACING["md"], pady=(SPACING["xs"], 0))
        self._fill_model_card(card, model_key, is_selected)

    def _build_model_card_grid(self, grid, model_key: str, col: int, row: int) -> None:
        is_selected = self._settings.model_size == model_key
        card = self._card_frame(is_selected, grid)
        card.grid(row=row, column=col, sticky="nsew",
                  padx=SPACING["md"], pady=SPACING["xs"])
        self._fill_model_card(card, model_key, is_selected)

    def _fill_model_card(self, card, model_key: str, is_selected: bool) -> None:
        info = MODEL_INFO.get(model_key, {})
        cached = self._is_model_cached(model_key)
        downloading = is_downloading(model_key)

        inner = ctk.CTkFrame(card, fg_color="transparent")
        inner.pack(fill="x", padx=SPACING["md"], pady=SPACING["sm"])

        top = ctk.CTkFrame(inner, fg_color="transparent")
        top.pack(fill="x")

        name_text = info.get("name", model_key)
        if is_selected:
            name_text += "  ●"

        ctk.CTkLabel(
            top, text=name_text,
            font=font("base", "bold"),
            text_color=self._colors.accent if is_selected else self._colors.fg_primary,
            anchor="w",
        ).pack(side="left")

        if cached:
            status_text, status_color = "✓ Ready", self._colors.success
        elif downloading:
            status_text, status_color = "Downloading…", self._colors.accent
        else:
            status_text, status_color = "Not downloaded", self._colors.fg_muted
        ctk.CTkLabel(
            top, text=status_text,
            font=font("xs"),
            text_color=status_color,
        ).pack(side="right")

        detail = f"{info.get('lang', '')} · {info.get('size', '')} · {info.get('device', '')}"
        ctk.CTkLabel(
            inner, text=detail,
            font=font("xs"),
            text_color=self._colors.fg_secondary, anchor="w",
        ).pack(fill="x")

        rec = info.get("rec", "")
        if rec:
            ctk.CTkLabel(
                inner, text=rec,
                font=font("xs"),
                text_color=self._colors.fg_muted, anchor="w",
            ).pack(fill="x")

        if not is_selected:
            actions = ctk.CTkFrame(inner, fg_color="transparent")
            actions.pack(fill="x", pady=(SPACING["xs"], 0))

            select_btn = secondary_button(
                actions, self._colors, text="Select",
                font=font("xs"),
                width=70, height=24,
                command=lambda k=model_key: self._select_model_from_card(k),
            )
            if not cached:
                # Selecting an absent model would silently trigger a long
                # download — download explicitly first, then select.
                select_btn.configure(state="disabled",
                                     text_color_disabled=self._colors.fg_muted)
            select_btn.pack(side="right")

            if not cached and not downloading:
                secondary_button(
                    actions, self._colors, text="↓",
                    font=font("sm", "bold"),
                    width=28, height=24,
                    command=lambda k=model_key: self._start_download(k),
                ).pack(side="right", padx=(0, SPACING["xs"]))

        if downloading:
            self._attach_progress(inner, model_key)

    def _attach_progress(self, inner, model_key: str) -> None:
        """Thin progress bar at the card bottom, filling left → right."""
        state = download_status(model_key)
        row = ctk.CTkFrame(inner, fg_color="transparent")
        row.pack(fill="x", pady=(SPACING["xs"], 0))

        percent = ctk.CTkLabel(
            row, text=f"{int(state.get('progress', 0.0) * 100)}%",
            font=font("xs"), text_color=self._colors.fg_secondary,
            width=36, anchor="e",
        )
        percent.pack(side="right")

        bar = ctk.CTkProgressBar(
            row, height=5, corner_radius=2,
            fg_color=self._colors.bg_tertiary,
            progress_color=self._colors.accent,
        )
        bar.set(state.get("progress", 0.0))
        bar.pack(side="left", fill="x", expand=True, padx=(0, SPACING["xs"]))

        self._progress_widgets[model_key] = (bar, percent)

    # -- Download polling -----------------------------------------------------

    def _start_download(self, model_key: str) -> None:
        start_download(self._settings, model_key)
        self.ctx.rebuild_tab(self.KEY)

    def _schedule_poll(self) -> None:
        try:
            self._poll_widget.after(500, self._poll_downloads)
        except Exception:
            pass

    def _poll_downloads(self) -> None:
        any_active = False
        any_finished = False
        for key, (bar, percent) in list(self._progress_widgets.items()):
            state = download_status(key)
            if not state:
                continue
            if state.get("done"):
                any_finished = True
                continue
            any_active = True
            fraction = state.get("progress", 0.0)
            try:
                bar.set(fraction)
                percent.configure(text=f"{int(fraction * 100)}%")
            except Exception:
                return  # widgets destroyed (tab rebuilt) — that build's poller dies
        if any_finished:
            self.ctx.rebuild_tab(self.KEY)  # Select enables, status flips to Ready
            return
        if any_active:
            self._schedule_poll()

    def _select_model_from_card(self, key: str) -> None:
        self._settings.model_size = key
        self._settings.save()
        # Let other tabs (General) sync their own widgets.
        self.ctx.emit("model_changed", key)
        # Rebuild this tab to update the selection highlight.
        self.ctx.rebuild_tab(self.KEY)
        self._apply_settings(reload_model=True)
