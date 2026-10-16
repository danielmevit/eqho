"""First-time setup — a short walk-through shown inside the dashboard window.

A frame the dashboard shows instead of its tabs (no own Tk root). Pages:
welcome, language, microphone, speech model, sound, try it. Nothing is
downloaded and no sound setting changes until the user picks it here.
"""

import logging
from typing import Callable, Optional

import customtkinter as ctk

from .. import modelstore, oskit
from ..modelpick import recommend_model
from ..settings import SUPPORTED_LANGUAGES, WHISPER_MODELS
from ..theme import SPACING, RADIUS_SM, font
from .layout import TabBase
from .widgets import (primary_button, secondary_button, ghost_button,
                      auto_wrap_label)

log = logging.getLogger(__name__)

PAGES = ("welcome", "language", "microphone", "model", "sound", "try")

MIC_BLOCKED_TEXT = ("Windows is blocking the microphone. Turn on microphone "
                    "access for desktop apps, then come back to this page.")
NO_MIC_TEXT = "No microphone found. Plug one in, then choose it here."
LATER_TEXT = "Eqho will download it the first time you dictate."
DOWNLOAD_FAILED_TEXT = ("The download did not finish. Check the internet "
                        "connection, then press Download now again.")


def language_choices() -> list[tuple[str, str]]:
    """(label, code): English first, then the rest by name."""
    others = sorted((name, code) for code, name in SUPPORTED_LANGUAGES.items()
                    if code != "en")
    return [("English", "en"), *others]


def model_label(model_key: str) -> str:
    """'Distil Large v3' from 'Distil Large v3 (~1.5 GB, …)'."""
    return WHISPER_MODELS.get(model_key, model_key).split(" (")[0]


def model_size_text(model_key: str) -> str:
    mb = modelstore.MODEL_SIZES_MB.get(model_key, 1000)
    return f"{mb / 1000:.1f} GB" if mb >= 1000 else f"{mb} MB"


