"""About tab: version details and credits."""

import customtkinter as ctk

from ...theme import SPACING, font
from ...version import __version__
from ..layout import TabBase


class AboutTab(TabBase):
    KEY = "about"

    def build(self, tab) -> None:
        self._tab_header(tab, "About Eqho", "Your voice, everywhere.")

        cols = self._get_col_count()

        if cols >= 2:
            grid = self._make_grid_container(tab)

            col0 = ctk.CTkFrame(grid, fg_color="transparent")
            col0.grid(row=0, column=0, sticky="nsew", padx=SPACING["xs"])

            self._build_about_details(col0)

            col1 = ctk.CTkFrame(grid, fg_color="transparent")
            col1.grid(row=0, column=1, sticky="nsew", padx=SPACING["xs"])

            self._build_about_credits(col1)
        else:
            self._build_about_details(tab)
            self._build_about_credits(tab)

        self._add_bottom_padding(tab)

    def _build_about_details(self, parent) -> None:
        """About tab: version info and author."""
        self._section_label(parent, "DETAILS")
        card = self._card(parent)
        inner = ctk.CTkFrame(card, fg_color="transparent")
        inner.pack(fill="x", padx=SPACING["lg"], pady=SPACING["lg"])

        details = [
            ("Version", __version__),
            ("Engine", "faster-whisper (CTranslate2)"),
            ("Default Model", "Distil Large v3"),
            ("GPU", "CUDA (NVIDIA) with CPU fallback"),
            ("Platform", "Windows 10/11"),
            ("Font", "Inter (SIL Open Font License)"),
        ]
        for label, value in details:
            row = ctk.CTkFrame(inner, fg_color="transparent")
            row.pack(fill="x", pady=2)
            ctk.CTkLabel(
                row, text=label,
                font=font("sm"),
                text_color=self._colors.fg_secondary, anchor="w",
                width=120,
            ).pack(side="left")
            ctk.CTkLabel(
                row, text=value,
                font=font("sm"),
                text_color=self._colors.fg_primary, anchor="w",
            ).pack(side="left")

        # Author row with clickable GitHub link
        row = ctk.CTkFrame(inner, fg_color="transparent")
        row.pack(fill="x", pady=2)
        ctk.CTkLabel(
            row, text="Author",
            font=font("sm"),
            text_color=self._colors.fg_secondary, anchor="w",
            width=120,
        ).pack(side="left")
        author_link = ctk.CTkLabel(
            row, text="Daniel Mevit",
            font=font("sm"),
            text_color=self._colors.accent, anchor="w",
            cursor="hand2",
        )
        author_link.pack(side="left")
        author_link.bind("<Button-1>", lambda e: self._open_url("https://github.com/DanielMevit"))

    def _build_about_credits(self, parent) -> None:
        """About tab: powered by section."""
        self._section_label(parent, "POWERED BY")
        card = self._card(parent)
        inner = ctk.CTkFrame(card, fg_color="transparent")
        inner.pack(fill="x", padx=SPACING["lg"], pady=SPACING["sm"])

        techs = [
            "faster-whisper — Fast Whisper inference (MIT)",
            "customtkinter — Modern tkinter widgets",
            "pystray — System tray integration",
            "keyboard — Global hotkey capture",
            "pycaw — Windows audio control",
        ]
        for t in techs:
            ctk.CTkLabel(
                inner, text=f"·  {t}",
                font=font("xs"),
                text_color=self._colors.fg_secondary, anchor="w",
            ).pack(anchor="w", pady=1)

    @staticmethod
    def _open_url(url: str) -> None:
        import webbrowser
        webbrowser.open(url)
