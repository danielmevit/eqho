"""Models tab: Whisper model cards with selection, download, and progress.

All state changes (select, download start/finish/fail) update the existing
card widgets IN PLACE — never a tab rebuild, which flashed the whole view.
"""

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
        self._card_refs: dict = {}
        self._progress_widgets: dict = {}
        self._poll_widget = None
        self._selected_key = ""

    def build(self, tab) -> None:
        self._card_refs = {}
        self._progress_widgets = {}
        self._poll_widget = tab
        self._selected_key = self._settings.model_size

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

    # -- Card construction -----------------------------------------------------

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

        name_label = ctk.CTkLabel(
            top, text=info.get("name", model_key) + ("  ●" if is_selected else ""),
            font=font("base", "bold"),
            text_color=self._colors.accent if is_selected else self._colors.fg_primary,
            anchor="w",
        )
        name_label.pack(side="left")

        if cached:
            status_text, status_color = "✓ Ready", self._colors.success
        elif downloading:
            status_text, status_color = "Downloading…", self._colors.accent
        else:
            status_text, status_color = "Not downloaded", self._colors.fg_muted
        status_label = ctk.CTkLabel(
            top, text=status_text, font=font("xs"), text_color=status_color,
        )
        status_label.pack(side="right")

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

        # Actions row is ALWAYS built (hidden while selected) so selection
        # changes can toggle it in place without rebuilding the card.
        actions = ctk.CTkFrame(inner, fg_color="transparent")

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

        download_btn = None
        if not cached and not downloading:
            download_btn = self._make_download_button(actions, model_key)

        if not is_selected:
            actions.pack(fill="x", pady=(SPACING["xs"], 0))

        self._card_refs[model_key] = {
            "card": card, "inner": inner, "name": name_label,
            "status": status_label, "actions": actions,
            "select": select_btn, "download": download_btn,
            "base_name": info.get("name", model_key),
        }

        if downloading:
            self._attach_progress(model_key)

    def _make_download_button(self, actions, model_key: str) -> ctk.CTkButton:
        btn = secondary_button(
            actions, self._colors, text="↓",
            font=font("sm", "bold"),
            width=28, height=24,
            command=lambda k=model_key: self._start_download(k),
        )
        btn.pack(side="right", padx=(0, SPACING["xs"]))
        return btn

    def _attach_progress(self, model_key: str) -> None:
        """Thin progress bar at the card bottom, filling left → right."""
        refs = self._card_refs.get(model_key)
        if not refs:
            return
        state = download_status(model_key)
        row = ctk.CTkFrame(refs["inner"], fg_color="transparent")
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

        self._progress_widgets[model_key] = (row, bar, percent)

    # -- Download lifecycle (all in-place, no rebuilds) --------------------------

    def _start_download(self, model_key: str) -> None:
        refs = self._card_refs.get(model_key)
        if not refs:
            return
        start_download(self._settings, model_key)
        if refs["download"] is not None:
            refs["download"].destroy()
            refs["download"] = None
        refs["status"].configure(text="Downloading…", text_color=self._colors.accent)
        self._attach_progress(model_key)
        self._schedule_poll()

    def _schedule_poll(self) -> None:
        try:
            self._poll_widget.after(500, self._poll_downloads)
        except Exception:
            pass

    def _poll_downloads(self) -> None:
        any_active = False
        for key in list(self._progress_widgets.keys()):
            row, bar, percent = self._progress_widgets[key]
            state = download_status(key)
            if not state:
                continue
            try:
                if state.get("done"):
                    self._finish_download(key, state)
                    continue
                any_active = True
                fraction = state.get("progress", 0.0)
                bar.set(fraction)
                percent.configure(text=f"{int(fraction * 100)}%")
            except Exception:
                # Widgets destroyed (tab rebuilt) — drop this entry, keep going
                self._progress_widgets.pop(key, None)
        if any_active:
            self._schedule_poll()

    def _finish_download(self, model_key: str, state: dict) -> None:
        row, _bar, _percent = self._progress_widgets.pop(model_key, (None, None, None))
        if row is not None:
            row.destroy()
        refs = self._card_refs.get(model_key)
        if not refs:
            return
        if state.get("error"):
            refs["status"].configure(text="Download failed — check logs",
                                     text_color=self._colors.danger)
            if refs["download"] is None:  # let the user retry
                refs["download"] = self._make_download_button(refs["actions"], model_key)
            return
        refs["status"].configure(text="✓ Ready", text_color=self._colors.success)
        refs["select"].configure(state="normal", text_color=self._colors.fg_primary)
        # Refresh the General tab's model info line ("Downloaded" status)
        self.ctx.emit("model_changed", self._settings.model_size)

    # -- Selection (in-place highlight swap) --------------------------------------

    def _select_model_from_card(self, key: str) -> None:
        # A model change needs a fresh process (in-process swap crashes on
        # this CUDA stack). change_model confirms via a one-time dialog, then
        # cleanly restarts the app to load the new model.
        self.ctx.change_model(key)
