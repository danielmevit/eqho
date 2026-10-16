"""About tab: version details and credits."""

import customtkinter as ctk

from ...theme import SPACING, font
from ...version import __version__
from ..layout import TabBase
from ..widgets import auto_wrap_label, secondary_button


class AboutTab(TabBase):
    KEY = "about"

    def build(self, tab, embedded: bool = False) -> None:
        # `embedded` = rendered as a section inside the Settings view (no own
        # header; the caller provides the section label).
        self._embedded = embedded
        if not embedded:
            self._tab_header(tab, "About Eqho", "Your voice, everywhere.", icon="about")

        cols = self._get_col_count()

        # Side by side only in the wide layout: at two columns the details
        # card is too narrow for its label column plus the values.
        if cols >= 3:
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

        if not embedded:
            self._add_bottom_padding(tab)

    def _build_about_details(self, parent) -> None:
        """About tab: version info and author."""
        if not getattr(self, "_embedded", False):
            self._section_label(parent, "DETAILS")
        card = self._card(parent)
        inner = ctk.CTkFrame(card, fg_color="transparent")
        inner.pack(fill="x", padx=SPACING["lg"], pady=SPACING["lg"])

        details = [
            ("Version", __version__),
            ("Engine", "faster-whisper and whisper.cpp"),
            ("Default Model", "Distil Large v3"),
            ("Runs on", "Your graphics card or the processor"),
            ("Platform", "Windows, macOS and Linux"),
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
            value_lbl = ctk.CTkLabel(
                row, text=value,
                font=font("sm"),
                text_color=self._colors.fg_primary, anchor="w",
            )
            value_lbl.pack(side="left", fill="x", expand=True)
            # Wrap within what's left of the row after the 120 px label column.
            auto_wrap_label(value_lbl, row, padding=128)

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
        author_link.bind("<Button-1>", lambda e: self._open_url("https://github.com/danielmevit"))

        # Website row (damt.xyz — the public-facing brand)
        row = ctk.CTkFrame(inner, fg_color="transparent")
        row.pack(fill="x", pady=2)
        ctk.CTkLabel(
            row, text="Website",
            font=font("sm"),
            text_color=self._colors.fg_secondary, anchor="w",
            width=120,
        ).pack(side="left")
        site_link = ctk.CTkLabel(
            row, text="damt.xyz",
            font=font("sm"),
            text_color=self._colors.accent, anchor="w",
            cursor="hand2",
        )
        site_link.pack(side="left")
        site_link.bind("<Button-1>", lambda e: self._open_url("https://damt.xyz"))

        secondary_button(
            inner, self._colors, text="Run first-time setup again",
            command=self._run_setup_again,
        ).pack(anchor="w", pady=(SPACING["md"], 0))

    def _run_setup_again(self) -> None:
        dash = self.ctx.master()
        if dash is not None and hasattr(dash, "show_onboarding"):
            dash.show_onboarding()

    def _build_about_credits(self, parent) -> None:
        """About tab: powered by section."""
        if not getattr(self, "_embedded", False):
            self._section_label(parent, "POWERED BY")
        card = self._card(parent)
        inner = ctk.CTkFrame(card, fg_color="transparent")
        inner.pack(fill="x", padx=SPACING["lg"], pady=SPACING["sm"])

        techs = [
            "faster-whisper — Fast Whisper inference (MIT)",
            "customtkinter — Modern tkinter widgets",
            "Phosphor Icons — UI iconography (MIT)",
            "pystray — System tray integration",
            "keyboard — Global hotkey capture",
            "pycaw — Windows audio control",
        ]
        for t in techs:
            lbl = ctk.CTkLabel(
                inner, text=f"·  {t}",
                font=font("xs"),
                text_color=self._colors.fg_secondary, anchor="w",
            )
            lbl.pack(anchor="w", fill="x", pady=1)
            auto_wrap_label(lbl, inner, padding=4)

    @staticmethod
    def _open_url(url: str) -> None:
        import webbrowser
        webbrowser.open(url)
