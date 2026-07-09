"""History tab (placeholder until Phase 5 ships the transcript log)."""

import customtkinter as ctk

from ...fonts import FONT_FAMILY
from ...theme import FONT_SIZES, SPACING
from ..layout import TabBase


class HistoryTab(TabBase):
    KEY = "history"

    def build(self, tab) -> None:
        self._tab_header(tab, "History", "Transcript history log")

        # Coming soon card
        card = self._card(tab)
        inner = ctk.CTkFrame(card, fg_color="transparent")
        inner.pack(fill="x", padx=SPACING["xl"], pady=SPACING["2xl"])

        ctk.CTkLabel(
            inner, text="⧖  Coming in Phase 3",
            font=(FONT_FAMILY, FONT_SIZES["lg"], "bold"),
            text_color=self._colors.fg_muted,
        ).pack()

        ctk.CTkLabel(
            inner, text="Your past dictations will be saved and searchable here.",
            font=(FONT_FAMILY, FONT_SIZES["sm"]),
            text_color=self._colors.fg_muted,
        ).pack(pady=(SPACING["xs"], 0))

        # Planned features
        self._section_label(tab, "PLANNED FEATURES")
        card = self._card(tab)
        planned = [
            ("☐", "Transcript Log", "Save all dictations to a local searchable file"),
            ("☐", "Voice Commands", '"New line", "period", "delete that"'),
            ("☐", "Sound Feedback", "Subtle chime on start/stop"),
            ("☐", "Per-App Paste Rules", "Some apps need typing instead of clipboard"),
        ]
        for icon, title, desc in planned:
            row = ctk.CTkFrame(card, fg_color="transparent")
            row.pack(fill="x", padx=SPACING["lg"], pady=SPACING["xs"])
            ctk.CTkLabel(
                row, text=f"{icon}  {title}",
                font=(FONT_FAMILY, FONT_SIZES["sm"]),
                text_color=self._colors.fg_muted, anchor="w",
            ).pack(anchor="w")
            ctk.CTkLabel(
                row, text=f"     {desc}",
                font=(FONT_FAMILY, FONT_SIZES["xs"]),
                text_color=self._colors.fg_muted, anchor="w",
            ).pack(anchor="w")

        self._add_bottom_padding(tab)
