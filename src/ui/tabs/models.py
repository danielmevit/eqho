"""Models tab: Whisper model cards with selection, download, and progress.

All state changes (select, download start/finish/fail) update the existing
card widgets IN PLACE — never a tab rebuild, which flashed the whole view.
"""

import customtkinter as ctk

from ...modelpick import hardware_class, recommend_model
from ...modelstore import is_downloading, start_download, status as download_status
from ...theme import MODEL_INFO, SPACING, RADIUS_LG, RADIUS_SM, font
from ..layout import EDGE, ROW_GAP, TabBase, column_inset
from ..widgets import secondary_button, auto_wrap_label

_EN_MODELS = ["distil-large-v3", "distil-medium.en", "distil-small.en"]
_ML_MODELS = ["large-v3-turbo", "medium", "small", "base", "tiny", "large-v3"]


FAILED_TEXT = "Download failed"  # short: it sits next to the model name
FAILED_HINT = "Check the internet connection, then press Try again."


def runs_on_text(hw: str) -> str:
    """The one line at the top of the tab saying where models run."""
    if hw == "gpu":
        return "On this PC, speech models run on your graphics card."
    return ("On this PC, speech models run on the processor — "
            "the smaller models are much faster here.")


def detail_text(info: dict, hw: str) -> str:
    """Card detail line: "<languages> · <size>" (+ device note on gpu PCs)."""
    parts = [info.get("lang", ""), info.get("size", "")]
    if hw == "gpu" and info.get("device"):
        parts.append(info["device"])
    return " · ".join(parts)


