"""General tab: audio input, hotkey, model, and behavior settings."""

import logging

import customtkinter as ctk

from ...audio import list_input_devices
from ...settings import SUPPORTED_LANGUAGES, WHISPER_MODELS
from ...theme import MODEL_INFO, SPACING, RADIUS_SM, font
from ..layout import TabBase
from ..widgets import ghost_button, primary_button, secondary_button, segmented, themed_switch

log = logging.getLogger(__name__)


class GeneralTab(TabBase):
    KEY = "general"

    def __init__(self, ctx):
        super().__init__(ctx)
        self._hotkey_capturing = False
        self._hotkey_hook = None
        self._captured_keys: set[str] = set()
        # React when another tab (Models) changes the model selection.
        ctx.subscribe("model_changed", self.KEY, self._on_external_model_change)

    def build(self, tab) -> None:
        self._tab_header(tab, "General", "Core settings for dictation")

        cols = self._get_col_count()

        if cols == 1:
            self._build_general_1col(tab)
        else:
            self._build_general_multi(tab, cols)
        self._add_bottom_padding(tab)

    def _build_general_1col(self, tab) -> None:
        """Single-column layout (narrow window)."""
        # Audio Input
        self._section_label(tab, "AUDIO INPUT")
        card = self._card(tab)
        self._build_mic_setting(card)

        # Hotkey
        self._section_label(tab, "HOTKEY")
        card = self._card(tab)
        self._build_hotkey_setting(card)
        self._build_hotkey_mode_setting(card)

        # Model
        self._section_label(tab, "MODEL")
        card = self._card(tab)
        self._build_model_setting(card)

        # Behavior
        self._section_label(tab, "BEHAVIOR")
        card = self._card(tab)
        self._build_behavior_settings(card)

        # Dictation (local features)
        self._section_label(tab, "DICTATION")
        card = self._card(tab)
        self._build_vocab_setting(card)
        self._build_dictation_settings(card)

        # Interface
        self._section_label(tab, "INTERFACE")
        card = self._card(tab)
        self._build_interface_settings(card)

    def _build_general_multi(self, tab, cols: int) -> None:
        """Multi-column grid layout."""
        grid = self._make_grid_container(tab)

        # Distribute sections across columns
        # Col 0: Audio + Hotkey
        col0 = ctk.CTkFrame(grid, fg_color="transparent")
        col0.grid(row=0, column=0, sticky="nsew", padx=0)

        self._section_label(col0, "AUDIO INPUT")
        card = self._card(col0)
        self._build_mic_setting(card)

        self._section_label(col0, "HOTKEY")
        card = self._card(col0)
        self._build_hotkey_setting(card)
        self._build_hotkey_mode_setting(card)

        # Col 1: Model + Behavior
        col1 = ctk.CTkFrame(grid, fg_color="transparent")
        col1.grid(row=0, column=1, sticky="nsew", padx=0)

        self._section_label(col1, "MODEL")
        card = self._card(col1)
        self._build_model_setting(card)

        self._section_label(col1, "BEHAVIOR")
        card = self._card(col1)
        self._build_behavior_settings(card)

        # Col 2 (if 3-col): pull behavior out, add language/startup separately
        if cols >= 3:
            col2 = ctk.CTkFrame(grid, fg_color="transparent")
            col2.grid(row=0, column=2, sticky="nsew", padx=0)

            self._section_label(col2, "BEHAVIOR")
            card = self._card(col2)
            self._build_language_setting(card)
            self._build_startup_setting(card)

            self._section_label(col2, "DICTATION")
            card = self._card(col2)
            self._build_vocab_setting(card)
            self._build_dictation_settings(card)

            self._section_label(col2, "INTERFACE")
            card = self._card(col2)
            self._build_interface_settings(card)
        else:
            # 2-col: dictation + interface balance under Audio + Hotkey
            self._section_label(col0, "DICTATION")
            card = self._card(col0)
            self._build_vocab_setting(card)
            self._build_dictation_settings(card)

            self._section_label(col0, "INTERFACE")
            card = self._card(col0)
            self._build_interface_settings(card)

    # -- Dictation section (local features, v0.5.0) ------------------------------

    def _build_switch_row(self, card, label: str, desc: str, value: bool, on_change):
        right = self._setting_row(card, label, desc)
        var = self._bool_var(value=value)
        themed_switch(
            right, self._colors, variable=var,
            command=lambda: on_change(var.get()),
        ).pack()
        return var

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

    def _build_dictation_settings(self, card) -> None:
        self._voice_cmd_var = self._build_switch_row(
            card, "Voice Commands", '"new line", "period", "delete that"',
            self._settings.voice_commands, self._on_voice_commands_changed)
        self._chime_var = self._build_switch_row(
            card, "Sound Feedback", "Soft chime on start/stop",
            self._settings.sound_feedback, self._on_chime_changed)
        self._history_var = self._build_switch_row(
            card, "Save History", "Keep dictations in the History tab",
            self._settings.history_enabled, self._on_history_changed)

        right = self._setting_row(card, "Text Replacements", "Auto-correct words after transcription")
        secondary_button(
            right, self._colors,
            text=f"Edit…  ({len(self._settings.replacements)})", width=100,
            command=self._open_replacements_editor,
        ).pack()

    def _build_interface_settings(self, card) -> None:
        control = self._setting_row(card, "UI Zoom", "Scale the whole dashboard")
        levels = ["100%", "125%", "150%", "175%", "200%"]
        current = f"{int(round(self._settings.ui_scale * 100))}%"
        self._zoom_var = self._string_var(value=current if current in levels else "150%")
        segmented(
            control, self._colors,
            values=levels,
            variable=self._zoom_var,
            command=self._on_zoom_changed,
        ).pack(anchor="w")

    def _on_zoom_changed(self, value: str) -> None:
        scale = int(value.rstrip("%")) / 100.0
        if abs(scale - self._settings.ui_scale) > 0.01:
            self.ctx.set_ui_scale(scale)

    def _on_vocab_changed(self, _event=None) -> None:
        text = self._vocab_box.get("1.0", "end").strip()
        if text != self._settings.initial_prompt:
            self._settings.initial_prompt = text
            self._settings.save()

    def _on_voice_commands_changed(self, value: bool) -> None:
        self._settings.voice_commands = value
        self._settings.save()

    def _on_chime_changed(self, value: bool) -> None:
        self._settings.sound_feedback = value
        self._settings.save()

    def _on_history_changed(self, value: bool) -> None:
        self._settings.history_enabled = value
        self._settings.save()

    def _open_replacements_editor(self) -> None:
        parent = self._vocab_box.winfo_toplevel()
        top = ctk.CTkToplevel(parent)
        top.title("Text Replacements")
        top.geometry("420x340")
        top.transient(parent)
        top.grab_set()
        top.configure(fg_color=self._colors.bg_primary)

        ctk.CTkLabel(
            top, text="One rule per line:   spoken text => written text",
            font=font("sm"), text_color=self._colors.fg_secondary,
        ).pack(anchor="w", padx=SPACING["md"], pady=(SPACING["md"], SPACING["xs"]))

        box = ctk.CTkTextbox(
            top, corner_radius=RADIUS_SM, font=font("sm"),
            fg_color=self._colors.bg_tertiary, text_color=self._colors.fg_primary,
            border_width=1, border_color=self._colors.border,
        )
        box.pack(fill="both", expand=True, padx=SPACING["md"])
        if self._settings.replacements:
            box.insert("1.0", "\n".join(
                f"{src} => {dst}" for src, dst in self._settings.replacements.items()
            ))

        def _save() -> None:
            rules = {}
            for line in box.get("1.0", "end").splitlines():
                if "=>" in line:
                    src, dst = line.split("=>", 1)
                    src, dst = src.strip(), dst.strip()
                    if src:
                        rules[src] = dst
            self._settings.replacements = rules
            self._settings.save()
            top.destroy()
            self.ctx.rebuild_tab(self.KEY)  # refresh the rule count on the button

        buttons = ctk.CTkFrame(top, fg_color="transparent")
        buttons.pack(fill="x", padx=SPACING["md"], pady=SPACING["md"])
        primary_button(buttons, self._colors, text="Save", width=80,
                       command=_save).pack(side="right")
        ghost_button(buttons, self._colors, text="Cancel", width=70,
                     command=top.destroy).pack(side="right", padx=(0, SPACING["xs"]))

    # -- General tab building blocks (reusable in any layout) ------------------

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

    def _build_model_setting(self, card) -> None:
        control = self._setting_row(card, "Whisper Model")

        self._model_info_label = ctk.CTkLabel(
            control, text=self._get_model_info_text(self._settings.model_size),
            font=font("xs"),
            text_color=self._colors.fg_muted, anchor="w", justify="left",
        )
        self._model_info_label.pack(anchor="w")

        model_right = ctk.CTkFrame(control, fg_color="transparent")
        model_right.pack(anchor="w", pady=(SPACING["xs"], 0))

        model_display_names = [MODEL_INFO[k]["name"] for k in WHISPER_MODELS]
        model_keys = list(WHISPER_MODELS.keys())
        current_model_name = MODEL_INFO.get(self._settings.model_size, {}).get("name", "Unknown")

        self._model_var = self._string_var(value=current_model_name)
        self._model_keys = model_keys
        self._model_display_names = model_display_names

        self._dropdown(
            model_right,
            variable=self._model_var,
            values=model_display_names,
            width=180, height=30,
            corner_radius=RADIUS_SM,
            font=font("sm"),
            dropdown_font=font("sm"),
            command=self._on_model_changed,
        ).pack()

    def _build_behavior_settings(self, card) -> None:
        # Volume while speaking
        right = self._setting_row(card, "Volume While Speaking", "System volume during dictation")
        duck_labels = {"off": "Off", "50%": "50%", "25%": "25%", "10%": "10%", "mute": "Mute"}
        self._duck_var = self._string_var(value=duck_labels.get(self._settings.volume_duck, "Mute"))
        self._dropdown(
            right,
            variable=self._duck_var,
            values=list(duck_labels.values()),
            width=100, height=30,
            corner_radius=RADIUS_SM,
            font=font("sm"),
            dropdown_font=font("sm"),
            command=lambda val: self._on_duck_changed(val, duck_labels),
        ).pack()

        # Paste mode
        right = self._setting_row(card, "Paste Mode", "How text is injected into apps")
        self._paste_var = self._string_var(value="Clipboard" if self._settings.auto_paste else "Typing")
        segmented(
            right, self._colors,
            values=["Clipboard", "Typing"],
            variable=self._paste_var,
            command=self._on_paste_changed,
        ).pack()

        # Language
        self._build_language_setting(card)

        # Start with Windows
        self._build_startup_setting(card)

    def _build_language_setting(self, card) -> None:
        right = self._setting_row(card, "Language", "Transcription language")
        lang_names = list(SUPPORTED_LANGUAGES.values())
        lang_codes = list(SUPPORTED_LANGUAGES.keys())
        current_lang = SUPPORTED_LANGUAGES.get(self._settings.language, "English")
        self._lang_var = self._string_var(value=current_lang)
        self._lang_codes = lang_codes
        self._lang_names = lang_names

        self._dropdown(
            right,
            variable=self._lang_var,
            values=lang_names,
            width=140, height=30,
            corner_radius=RADIUS_SM,
            font=font("sm"),
            dropdown_font=font("sm"),
            command=self._on_lang_changed,
        ).pack()

    def _build_startup_setting(self, card) -> None:
        right = self._setting_row(card, "Start with Windows", "Launch Eqho on login")
        self._startup_var = self._bool_var(value=self._settings.start_with_windows)
        themed_switch(
            right, self._colors, variable=self._startup_var,
            command=self._on_startup_changed,
        ).pack()

    # -- Callbacks ---------------------------------------------------------------

    def _get_model_info_text(self, model_key: str) -> str:
        info = MODEL_INFO.get(model_key, {})
        icon = info.get("icon", "")
        lang = info.get("lang", "")
        size = info.get("size", "")
        device = info.get("device", "")
        rec = info.get("rec", "")
        cached = self._is_model_cached(model_key)
        status = "Downloaded" if cached else "Not downloaded"
        return f"{icon} {lang} · {size} · {device}\n{rec} · {status}"

    def _on_external_model_change(self, model_key: str) -> None:
        """Another tab changed the model — sync our dropdown + info label."""
        name = MODEL_INFO.get(model_key, {}).get("name", model_key)
        self._model_var.set(name)
        self._model_info_label.configure(text=self._get_model_info_text(model_key))
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

    def _on_model_changed(self, display_name) -> None:
        idx = self._model_display_names.index(display_name)
        key = self._model_keys[idx]
        if key != self._settings.model_size:
            self._settings.model_size = key
            self._settings.save()
            self._model_info_label.configure(text=self._get_model_info_text(key))
            self.refresh_header_status()
            self.ctx.emit("model_changed", key)
            self._apply_settings(reload_model=True)

    def _on_duck_changed(self, val, labels) -> None:
        reverse = {v: k for k, v in labels.items()}
        key = reverse.get(val, "mute")
        self._settings.volume_duck = key
        self._settings.save()

    def _on_paste_changed(self, val) -> None:
        self._settings.auto_paste = val == "Clipboard"
        self._settings.save()

    def _on_lang_changed(self, val) -> None:
        idx = self._lang_names.index(val)
        code = self._lang_codes[idx]
        self._settings.language = code
        self._settings.save()
        self._apply_settings(reload_model=True)

    def _on_startup_changed(self) -> None:
        enabled = self._startup_var.get()
        self._settings.start_with_windows = enabled
        self._settings.save()
        from ... import oskit
        from ...oskit.base import autostart_command
        oskit.get().set_autostart(enabled, autostart_command())

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
