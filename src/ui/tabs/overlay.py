"""Overlay tab: visibility, position, and appearance of the floating bar."""

import customtkinter as ctk

from ...fonts import FONT_FAMILY
from ...theme import FONT_SIZES, SPACING, RADIUS_SM
from ..layout import TabBase


class OverlayTab(TabBase):
    KEY = "overlay"

    def build(self, tab) -> None:
        self._tab_header(tab, "Overlay", "Floating transcription preview bar")

        cols = self._get_col_count()

        if cols >= 2:
            grid = self._make_grid_container(tab)

            col0 = ctk.CTkFrame(grid, fg_color="transparent")
            col0.grid(row=0, column=0, sticky="nsew", padx=SPACING["xs"])

            self._section_label(col0, "VISIBILITY")
            card = self._card(col0)
            self._build_overlay_switch(card)

            self._section_label(col0, "POSITION")
            card = self._card(col0)
            self._build_position_setting(card)

            col1 = ctk.CTkFrame(grid, fg_color="transparent")
            col1.grid(row=0, column=1, sticky="nsew", padx=SPACING["xs"])

            self._section_label(col1, "APPEARANCE")
            card = self._card(col1)
            self._build_opacity_setting(card)
            self._build_fontsize_setting(card)
        else:
            # Single column
            self._section_label(tab, "VISIBILITY")
            card = self._card(tab)
            self._build_overlay_switch(card)

            self._section_label(tab, "POSITION")
            card = self._card(tab)
            self._build_position_setting(card)

            self._section_label(tab, "APPEARANCE")
            card = self._card(tab)
            self._build_opacity_setting(card)
            self._build_fontsize_setting(card)

        self._add_bottom_padding(tab)

    def _build_overlay_switch(self, card) -> None:
        right = self._setting_row(card, "Show Overlay", "Display transcription text while dictating")
        self._overlay_var = ctk.BooleanVar(value=self._settings.overlay_enabled)
        ctk.CTkSwitch(
            right, text="", variable=self._overlay_var,
            onvalue=True, offvalue=False,
            command=self._on_overlay_toggle,
            width=44, height=22,
        ).pack()

    def _build_position_setting(self, card) -> None:
        right = self._setting_row(card, "Screen Position", "Where the overlay appears")
        pos_labels = {
            "bottom-center": "Bottom Center", "top-center": "Top Center",
            "top-left": "Top Left", "top-right": "Top Right",
            "bottom-left": "Bottom Left", "bottom-right": "Bottom Right",
        }
        current_pos = pos_labels.get(self._settings.overlay_position, "Bottom Center")
        self._pos_var = ctk.StringVar(value=current_pos)
        self._dropdown(
            right, variable=self._pos_var,
            values=list(pos_labels.values()),
            width=160, height=30,
            corner_radius=RADIUS_SM,
            font=(FONT_FAMILY, FONT_SIZES["sm"]),
            dropdown_font=(FONT_FAMILY, FONT_SIZES["sm"]),
            command=lambda v: self._on_pos_changed(v, pos_labels),
        ).pack()

    def _build_opacity_setting(self, card) -> None:
        opacity_row = ctk.CTkFrame(card, fg_color="transparent")
        opacity_row.pack(fill="x", padx=SPACING["lg"], pady=SPACING["sm"])

        ctk.CTkLabel(
            opacity_row, text="Opacity",
            font=(FONT_FAMILY, FONT_SIZES["base"]),
            text_color=self._colors.fg_primary, anchor="w",
        ).pack(anchor="w")

        slider_frame = ctk.CTkFrame(opacity_row, fg_color="transparent")
        slider_frame.pack(fill="x")

        self._opacity_val_label = ctk.CTkLabel(
            slider_frame,
            text=f"{int(self._settings.overlay_opacity * 100)}%",
            font=(FONT_FAMILY, FONT_SIZES["sm"]),
            text_color=self._colors.fg_secondary,
            width=40,
        )
        self._opacity_val_label.pack(side="right", padx=(SPACING["sm"], 0))

        ctk.CTkSlider(
            slider_frame,
            from_=0.3, to=1.0,
            number_of_steps=14,
            command=self._on_opacity_changed,
            height=16,
        ).pack(side="left", fill="x", expand=True)

    def _build_fontsize_setting(self, card) -> None:
        font_row = ctk.CTkFrame(card, fg_color="transparent")
        font_row.pack(fill="x", padx=SPACING["lg"], pady=SPACING["sm"])

        ctk.CTkLabel(
            font_row, text="Font Size",
            font=(FONT_FAMILY, FONT_SIZES["base"]),
            text_color=self._colors.fg_primary, anchor="w",
        ).pack(anchor="w")

        fs_frame = ctk.CTkFrame(font_row, fg_color="transparent")
        fs_frame.pack(fill="x")

        self._fontsize_val_label = ctk.CTkLabel(
            fs_frame,
            text=f"{self._settings.overlay_font_size}px",
            font=(FONT_FAMILY, FONT_SIZES["sm"]),
            text_color=self._colors.fg_secondary,
            width=40,
        )
        self._fontsize_val_label.pack(side="right", padx=(SPACING["sm"], 0))

        ctk.CTkSlider(
            fs_frame,
            from_=10, to=28,
            number_of_steps=18,
            command=self._on_fontsize_changed,
            height=16,
        ).pack(side="left", fill="x", expand=True)

    def _on_overlay_toggle(self) -> None:
        self._settings.overlay_enabled = self._overlay_var.get()
        self._settings.save()

    def _on_pos_changed(self, val, labels) -> None:
        reverse = {v: k for k, v in labels.items()}
        key = reverse.get(val, "bottom-center")
        self._settings.overlay_position = key
        self._settings.save()

    def _on_opacity_changed(self, val) -> None:
        self._settings.overlay_opacity = round(val, 2)
        self._settings.save()
        self._opacity_val_label.configure(text=f"{int(val * 100)}%")

    def _on_fontsize_changed(self, val) -> None:
        self._settings.overlay_font_size = int(val)
        self._settings.save()
        self._fontsize_val_label.configure(text=f"{int(val)}px")