class ModelsTab(TabBase):
    KEY = "models"

    def __init__(self, ctx):
        super().__init__(ctx)
        self._card_refs: dict = {}
        self._progress_widgets: dict = {}
        self._poll_widget = None
        self._selected_key = ""
        # The dashboard's "model_changed" handler marks every OTHER tab stale
        # — this one stays as-is, so it must move its own highlight in place.
        ctx.subscribe("model_changed", self.KEY, self._on_model_changed)

    def build(self, tab) -> None:
        self._card_refs = {}
        self._progress_widgets = {}
        self._poll_widget = tab
        self._selected_key = self._settings.model_size

        cols = self._get_col_count()

        ctk.CTkLabel(
            tab, text=runs_on_text(hardware_class()),
            font=font("xs"), text_color=self._colors.fg_muted, anchor="w",
        ).pack(fill="x", padx=EDGE, pady=(SPACING["sm"], 0))

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

        # Engine + storage belong with the models (moved out of General —
        # they're model infrastructure, not daily-driver settings).
        if cols >= 2:
            col0, col1 = self._columns(tab, 2)
        else:
            col0 = col1 = tab
        self._section_label(col0, "INFERENCE ENGINE")
        card = self._card(col0)
        self._build_engine_setting(card)
        self._section_label(col1, "STORAGE")
        card = self._card(col1)
        self._build_storage_setting(card)

        self._add_bottom_padding(tab)

        if self._progress_widgets:
            self._schedule_poll()

    def _build_engine_setting(self, card) -> None:
        import importlib.util

        control = self._setting_row(
            card, "Inference Engine", "Auto picks the fastest for your hardware")

        cpp_available = importlib.util.find_spec("pywhispercpp") is not None
        self._engine_keys = ["auto", "faster-whisper", "whisper.cpp"]
        self._engine_display_names = [
            "Auto (recommended)",
            "faster-whisper (NVIDIA / CPU)",
            "whisper.cpp (AMD / Intel / CPU)" if cpp_available
            else "whisper.cpp (not installed)",
        ]
        current = self._settings.engine_backend
        idx = self._engine_keys.index(current) if current in self._engine_keys else 0
        self._engine_var = self._string_var(value=self._engine_display_names[idx])

        self._dropdown(
            control,
            variable=self._engine_var,
            values=self._engine_display_names,
            width=200, height=30,
            corner_radius=RADIUS_SM,
            font=font("sm"),
            dropdown_font=font("sm"),
            command=self._on_engine_changed,
        ).pack()

    def _on_engine_changed(self, display_name) -> None:
        idx = self._engine_display_names.index(display_name)
        key = self._engine_keys[idx]
        if key != self._settings.engine_backend:
            self._settings.engine_backend = key
            self._settings.save()
            # Respawns the model host on the new backend (seamless, like a mic
            # change) — see App._on_settings_changed and transcriber.set_engine.
            self._apply_settings(reload_model=True)

    def _build_storage_setting(self, card) -> None:
        control = self._setting_row(card, "Model Storage", "Where downloaded models live")
        path = str(self._settings.resolve_model_dir())
        path_lbl = ctk.CTkLabel(
            control, text=path,
            font=font("xs"), text_color=self._colors.fg_muted,
            anchor="w", justify="left",
        )
        path_lbl.pack(anchor="w", fill="x")
        # Follow the real width (a fixed wraplength overflowed at high scaling);
        # Tk breaks a long path mid-word once it exceeds the wrap width.
        auto_wrap_label(path_lbl, control, padding=4)
        secondary_button(
            control, self._colors, text="Open Folder", width=100,
            command=lambda: self._open_folder(path),
        ).pack(anchor="w", pady=(SPACING["xs"], 0))

    @staticmethod
    def _open_folder(path: str) -> None:
        import os
        import subprocess
        import sys
        try:
            if sys.platform == "win32":
                os.startfile(path)  # noqa: S606 — local folder open
            elif sys.platform == "darwin":
                subprocess.Popen(["open", path])
            else:
                subprocess.Popen(["xdg-open", path])
        except Exception:
            pass

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
        card.pack(fill="x", padx=self._hpad(parent), pady=(0, ROW_GAP))
        self._fill_model_card(card, model_key, is_selected)

    def _build_model_card_grid(self, grid, model_key: str, col: int, row: int) -> None:
        is_selected = self._settings.model_size == model_key
        card = self._card_frame(is_selected, grid)
        card.grid(row=row, column=col, sticky="nsew",
                  padx=column_inset(col, self._get_col_count()),
                  pady=(0, ROW_GAP))  # rows of one section sit closer than the columns
        self._fill_model_card(card, model_key, is_selected)

    @staticmethod
    def _stack_when_narrow(row, name_label, status_label) -> None:
        """Keep the status beside the title while both fit; otherwise move it
        under the title (narrow cards) so the two never overlap."""
        state = {"stacked": False}

        def _check(_event=None):
            try:
                need = name_label.winfo_reqwidth() + status_label.winfo_reqwidth() + 8
                narrow = row.winfo_width() > 1 and need > row.winfo_width()
                if narrow == state["stacked"]:
                    return
                state["stacked"] = narrow
                status_label.pack_forget()
                if narrow:
                    status_label.pack(side="top", anchor="w")
                    name_label.pack_configure(side="top", anchor="w")
                else:
                    name_label.pack_configure(side="left", anchor="center")
                    status_label.pack(side="right")
            except Exception:
                pass

        row.bind("<Configure>", _check, add="+")
        row.after_idle(_check)

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

        failed = (not cached and not downloading
                  and bool((download_status(model_key) or {}).get("error")))
        if cached:
            status_text, status_color = "✓ Ready", self._colors.success
        elif failed:
            status_text, status_color = FAILED_TEXT, self._colors.danger
        elif downloading:
            status_text, status_color = "Downloading…", self._colors.accent
        else:
            status_text, status_color = "Not downloaded", self._colors.fg_muted
        status_label = ctk.CTkLabel(
            top, text=status_text, font=font("xs"), text_color=status_color,
        )
        status_label.pack(side="right")
        self._stack_when_narrow(top, name_label, status_label)

        detail = detail_text(info, hardware_class())
        detail_lbl = ctk.CTkLabel(
            inner, text=detail,
            font=font("xs"),
            text_color=self._colors.fg_secondary, anchor="w",
        )
        detail_lbl.pack(fill="x")
        auto_wrap_label(detail_lbl, card)

        rec = info.get("rec", "")
        if model_key == recommend_model(self._settings.language):
            rec = "Recommended for this PC"
        if rec:
            rec_lbl = ctk.CTkLabel(
                inner, text=rec,
                font=font("xs"),
                text_color=self._colors.fg_muted, anchor="w",
            )
            rec_lbl.pack(fill="x")
            auto_wrap_label(rec_lbl, card)

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
            "base_name": info.get("name", model_key), "retry": None,
        }
        if failed:
            self._show_retry(model_key)

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

    def _show_retry(self, model_key: str) -> None:
        """"Try again" row under the card's text. Packed into the card body, not
        the actions row, so it shows on the selected card too."""
        refs = self._card_refs.get(model_key)
        if not refs or refs.get("retry") is not None:
            return
        row = ctk.CTkFrame(refs["inner"], fg_color="transparent")
        row.pack(fill="x", pady=(SPACING["xs"], 0))
        hint = ctk.CTkLabel(
            row, text=FAILED_HINT, font=font("xs"),
            text_color=self._colors.fg_muted, anchor="w", justify="left",
        )
        hint.pack(fill="x")
        auto_wrap_label(hint, row, padding=4)
        btn = secondary_button(
            row, self._colors, text="Try again",
            font=font("xs"), height=24,
            command=lambda k=model_key: self._start_download(k),
        )
        btn.pack(anchor="e", pady=(SPACING["xs"], 0))
        refs["retry"] = row
        refs["retry_button"] = btn

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
        if refs.get("retry") is not None:
            refs["retry"].destroy()
            refs["retry"] = refs["retry_button"] = None
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
            refs["status"].configure(text=FAILED_TEXT, text_color=self._colors.danger)
            self._show_retry(model_key)
            return
        refs["status"].configure(text="✓ Ready", text_color=self._colors.success)
        refs["select"].configure(state="normal", text_color=self._colors.fg_primary)
        # Refresh the General tab's model info line ("Downloaded" status)
        self.ctx.emit("model_changed", self._settings.model_size)

    # -- Selection (in-place highlight swap) --------------------------------------

    def _select_model_from_card(self, key: str) -> None:
        # Switch models seamlessly — the model host swaps in the background
        # (a fresh child process loads the new model), no app restart needed.
        self.ctx.change_model(key)

    def _on_model_changed(self, model_key: str) -> None:
        """Selection moved (Select button, download finish, onboarding) —
        swap the card highlight in place, no rebuild."""
        old_key = self._selected_key
        self._selected_key = model_key
        if old_key and old_key != model_key:
            self._apply_card_selection(old_key, False)
        self._apply_card_selection(model_key, True)

    def _apply_card_selection(self, model_key: str, selected: bool) -> None:
        """Restyle one existing card to the selected/deselected look."""
        refs = self._card_refs.get(model_key)
        if not refs:
            return
        try:
            refs["card"].configure(
                border_width=2 if selected else 1,
                border_color=(self._colors.accent if selected
                              else self._colors.border_subtle),
            )
            refs["name"].configure(
                text=refs["base_name"] + ("  ●" if selected else ""),
                text_color=(self._colors.accent if selected
                            else self._colors.fg_primary),
            )
            if selected:
                refs["actions"].pack_forget()
            else:
                refs["actions"].pack(fill="x", pady=(SPACING["xs"], 0))
        except Exception:
            # Widgets are gone (tab mid-rebuild) — build() re-reads settings.
            pass
