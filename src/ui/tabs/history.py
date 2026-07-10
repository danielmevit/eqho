"""History tab: browse, search, copy, delete, and export past dictations."""

import logging
from datetime import datetime
from pathlib import Path

import customtkinter as ctk

from ...history import TranscriptHistory
from ...theme import SPACING, RADIUS_SM, font
from ..layout import TabBase
from ..widgets import ghost_button, secondary_button

log = logging.getLogger(__name__)

_MAX_SHOWN = 200


class HistoryTab(TabBase):
    KEY = "history"

    def __init__(self, ctx):
        super().__init__(ctx)
        self._history = TranscriptHistory()
        self._search_var = None
        self._list_frame = None
        self._search_job = None

    def build(self, tab) -> None:
        self._tab_header(tab, "History", "Your past dictations — stored locally, searchable")

        # Toolbar: search + export + clear
        bar = ctk.CTkFrame(tab, fg_color="transparent")
        bar.pack(fill="x", padx=SPACING["md"], pady=(0, SPACING["xs"]))

        self._search_var = self._string_var()
        search = ctk.CTkEntry(
            bar, textvariable=self._search_var,
            placeholder_text="Search transcripts…",
            height=30, corner_radius=RADIUS_SM,
            font=font("sm"),
            fg_color=self._colors.bg_tertiary,
            text_color=self._colors.fg_primary,
            placeholder_text_color=self._colors.fg_muted,
            border_width=1, border_color=self._colors.border,
        )
        search.pack(side="left", fill="x", expand=True, padx=(0, SPACING["sm"]))
        search.bind("<KeyRelease>", self._on_search_changed)

        secondary_button(bar, self._colors, text="Export .txt", width=90,
                         command=self._export).pack(side="left", padx=(0, SPACING["xs"]))
        secondary_button(bar, self._colors, text="Clear All", width=80,
                         command=self._clear_all).pack(side="left")

        self._list_frame = ctk.CTkFrame(tab, fg_color="transparent")
        self._list_frame.pack(fill="both", expand=True)
        self._render_entries()
        self._add_bottom_padding(tab)

    # -- Rendering ---------------------------------------------------------------

    def _on_search_changed(self, _event=None) -> None:
        # Debounced so typing doesn't rebuild the list on every keystroke
        if self._search_job is not None:
            try:
                self._list_frame.after_cancel(self._search_job)
            except Exception:
                pass
        self._search_job = self._list_frame.after(250, self._render_entries)

    def _render_entries(self) -> None:
        self._search_job = None
        for child in self._list_frame.winfo_children():
            child.destroy()

        query = self._search_var.get() if self._search_var else ""
        entries = self._history.search(query)

        if not entries:
            card = self._card(self._list_frame)
            message = "No matches." if query.strip() else "No dictations yet — press the hotkey and speak."
            ctk.CTkLabel(
                card, text=message,
                font=font("sm"), text_color=self._colors.fg_muted,
            ).pack(padx=SPACING["md"], pady=SPACING["xl"])
            return

        for entry in entries[:_MAX_SHOWN]:
            self._render_entry(entry)

        if len(entries) > _MAX_SHOWN:
            ctk.CTkLabel(
                self._list_frame,
                text=f"…{len(entries) - _MAX_SHOWN} older entries not shown (search or export to reach them)",
                font=font("xs"), text_color=self._colors.fg_muted,
            ).pack(pady=SPACING["sm"])

    def _render_entry(self, entry: dict) -> None:
        card = self._card(self._list_frame)
        row = ctk.CTkFrame(card, fg_color="transparent")
        row.pack(fill="x", padx=SPACING["md"], pady=SPACING["sm"])

        left = ctk.CTkFrame(row, fg_color="transparent")
        left.pack(side="left", fill="x", expand=True)

        stamp = datetime.fromtimestamp(entry.get("ts", 0)).strftime("%Y-%m-%d %H:%M")
        meta_parts = [stamp, entry.get("model", ""), entry.get("lang", "")]
        meta = "  ·  ".join(p for p in meta_parts if p)
        ctk.CTkLabel(left, text=meta, font=font("xs"),
                     text_color=self._colors.fg_muted, anchor="w").pack(anchor="w")
        ctk.CTkLabel(left, text=entry.get("text", ""), font=font("sm"),
                     text_color=self._colors.fg_primary, anchor="w",
                     wraplength=420, justify="left").pack(anchor="w")

        right = ctk.CTkFrame(row, fg_color="transparent")
        right.pack(side="right")
        ghost_button(right, self._colors, text="Copy", width=52,
                     command=lambda t=entry.get("text", ""): self._copy(t)).pack(side="left", padx=(0, 4))
        ghost_button(right, self._colors, text="✕", width=30,
                     text_color=self._colors.danger,
                     command=lambda ts=entry.get("ts"): self._delete(ts)).pack(side="left")

    # -- Actions -------------------------------------------------------------------

    def _copy(self, text: str) -> None:
        try:
            import pyperclip
            pyperclip.copy(text)
        except Exception as e:
            log.debug("Copy failed: %s", e)

    def _delete(self, ts) -> None:
        self._history.delete(ts)
        self._render_entries()

    def _clear_all(self) -> None:
        from tkinter import messagebox
        if messagebox.askyesno(
            "Eqho", "Delete ALL saved dictations?",
            parent=self._list_frame.winfo_toplevel(),
        ):
            self._history.clear()
            self._render_entries()

    def _export(self) -> None:
        from tkinter import filedialog
        dest = filedialog.asksaveasfilename(
            parent=self._list_frame.winfo_toplevel(),
            defaultextension=".txt",
            filetypes=[("Text file", "*.txt")],
            initialfile="eqho-history.txt",
        )
        if dest:
            count = self._history.export_txt(Path(dest))
            log.info("Exported %d history entries to %s", count, dest)
