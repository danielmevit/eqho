#!/usr/bin/env python3
"""Tiny Tk target window for the end-to-end dictation test (EQ-TESTE2E).

Shows a plain text box Eqho can type into. When told to close — the stopfile
appears or the window is closed — it writes its content to --out and exits.
Standalone (no Eqho imports), standard library + tkinter only.

    python tools/e2e_target.py --out text.txt --stopfile stop.txt
"""

import argparse
from pathlib import Path

import tkinter as tk

POLL_MS = 100


def main() -> None:
    p = argparse.ArgumentParser(description="Eqho e2e typing target window")
    p.add_argument("--out", required=True, help="file to write the text content to on close")
    p.add_argument("--stopfile", required=True, help="close and write --out when this file appears")
    p.add_argument("--title", default="Eqho E2E Target", help="window title")
    args = p.parse_args()

    root = tk.Tk()
    root.title(args.title)
    root.geometry("520x320")
    text = tk.Text(root, font=("Segoe UI", 12), wrap="word")
    text.pack(fill="both", expand=True)
    # Keyboard focus must be IN the text box, not just on the window: when the
    # test brings the window to the front, Ctrl+V otherwise goes to the frame.
    text.focus_set()
    root.bind("<FocusIn>", lambda _e: text.focus_set())
    state = {"done": False}

    def save_and_exit() -> None:
        if state["done"]:
            return
        state["done"] = True
        try:
            Path(args.out).write_text(text.get("1.0", "end-1c"), encoding="utf-8")
        finally:
            root.destroy()

    def poll() -> None:
        if Path(args.stopfile).exists():
            save_and_exit()
            return
        root.after(POLL_MS, poll)

    root.protocol("WM_DELETE_WINDOW", save_and_exit)
    root.after(POLL_MS, poll)
    root.mainloop()


if __name__ == "__main__":
    main()