class OnboardingScreen(TabBase):
    """The setup pages. `on_done(skipped, model_changed)` ends setup."""

    KEY = "onboarding"

    def __init__(self, ctx, parent, on_done: Callable[[bool, bool], None]):
        super().__init__(ctx)
        self._on_done = on_done
        self._start_model = self._settings.model_size
        if not self._settings.onboarding_done:
            # Fresh install: offer the model that is fast on this PC. "Run
            # setup again" keeps the model unless a language is picked.
            self._settings.model_size = recommend_model(self._settings.language)
            self._settings.save()
        self._page = 0
        self._stream = None
        self._level = 0.0
        self._level_job = None
        self._poll_job = None
        self._chose_later = False
        self._closed = False

        self.frame = ctk.CTkFrame(parent, fg_color=self._colors.bg_primary,
                                  corner_radius=0)
        # The nav row is packed BEFORE the body so it keeps its space at any
        # window height; the body scrolls when a page doesn't fit.
        nav = ctk.CTkFrame(self.frame, fg_color="transparent")
        nav.pack(side="bottom", fill="x", padx=SPACING["2xl"], pady=SPACING["lg"])
        self._scroll = ctk.CTkScrollableFrame(
            self.frame, fg_color="transparent", corner_radius=0,
            scrollbar_button_color=self._colors.bg_tertiary,
            scrollbar_button_hover_color=self._colors.bg_hover)
        self._scroll.pack(side="top", fill="both", expand=True,
                          padx=(SPACING["2xl"], 0), pady=(SPACING["xl"], 0))
        self._body = ctk.CTkFrame(self._scroll, fg_color="transparent")
        self._body.pack(fill="both", expand=True, padx=(0, SPACING["2xl"]))
        try:  # CTkScrollableFrame keeps its canvas private
            self._scroll._parent_canvas.bind("<Configure>", self._fit_scroll, add="+")
        except Exception:
            pass
        self._body.bind("<Configure>", self._fit_scroll, add="+")
        self._skip_btn = ghost_button(nav, self._colors, text="Skip setup",
                                      width=110, command=self.skip)
        self._skip_btn.pack(side="left")
        self._next_btn = primary_button(nav, self._colors, text="Next",
                                        width=110, height=34, command=self.next)
        self._next_btn.pack(side="right")
        self._back_btn = secondary_button(nav, self._colors, text="Back",
                                          width=110, height=34, command=self.back)
        self._back_btn.pack(side="right", padx=(0, SPACING["sm"]))
        self._step_label = ctk.CTkLabel(nav, text="", font=font("sm"),
                                        text_color=self._colors.fg_secondary)
        self._step_label.pack(side="right", padx=SPACING["lg"])
        self.show_page(0)

    # -- navigation ----------------------------------------------------------------

    @property
    def page_name(self) -> str:
        return PAGES[self._page]

    def show_page(self, index: int) -> None:
        self._leave_page()
        self._page = max(0, min(len(PAGES) - 1, index))
        for child in self._body.winfo_children():
            child.destroy()
        last = self._page == len(PAGES) - 1
        self._back_btn.configure(state="normal" if self._page else "disabled")
        self._next_btn.configure(text="Finish" if last else "Next")
        self._set_primary(self._next_btn, True)
        self._step_label.configure(text=f"Step {self._page + 1} of {len(PAGES)}")
        getattr(self, f"_build_{self.page_name}")()
        try:
            self._scroll._parent_canvas.yview_moveto(0)
        except Exception:
            pass
        self.frame.after_idle(self._fit_scroll)

    def _fit_scroll(self, _event=None) -> None:
        """Show the scrollbar (and allow scrolling) only when the page is
        taller than the space above the nav row."""
        try:
            canvas = self._scroll._parent_canvas
            bar = self._scroll._scrollbar
            fits = self._body.winfo_reqheight() <= canvas.winfo_height()
            if fits:
                canvas.yview_moveto(0)
                if bar.winfo_ismapped():
                    bar.grid_remove()
            elif not bar.winfo_ismapped():
                bar.grid()
        except Exception:
            pass

    def next(self) -> None:
        if self._page == len(PAGES) - 1:
            self._finish(skipped=False)
        else:
            self.show_page(self._page + 1)

    def back(self) -> None:
        if self._page:
            self.show_page(self._page - 1)

    def skip(self) -> None:
        self._finish(skipped=True)

    def _finish(self, skipped: bool) -> None:
        self.close()
        self._on_done(skipped, self._settings.model_size != self._start_model)

    def close(self) -> None:
        """Stop the mic and timers (leaving setup, or the window closing)."""
        self._leave_page()
        self._closed = True

    def _set_primary(self, btn, enabled: bool) -> None:
        """Enable/disable a primary button. Disabled = dark secondary fill with
        dimmed but readable text (not light blue on blue)."""
        c = self._colors
        if enabled:
            btn.configure(state="normal", fg_color=c.accent, border_width=0)
        else:
            btn.configure(state="disabled", fg_color=c.bg_tertiary, border_width=1,
                          border_color=c.border, text_color_disabled=c.fg_muted)

    def _leave_page(self) -> None:
        self._close_mic_stream()
        for job in ("_level_job", "_poll_job"):
            jid = getattr(self, job)
            if jid is not None:
                try:
                    self.frame.after_cancel(jid)
                except Exception:
                    pass
                setattr(self, job, None)

    # -- page helpers ----------------------------------------------------------------

    def _title(self, text: str) -> None:
        ctk.CTkLabel(self._body, text=text, font=font("2xl", "bold"),
                     text_color=self._colors.fg_primary, anchor="w",
                     justify="left").pack(fill="x", pady=(0, SPACING["md"]))

    def _text(self, text: str, parent=None, color=None) -> ctk.CTkLabel:
        lbl = ctk.CTkLabel(parent or self._body, text=text, font=font("lg"),
                           text_color=color or self._colors.fg_primary,
                           anchor="w", justify="left")
        lbl.pack(fill="x", pady=(0, SPACING["sm"]))
        auto_wrap_label(lbl, parent or self._body, padding=4)
        return lbl

    # -- pages ------------------------------------------------------------------------

    def _build_welcome(self) -> None:
        self._title("Welcome to Eqho")
        self._text("Eqho types what you say, in any app. Everything stays on "
                   "this computer — nothing you say is sent anywhere.")
        self._text("This short setup takes about a minute.",
                   color=self._colors.fg_secondary)

    def _build_language(self) -> None:
        self._title("Which language will you speak?")
        choices = language_choices()
        self._lang_codes = {label: code for label, code in choices}
        current = self._settings.language
        current_label = next((l for l, c in choices if c == current), "English")
        self._lang_var = self._string_var(value=current_label)
        self._dropdown(self._body, variable=self._lang_var,
                       values=[l for l, _ in choices], width=320, height=34,
                       corner_radius=RADIUS_SM, font=font("base"),
                       dropdown_font=font("sm"),
                       command=self._on_language).pack(anchor="w")

    def _on_language(self, label: str) -> None:
        """Only an actual pick saves; showing the page changes nothing."""
        self._save_language(label)

    def _save_language(self, label: str) -> None:
        code = self._lang_codes.get(label, "en")
        self._settings.language = code
        self._settings.model_size = recommend_model(code)
        self._settings.save()

    def _build_microphone(self) -> None:
        from ..audio import list_input_devices
        self._title("Which microphone should Eqho use?")
        try:
            devices = list_input_devices()
        except Exception:
            log.debug("Listing microphones failed", exc_info=True)
            devices = []
        self._mic_names = ["System Default"] + [n for _, n in devices]
        self._mic_indices = [None] + [i for i, _ in devices]
        current = "System Default"
        for idx, name in devices:
            if idx == self._settings.audio_device:
                current = name
        self._mic_var = self._string_var(value=current)
        self._dropdown(self._body, variable=self._mic_var, values=self._mic_names,
                       width=320, height=34, corner_radius=RADIUS_SM,
                       font=font("base"), dropdown_font=font("sm"),
                       command=self._on_mic).pack(anchor="w", pady=(0, SPACING["lg"]))
        self._text("Say something — the bar should move.")
        self._level_bar = ctk.CTkProgressBar(
            self._body, height=14, corner_radius=7,
            fg_color=self._colors.bg_tertiary, progress_color=self._colors.accent)
        self._level_bar.set(0)
        self._level_bar.pack(fill="x", pady=(0, SPACING["md"]))
        self._mic_msg = self._text("", color=self._colors.warning)
        self._mic_btn_row = ctk.CTkFrame(self._body, fg_color="transparent")
        self._mic_btn_row.pack(fill="x")
        self._restart_mic()

    def _on_mic(self, name: str) -> None:
        try:
            idx = self._mic_indices[self._mic_names.index(name)]
        except ValueError:
            idx = None
        self._settings.audio_device = idx
        self._settings.save()
        self._restart_mic()

    def _restart_mic(self) -> None:
        self._close_mic_stream()
        self._mic_msg.configure(text="")
        for child in self._mic_btn_row.winfo_children():
            child.destroy()
        if self._open_mic_stream(self._settings.audio_device):
            self._tick_level()
        else:
            self._show_mic_problem()

    def _open_mic_stream(self, device: Optional[int]) -> bool:
        """Open the level-meter stream. Overridden by tools/test_ui.py."""
        try:
            import numpy as np
            import sounddevice as sd

            def _cb(indata, frames, time_info, status):
                rms = float(np.sqrt(np.mean(np.square(indata))))
                self._level = min(1.0, (rms / 0.012) ** 0.6)

            self._stream = sd.InputStream(device=device, channels=1,
                                          samplerate=16000, callback=_cb)
            self._stream.start()
            return True
        except Exception:
            log.info("Setup: the microphone could not open", exc_info=True)
            self._close_mic_stream()
            return False

    def _close_mic_stream(self) -> None:
        stream, self._stream = self._stream, None
        if stream is not None:
            try:
                stream.stop()
                stream.close()
            except Exception:
                log.debug("Closing the setup mic stream failed", exc_info=True)
        self._level = 0.0

    def _tick_level(self) -> None:
        try:
            self._level_bar.set(self._level)
        except Exception:
            return
        self._level_job = self.frame.after(50, self._tick_level)

    def _show_mic_problem(self) -> None:
        kit = oskit.get()
        try:
            blocked = kit.mic_access_blocked()
        except Exception:
            blocked = None
        if blocked:
            self._mic_msg.configure(text=MIC_BLOCKED_TEXT)
            secondary_button(self._mic_btn_row, self._colors,
                             text="Open Windows settings", height=34,
                             command=kit.open_mic_privacy_settings).pack(side="left")
            secondary_button(self._mic_btn_row, self._colors, text="Try again",
                             height=34, command=self._restart_mic).pack(
                side="left", padx=(SPACING["sm"], 0))
        else:
            self._mic_msg.configure(text=NO_MIC_TEXT)

    def _build_model(self) -> None:
        model = self._settings.model_size
        self._title("The speech model")
        if self._is_model_cached(model):
            self._text(f"The speech model ({model_label(model)}) is already on "
                       "this computer. Nothing to download.")
            return
        self._text(f"Eqho needs to download its speech model once: "
                   f"{model_label(model)}, about {model_size_text(model)}. "
                   "It stays on this computer.")
        row = self._dl_row = ctk.CTkFrame(self._body, fg_color="transparent")
        row.pack(fill="x", pady=(SPACING["sm"], SPACING["md"]))
        self._dl_btn = primary_button(row, self._colors, text="Download now",
                                      width=150, height=34, command=self._download)
        self._dl_btn.pack(side="left")
        self._later_btn = secondary_button(row, self._colors, text="Later",
                                           width=110, height=34, command=self._later)
        self._later_btn.pack(side="left", padx=(SPACING["sm"], 0))
        self._dl_bar = ctk.CTkProgressBar(
            self._body, height=14, corner_radius=7,
            fg_color=self._colors.bg_tertiary, progress_color=self._colors.accent)
        self._dl_bar.set(0)
        self._dl_msg = self._text("", color=self._colors.fg_secondary)
        if modelstore.is_downloading(model):
            self._show_download_running()
        elif self._chose_later:
            self._dl_msg.configure(text=LATER_TEXT)
        else:
            self._set_primary(self._next_btn, False)  # pick one first

    def _download(self) -> None:
        self._chose_later = False
        self._start_download(self._settings.model_size)
        self._show_download_running()

    def _start_download(self, model: str) -> None:
        """Start the real download. Overridden by tools/test_ui.py."""
        modelstore.start_download(self._settings, model)

    def _show_download_running(self) -> None:
        self._set_primary(self._dl_btn, False)
        self._later_btn.configure(state="disabled")
        self._set_primary(self._next_btn, False)
        self._dl_bar.pack(fill="x", pady=(0, SPACING["sm"]), before=self._dl_msg)
        self._dl_msg.configure(text="Downloading…")
        self._poll_download()

    def _poll_download(self) -> None:
        self._poll_job = None
        st = modelstore.status(self._settings.model_size)
        try:
            self._dl_bar.set(st.get("progress", 0.0))
            if st and st.get("done"):
                if st.get("error"):
                    self._dl_msg.configure(text=DOWNLOAD_FAILED_TEXT)
                    self._set_primary(self._dl_btn, True)
                    self._later_btn.configure(state="normal")
                else:
                    self._dl_row.pack_forget()
                    self._dl_bar.set(1.0)
                    self._dl_msg.configure(text="Done. The speech model is ready.")
                    self._set_primary(self._next_btn, True)
                return
        except Exception:
            return  # page was left
        self._poll_job = self.frame.after(500, self._poll_download)

    def _later(self) -> None:
        self._chose_later = True
        self._dl_msg.configure(text=LATER_TEXT)
        self._set_primary(self._next_btn, True)

    def _build_sound(self) -> None:
        self._title("Other sounds")
        self._text("Turn other sounds off while you dictate? It helps Eqho hear you.")
        row = self._dl_row = ctk.CTkFrame(self._body, fg_color="transparent")
        row.pack(fill="x", pady=(SPACING["sm"], 0))
        self._sound_btns = {}
        for value, label in (("mute", "Yes, turn them off"), ("off", "No, leave them")):
            btn = secondary_button(row, self._colors, text=label, width=170, height=34,
                                   command=lambda v=value: self._on_sound(v))
            btn.pack(side="left", padx=(0, SPACING["sm"]))
            self._sound_btns[value] = btn
        self._mark_sound()

    def _on_sound(self, value: str) -> None:
        self._settings.volume_duck = value
        self._settings.save()
        self._mark_sound()

    def _mark_sound(self) -> None:
        chosen = "mute" if self._settings.volume_duck == "mute" else "off"
        for value, btn in self._sound_btns.items():
            on = value == chosen
            btn.configure(fg_color=self._colors.accent if on else self._colors.bg_tertiary,
                          text_color=self._colors.on_accent if on else self._colors.fg_primary,
                          hover_color=self._colors.accent_hover if on else self._colors.bg_hover)

    def _build_try(self) -> None:
        hk = self._format_hotkey(self._settings.hotkey)
        self._title("Try it")
        if self._settings.hotkey_mode == "hold":
            how = f"Hold {hk} down, say a sentence, then let go."
        else:
            how = f"Press {hk}, say a sentence, press {hk} again."
        self._text(f"{how} Your words appear where your cursor is. Try it here:")
        self._try_box = ctk.CTkTextbox(
            self._body, height=120, font=font("lg"), corner_radius=RADIUS_SM,
            fg_color=self._colors.bg_secondary, text_color=self._colors.fg_primary,
            border_width=1, border_color=self._colors.border)
        self._try_box.pack(fill="x", pady=(SPACING["sm"], 0))
        self.frame.after(50, self._focus_try_box)

    def _focus_try_box(self) -> None:
        try:
            self._try_box.focus_set()
        except Exception:
            pass
