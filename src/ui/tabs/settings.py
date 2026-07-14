"""Settings view (gear icon): appearance + everything that isn't one of the
three main sections — the Overlay controls and the About panel fold in here
as sections (ref/ui structure: General/Models/History up top, the rest behind
the gear). No repeated titles: sections carry the labels, pages don't."""

import customtkinter as ctk

from ...theme import SPACING, font
from ..layout import TabBase
from ..icons import icon, icon_font
from ...theme import RADIUS_SM
from ..widgets import ghost_button, primary_button, secondary_button, segmented
from .about import AboutTab
from .overlay import OverlayTab


class SettingsTab(TabBase):
    KEY = "settings"

    def build(self, tab) -> None:

        # -- Appearance: Theme | UI Zoom side by side ----------------------------
        self._icon_section_label(tab, "theme", "APPEARANCE")
        if self._get_col_count() >= 2:
            grid = ctk.CTkFrame(tab, fg_color="transparent")
            grid.pack(fill="x", pady=SPACING["xs"])
            grid.columnconfigure(0, weight=1, uniform="appearance")
            grid.columnconfigure(1, weight=1, uniform="appearance")
            col0 = ctk.CTkFrame(grid, fg_color="transparent")
            col0.grid(row=0, column=0, sticky="nsew")
            col1 = ctk.CTkFrame(grid, fg_color="transparent")
            col1.grid(row=0, column=1, sticky="nsew")
            self._build_theme_card(col0)
            self._build_zoom_card(col1)
        else:
            self._build_theme_card(tab)
            self._build_zoom_card(tab)

        # -- Dictation power-tools + system (moved from General) -------------------
        if self._get_col_count() >= 2:
            col0, col1 = self._columns(tab, 2)
        else:
            col0 = col1 = tab
        self._icon_section_label(col0, "general", "DICTATION EXTRAS")
        card = self._card(col0)
        self._fillers_var = self._build_switch_row(
            card, "Remove Filler Words", '"um", "uh", "er" (needs auto-format)',
            self._settings.remove_fillers, self._on_fillers_changed)
        self._voice_cmd_var = self._build_switch_row(
            card, "Voice Commands", '"new line", "period", "delete that"',
            self._settings.voice_commands, self._on_voice_commands_changed)
        right = self._setting_row(card, "Text Replacements", "Auto-correct words after transcription")
        secondary_button(
            right, self._colors,
            text=f"Edit…  ({len(self._settings.replacements)})", width=100,
            command=self._open_replacements_editor,
        ).pack()
        right = self._setting_row(card, "Per-App Paste Rules", "Force typing or clipboard for specific apps")
        secondary_button(
            right, self._colors,
            text=f"Edit…  ({len(self._settings.paste_rules)})", width=100,
            command=self._open_paste_rules_editor,
        ).pack()

        self._icon_section_label(col1, "settings", "SYSTEM")
        card = self._card(col1)
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
        right = self._setting_row(card, "Start with Windows", "Launch Eqho on login")
        self._startup_var = self._bool_var(value=self._settings.start_with_windows)
        from ..widgets import themed_switch
        themed_switch(
            right, self._colors, variable=self._startup_var,
            command=self._on_startup_changed,
        ).pack()

        # -- Overlay (moved from its old tab; options only, no inner titles) ------
        self._icon_section_label(tab, "overlay", "OVERLAY")
        OverlayTab(self.ctx).build(tab, embedded=True)

        # -- About (moved from its old tab) ----------------------------------------
        self._icon_section_label(tab, "about", "ABOUT EQHO")
        AboutTab(self.ctx).build(tab, embedded=True)

        self._add_bottom_padding(tab)

    def _build_theme_card(self, parent) -> None:
        card = self._card(parent)
        right = self._setting_row(card, "Theme", "Light, dark, or follow Windows")
        labels = {"light": "Light", "dark": "Dark", "system": "System"}
        self._theme_var = self._string_var(
            value=labels.get(self._settings.theme, "System"),
        )
        segmented(
            right, self._colors,
            values=list(labels.values()),
            variable=self._theme_var,
            command=self._on_theme_changed,
        ).pack()

    def _build_zoom_card(self, parent) -> None:
        card = self._card(parent)
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

    def _icon_section_label(self, parent, icon_name: str, text: str) -> None:
        """Section divider with an accent Phosphor glyph — the warmth accents."""
        row = ctk.CTkFrame(parent, fg_color="transparent")
        row.pack(fill="x", padx=SPACING["md"], pady=(SPACING["lg"], 3))
        ctk.CTkLabel(
            row, text=icon(icon_name),
            font=icon_font("sm", 3),
            text_color=self._colors.accent, fg_color="transparent",
        ).pack(side="left", padx=(0, 6))
        ctk.CTkLabel(
            row, text=text,
            font=font("sm", "bold"),
            text_color=self._colors.fg_secondary, anchor="w",
        ).pack(side="left")

    def _on_theme_changed(self, val: str) -> None:
        mode = {"Light": "light", "Dark": "dark", "System": "system"}.get(val, "system")
        if mode != self._settings.theme:
            self.ctx.set_theme(mode)

    def _on_zoom_changed(self, value: str) -> None:
        scale = int(value.rstrip("%")) / 100.0
        if abs(scale - self._settings.ui_scale) > 0.01:
            self.ctx.set_ui_scale(scale)

    # -- Dictation extras / system (moved from General) ----------------------------

    def _on_fillers_changed(self, value: bool) -> None:
        self._settings.remove_fillers = value
        self._settings.save()

    def _on_voice_commands_changed(self, value: bool) -> None:
        self._settings.voice_commands = value
        self._settings.save()

    def _on_duck_changed(self, val, labels) -> None:
        reverse = {v: k for k, v in labels.items()}
        self._settings.volume_duck = reverse.get(val, "mute")
        self._settings.save()

    def _on_startup_changed(self) -> None:
        enabled = self._startup_var.get()
        self._settings.start_with_windows = enabled
        self._settings.save()
        from ... import oskit
        from ...oskit.base import autostart_command
        oskit.get().set_autostart(enabled, autostart_command())

    def _rules_editor(self, title: str, hint: str, initial: str, on_save) -> None:
        parent = self.ctx.master()
        top = ctk.CTkToplevel(parent)
        top.title(title)
        top.geometry("420x340")
        top.transient(parent)
        top.grab_set()
        top.configure(fg_color=self._colors.bg_primary)

        ctk.CTkLabel(
            top, text=hint,
            font=font("sm"), text_color=self._colors.fg_secondary,
        ).pack(anchor="w", padx=SPACING["md"], pady=(SPACING["md"], SPACING["xs"]))

        box = ctk.CTkTextbox(
            top, corner_radius=RADIUS_SM, font=font("sm"),
            fg_color=self._colors.bg_tertiary, text_color=self._colors.fg_primary,
            border_width=1, border_color=self._colors.border,
        )
        box.pack(fill="both", expand=True, padx=SPACING["md"])
        if initial:
            box.insert("1.0", initial)

        def _save() -> None:
            on_save(box.get("1.0", "end"))
            top.destroy()
            self.ctx.rebuild_tab(self.KEY)  # refresh the rule count on the button

        buttons = ctk.CTkFrame(top, fg_color="transparent")
        buttons.pack(fill="x", padx=SPACING["md"], pady=SPACING["md"])
        primary_button(buttons, self._colors, text="Save", width=80,
                       command=_save).pack(side="right")
        ghost_button(buttons, self._colors, text="Cancel", width=70,
                     command=top.destroy).pack(side="right", padx=(0, SPACING["xs"]))

    def _open_replacements_editor(self) -> None:
        def _save(raw: str) -> None:
            rules = {}
            for line in raw.splitlines():
                if "=>" in line:
                    src, dst = line.split("=>", 1)
                    src, dst = src.strip(), dst.strip()
                    if src:
                        rules[src] = dst
            self._settings.replacements = rules
            self._settings.save()

        self._rules_editor(
            "Text Replacements",
            "One rule per line:   spoken text => written text",
            "\n".join(f"{s} => {d}" for s, d in self._settings.replacements.items()),
            _save,
        )

    def _open_paste_rules_editor(self) -> None:
        def _save(raw: str) -> None:
            rules = {}
            for line in raw.splitlines():
                if "=" in line:
                    app, mode = line.split("=", 1)
                    app, mode = app.strip().lower(), mode.strip().lower()
                    if app and mode in ("typing", "clipboard"):
                        rules[app] = mode
            self._settings.paste_rules = rules
            self._settings.save()

        self._rules_editor(
            "Per-App Paste Rules",
            "One rule per line:   app.exe = typing   (or = clipboard)",
            "\n".join(f"{a} = {m}" for a, m in self._settings.paste_rules.items()),
            _save,
        )
