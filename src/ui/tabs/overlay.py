"""Overlay tab: visibility, position, and appearance of the floating bar."""

import customtkinter as ctk

from ...theme import SPACING, RADIUS_SM, font
from ..layout import TabBase
from ..widgets import themed_switch


class OverlayTab(TabBase):
    KEY = "overlay"

    def build(self, tab, embedded: bool = False) -> None:
        # `embedded` = rendered as a section inside the Settings view (no own
        # header; the caller provides the section label).
        if not embedded:
            self._tab_header(tab, "Overlay", "Floating transcription preview bar", icon="overlay")

        cols = self._get_col_count()

        if cols >= 2:
            grid = self._make_grid_container(tab)

            col0 = self._column_frame(grid, 0, cols)

            if not embedded:
                self._section_label(col0, "VISIBILITY")
            card = self._card(col0)
            self._build_overlay_switch(card)

            if not embedded:
                self._section_label(col0, "POSITION")
            card = self._card(col0)
            self._build_position_setting(card)

            col1 = self._column_frame(grid, 1, cols)

            if not embedded:
                self._section_label(col1, "APPEARANCE")
            card = self._card(col1)
            self._build_opacity_setting(card)
            self._build_fontsize_setting(card)
        else:
            # Single column
            if not embedded:
                self._section_label(tab, "VISIBILITY")
            card = self._card(tab)
            self._build_overlay_switch(card)

            if not embedded:
                self._section_label(tab, "POSITION")
            card = self._card(tab)
            self._build_position_setting(card)

            if not embedded:
                self._section_label(tab, "APPEARANCE")
            card = self._card(tab)
            self._build_opacity_setting(card)
            self._build_fontsize_setting(card)

        if not embedded:
            self._add_bottom_padding(tab)

    def _build_overlay_switch(self, card) -> None:
        right = self._setting_row(card, "Show Overlay", "Display transcription text while dictating")
        self._overlay_var = self._bool_var(value=self._settings.overlay_enabled)
        themed_switch(
            right, self._colors, variable=self._overlay_var,
            command=self._on_overlay_toggle,
        ).pack()

        right = self._setting_row(card, "Audio Level Indicator", "Small line that moves with your voice")
        self._level_var = self._bool_var(value=self._settings.overlay_show_level)
        themed_switch(
            right, self._colors, variable=self._level_var,
            command=self._on_level_toggle,
        ).pack()

    def _build_position_setting(self, card) -> None:
        right = self._setting_row(card, "Screen Position", "Where the overlay appears")
        pos_labels = {
            "bottom-center": "Bottom Center", "top-center": "Top Center",
            "top-left": "Top Left", "top-right": "Top Right",
            "bottom-left": "Bottom Left", "bottom-right": "Bottom Right",
        }
        current_pos = pos_labels.get(self._settings.overlay_position, "Bottom Center")
        self._pos_var = self._string_var(value=current_pos)
        self._dropdown(
            right, variable=self._pos_var,
            values=list(pos_labels.values()),
            width=160, height=30,
            corner_radius=RADIUS_SM,
            font=font("sm"),
            dropdown_font=font("sm"),
            command=lambda v: self._on_pos_changed(v, pos_labels),
        ).pack()

    def _build_opacity_setting(self, card) -> None:
        self._build_pill_size_setting(card)
        # The sliders show how SEE-THROUGH it is (right = clearer); the settings
        # keep storing opacity, so value = 1 - opacity (opacity 0.3–1.0).
        see_pill = 1.0 - self._settings.overlay_opacity
        see_panel = 1.0 - self._settings.overlay_panel_opacity
        self._pill_opacity_val_label = self._slider_row(
            card, "Pill see-through", see_pill, 0.0, 0.7, 14,
            self._pct(see_pill), self._on_opacity_changed, ends=("Solid", "Clear"))
        self._panel_opacity_val_label = self._slider_row(
            card, "Text box see-through", see_panel, 0.0, 0.7, 14,
            self._pct(see_panel), self._on_panel_opacity_changed, ends=("Solid", "Clear"))

    def _build_pill_size_setting(self, card) -> None:
        self._pill_size_val_label = self._slider_row(
            card, "Pill size", self._settings.overlay_pill_scale, 0.6, 1.4, 8,
            self._pct(self._settings.overlay_pill_scale), self._on_pill_size_changed,
            ends=("Small", "Large"))

    def _build_fontsize_setting(self, card) -> None:
        self._fontsize_val_label = self._slider_row(
            card, "Text size", self._settings.overlay_font_size, 10, 28, 18,
            f"{self._settings.overlay_font_size}px", self._on_fontsize_changed)

    @staticmethod
    def _pct(val: float) -> str:
        return f"{int(round(val * 100))}%"

    def _slider_row(self, card, title, value, lo, hi, steps, val_text, command, ends=None):
        """Title, then slider + value label on one line; returns the value label."""
        row = ctk.CTkFrame(card, fg_color="transparent")
        row.pack(fill="x", padx=SPACING["md"], pady=SPACING["sm"])

        ctk.CTkLabel(
            row, text=title,
            font=font("base"),
            text_color=self._colors.fg_primary, anchor="w",
        ).pack(anchor="w")

        slider_frame = ctk.CTkFrame(row, fg_color="transparent")
        slider_frame.pack(fill="x")

        val_label = ctk.CTkLabel(
            slider_frame,
            text=val_text,
            font=font("sm"),
            text_color=self._colors.fg_secondary,
            width=40,
        )
        val_label.pack(side="right", padx=(SPACING["sm"], 0))

        if ends:
            ctk.CTkLabel(slider_frame, text=ends[1], font=font("sm"),
                         text_color=self._colors.fg_secondary).pack(side="right", padx=(SPACING["xs"], 0))
            ctk.CTkLabel(slider_frame, text=ends[0], font=font("sm"),
                         text_color=self._colors.fg_secondary).pack(side="left", padx=(0, SPACING["xs"]))

        slider = ctk.CTkSlider(
            slider_frame,
            from_=lo, to=hi,
            number_of_steps=steps,
            command=command,
            height=16,
            fg_color=self._colors.bg_tertiary,
            progress_color=self._colors.accent,
            button_color=self._colors.accent,
            button_hover_color=self._colors.accent_hover,
        )
        slider.set(value)
        slider.pack(side="left", fill="x", expand=True)
        return val_label

    def _on_overlay_toggle(self) -> None:
        self._settings.overlay_enabled = self._overlay_var.get()
        self._settings.save()

    def _on_level_toggle(self) -> None:
        self._settings.overlay_show_level = self._level_var.get()
        self._settings.save()

    def _on_pos_changed(self, val, labels) -> None:
        reverse = {v: k for k, v in labels.items()}
        key = reverse.get(val, "bottom-center")
        self._settings.overlay_position = key
        self._settings.save()

    def _on_opacity_changed(self, val) -> None:  # val = see-through amount
        self._settings.overlay_opacity = round(1.0 - val, 2)
        self._settings.save()
        self._pill_opacity_val_label.configure(text=self._pct(val))

    def _on_panel_opacity_changed(self, val) -> None:  # val = see-through amount
        self._settings.overlay_panel_opacity = round(1.0 - val, 2)
        self._settings.save()
        self._panel_opacity_val_label.configure(text=self._pct(val))

    def _on_pill_size_changed(self, val) -> None:
        self._settings.overlay_pill_scale = round(val, 2)
        self._settings.save()
        self._pill_size_val_label.configure(text=self._pct(val))

    def _on_fontsize_changed(self, val) -> None:
        self._settings.overlay_font_size = int(val)
        self._settings.save()
        self._fontsize_val_label.configure(text=f"{int(val)}px")
