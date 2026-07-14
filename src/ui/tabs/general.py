"""General tab — "how do I dictate?": mic, hotkey, language, vocabulary, and
the daily behavior toggles. Model/engine choices live in Models; power-user
extras (fillers, voice commands, replacements, paste rules, ducking, startup)
live in Settings (gear)."""

import logging

import customtkinter as ctk

from ...audio import list_input_devices
from ...settings import LANGUAGE_TIERS, SUPPORTED_LANGUAGES, is_english_only_model
from ...theme import SPACING, RADIUS_SM, font
from ..layout import TabBase
from ..widgets import segmented

log = logging.getLogger(__name__)


class GeneralTab(TabBase):
    KEY = "general"

    def __init__(self, ctx):
        super().__init__(ctx)
        self._hotkey_capturing = False
        self._hotkey_hook = None
        self._captured_keys: set[str] = set()
        # Model changes (Models tab) affect the English-only language warning.
        ctx.subscribe("model_changed", self.KEY, self._on_external_model_change)

    def build(self, tab) -> None:
        cols = self._get_col_count()
        if cols == 1:
            self._build_sections(tab, tab, tab)
        elif cols == 2:
            col0, col1 = self._columns(tab, 2)
            self._build_sections(col0, col1, col1)
        else:
            col0, col1, col2 = self._columns(tab, 3)
            self._build_sections(col0, col1, col2)
        self._add_bottom_padding(tab)

    def _build_sections(self, input_col, speech_col, behavior_col) -> None:
        """The four General sections, distributed over 1–3 columns."""
        self._section_label(input_col, "AUDIO INPUT")
        card = self._card(input_col)
        self._build_mic_setting(card)
        self._build_sensitivity_setting(card)

        self._section_label(input_col, "HOTKEY")
        card = self._card(input_col)
        self._build_hotkey_setting(card)
        self._build_hotkey_mode_setting(card)

        self._section_label(speech_col, "SPEECH")
        card = self._card(speech_col)
        self._build_language_setting(card)
        self._build_vocab_setting(card)

        self._section_label(behavior_col, "BEHAVIOR")
        card = self._card(behavior_col)
        self._build_behavior_settings(card)

    # -- Audio input -------------------------------------------------------------

    def _build_mic_setting(self, card) -> None:
        right = self._setting_row(card, "Microphone", "Select your input device")

        devices = list_input_devices()
        device_names = ["System Default"] + [name for _, name in devices]
        device_indices = [None] + [idx for idx, _ in devices]

        current_idx = self._settings.audio_device
        current_name = "System Default"
        for idx, name in devices:
            if idx == current_idx:
                current_name = name
                break

        self._mic_var = self._string_var(value=current_name)
        self._dropdown(
            right,
            variable=self._mic_var,
            values=device_names,
            width=200, height=30,
            corner_radius=RADIUS_SM,
            font=font("sm"),
            dropdown_font=font("sm"),
            command=lambda val: self._on_mic_changed(val, device_names, device_indices),
        ).pack()

    def _build_sensitivity_setting(self, card) -> None:
        control = self._setting_row(card, "Mic Sensitivity", "Higher picks up quieter speech")
        srow = ctk.CTkFrame(control, fg_color="transparent")
        srow.pack(fill="x")
        self._sens_label = ctk.CTkLabel(
            srow, text=self._sens_text(self._settings.vad_sensitivity),
            font=font("xs"), text_color=self._colors.fg_secondary, width=52,
        )
        self._sens_label.pack(side="right", padx=(SPACING["sm"], 0))
        slider = ctk.CTkSlider(
            srow, from_=0.5, to=2.0, number_of_steps=15,
            command=self._on_sens_changed, height=16,
            fg_color=self._colors.bg_tertiary,
            progress_color=self._colors.accent,
            button_color=self._colors.accent,
            button_hover_color=self._colors.accent_hover,
        )
        slider.set(self._settings.vad_sensitivity)
        slider.pack(side="left", fill="x", expand=True)

    def _sens_text(self, value: float) -> str:
        if value <= 0.8:
            return "Low"
        if value >= 1.4:
            return "High"
        return "Normal"

    def _on_sens_changed(self, value: float) -> None:
        self._settings.vad_sensitivity = round(value, 2)
        self._settings.save()
        self._sens_label.configure(text=self._sens_text(value))

    # -- Hotkey ------------------------------------------------------------------

    def _build_hotkey_setting(self, card) -> None:
        control = self._setting_row(card, "Global Hotkey", "Press to activate/deactivate dictation")

        self._hotkey_display = ctk.CTkButton(
            control,
            text=self._format_hotkey(self._settings.hotkey),
            font=font("lg", "bold"),
            width=160, height=36,
            corner_radius=RADIUS_SM,
            fg_color=self._colors.bg_tertiary,
            text_color=self._colors.accent,
            hover_color=self._colors.bg_hover,
            border_width=1,
            border_color=self._colors.border,
            command=self._start_hotkey_capture,
        )
        self._hotkey_display.pack(anchor="w")

        self._hotkey_hint = ctk.CTkLabel(
            control, text="Click to change",
            font=font("xs"),
            text_color=self._colors.fg_muted,
        )
        self._hotkey_hint.pack(anchor="w", pady=(2, 0))

    def _build_hotkey_mode_setting(self, card) -> None:
        right = self._setting_row(card, "Hotkey Mode", "Toggle on/off or hold to talk")
        self._mode_var = self._string_var(value=self._settings.hotkey_mode)
        segmented(
            right, self._colors,
            values=["toggle", "hold"],
            variable=self._mode_var,
            command=self._on_mode_changed,
        ).pack()

    # -- Speech ------------------------------------------------------------------

    def _build_language_setting(self, card) -> None:
        right = self._setting_row(card, "Language", "Transcription language")
        # All 99 Whisper languages, grouped by honesty tier (see settings.py).
        # Headers ("— … —") render as non-clickable rows in ThemedDropdown.
        self._lang_by_name: dict[str, str] = {}
        values: list[str] = []
        for header, tier in (
            ("— hand-tuned —", LANGUAGE_TIERS["tuned"]),
            ("— strong accuracy —", LANGUAGE_TIERS["strong"]),
            ("— experimental · quality varies —", LANGUAGE_TIERS["experimental"]),
        ):
            values.append(header)
            for code, name in tier.items():
                values.append(name)
                self._lang_by_name[name] = code
        current_lang = SUPPORTED_LANGUAGES.get(self._settings.language, "English")
        self._lang_var = self._string_var(value=current_lang)

        self._dropdown(
            right,
            variable=self._lang_var,
            values=values,
            width=170, height=30,
            corner_radius=RADIUS_SM,
            font=font("sm"),
            dropdown_font=font("sm"),
            command=self._on_lang_changed,
        ).pack()

        # Honesty guard: a non-English language on an English-only model would
        # transcribe garbage — say so right where the choice happens.
        self._lang_warn = ctk.CTkLabel(
            right,
            text="",
            font=font("xs"),
            text_color=self._colors.accent,
            fg_color="transparent",
            anchor="e",
            justify="right",
            wraplength=230,
        )
        self._update_lang_warning()

    def _build_vocab_setting(self, card) -> None:
        row = ctk.CTkFrame(card, fg_color="transparent")
        row.pack(fill="x", padx=SPACING["md"], pady=SPACING["sm"])
        ctk.CTkLabel(
            row, text="Custom Vocabulary",
            font=font("base"), text_color=self._colors.fg_primary, anchor="w",
        ).pack(anchor="w")
        ctk.CTkLabel(
            row, text="Names and jargon Whisper should expect (used as its prompt)",
            font=font("xs"), text_color=self._colors.fg_muted, anchor="w",
        ).pack(anchor="w")
        self._vocab_box = ctk.CTkTextbox(
            row, height=56, corner_radius=RADIUS_SM, font=font("sm"),
            fg_color=self._colors.bg_tertiary, text_color=self._colors.fg_primary,
            border_width=1, border_color=self._colors.border, wrap="word",
        )
        self._vocab_box.pack(fill="x", pady=(SPACING["xs"], 0))
        if self._settings.initial_prompt:
            self._vocab_box.insert("1.0", self._settings.initial_prompt)
        self._vocab_box.bind("<FocusOut>", self._on_vocab_changed)

    def _on_vocab_changed(self, _event=None) -> None:
        text = self._vocab_box.get("1.0", "end").strip()
        if text != self._settings.initial_prompt:
            self._settings.initial_prompt = text
            self._settings.save()

    # -- Behavior (daily toggles) --------------------------------------------------

    def _build_behavior_settings(self, card) -> None:
        self._format_var = self._build_switch_row(
            card, "Auto-format", "Fix capitalization & spacing",
            self._settings.format_cleanup, self._on_format_changed)
        self._chime_var = self._build_switch_row(
            card, "Sound Feedback", "Soft chime on start/stop",
            self._settings.sound_feedback, self._on_chime_changed)
        self._history_var = self._build_switch_row(
            card, "Save History", "Keep dictations in the History tab",
            self._settings.history_enabled, self._on_history_changed)

        right = self._setting_row(card, "Paste Mode", "How text is injected into apps")
        self._paste_var = self._string_var(value="Clipboard" if self._settings.auto_paste else "Typing")
        segmented(
            right, self._colors,
            values=["Clipboard", "Typing"],
            variable=self._paste_var,
            command=self._on_paste_changed,
        ).pack()

    # -- Callbacks ---------------------------------------------------------------

    def _on_external_model_change(self, model_key: str) -> None:
        """Model changed in the Models tab — refresh the language warning."""
        self._update_lang_warning()
        self.refresh_header_status()

    def _on_mic_changed(self, val, names, indices) -> None:
        idx = indices[names.index(val)] if val in names else None
        self._settings.audio_device = idx
        self._settings.save()
        self._apply_settings(reload_model=True)

    def _on_mode_changed(self, val) -> None:
        self._settings.hotkey_mode = val
        self._settings.save()
        self._apply_settings(reload_model=False)

    def _on_paste_changed(self, val) -> None:
        self._settings.auto_paste = val == "Clipboard"
        self._settings.save()

    def _on_format_changed(self, value: bool) -> None:
        self._settings.format_cleanup = value
        self._settings.save()

    def _on_chime_changed(self, value: bool) -> None:
        self._settings.sound_feedback = value
        self._settings.save()

    def _on_history_changed(self, value: bool) -> None:
        self._settings.history_enabled = value
        self._settings.save()

    def _on_lang_changed(self, val) -> None:
        code = self._lang_by_name.get(val)
        if code is None:  # a tier header slipped through — ignore
            return
        self._settings.language = code
        self._settings.save()
        self._update_lang_warning()
        # Language is passed per-transcribe — no model reload/restart needed.
        self._apply_settings(reload_model=False)

    def _update_lang_warning(self) -> None:
        """Show/hide the English-only-model warning under the language picker."""
        mismatch = (
            self._settings.language != "en"
            and is_english_only_model(self._settings.model_size)
        )
        if mismatch:
            self._lang_warn.configure(
                text="This model is English-only — pick a multilingual model\n"
                     "(e.g. Large v3 Turbo) in the Models tab.",
            )
            self._lang_warn.pack(pady=(4, 0))
        else:
            self._lang_warn.pack_forget()

    # -- Hotkey capture --------------------------------------------------------

    def _start_hotkey_capture(self) -> None:
        if self._hotkey_capturing:
            return
        import keyboard
        self._hotkey_capturing = True
        self._captured_keys.clear()
        self._hotkey_display.configure(
            text="Press a key combo...",
            fg_color=self._colors.accent_muted,
            text_color=self._colors.danger,
        )
        self._hotkey_hint.configure(text="Esc to cancel")
        self._hotkey_hook = keyboard.hook(self._on_hotkey_event, suppress=False)

    def _on_hotkey_event(self, event) -> None:
        import keyboard as kb
        name = event.name.lower() if event.name else ""
        if name == "esc":
            self._stop_hotkey_capture(cancelled=True)
            return

        if event.event_type == kb.KEY_DOWN:
            self._captured_keys.add(name)
            combo = self._keys_to_combo(self._captured_keys)
            try:
                self._hotkey_display.after(0, lambda: self._hotkey_display.configure(
                    text=self._format_hotkey(combo), text_color=self._colors.accent))
            except Exception:
                pass
        elif event.event_type == kb.KEY_UP:
            if self._captured_keys:
                combo = self._keys_to_combo(self._captured_keys)
                self._settings.hotkey = combo
                self._settings.save()
                self._stop_hotkey_capture(cancelled=False)
                self._apply_settings(reload_model=False)

    def _keys_to_combo(self, keys: set[str]) -> str:
        mod_names = {"ctrl", "left ctrl", "right ctrl", "alt", "left alt", "right alt",
                     "shift", "left shift", "right shift", "left windows", "right windows"}
        mod_map = {
            "left ctrl": "ctrl", "right ctrl": "ctrl",
            "left alt": "alt", "right alt": "alt",
            "left shift": "shift", "right shift": "shift",
            "left windows": "win", "right windows": "win",
        }
        modifiers, regular = [], []
        for k in keys:
            if k in mod_names:
                canonical = mod_map.get(k, k)
                if canonical not in modifiers:
                    modifiers.append(canonical)
            else:
                regular.append(k)
        mod_order = {"ctrl": 0, "alt": 1, "shift": 2, "win": 3}
        modifiers.sort(key=lambda m: mod_order.get(m, 9))
        return "+".join(modifiers + regular)

    def _stop_hotkey_capture(self, cancelled: bool) -> None:
        import keyboard
        self._hotkey_capturing = False
        if self._hotkey_hook is not None:
            try:
                keyboard.unhook(self._hotkey_hook)
            except Exception:
                pass
            self._hotkey_hook = None

        combo = self._settings.hotkey
        try:
            self._hotkey_display.after(0, lambda: [
                self._hotkey_display.configure(
                    text=self._format_hotkey(combo),
                    fg_color=self._colors.bg_tertiary,
                    text_color=self._colors.accent,
                ),
                self._hotkey_hint.configure(text="Click to change"),
            ])
        except Exception:
            pass
