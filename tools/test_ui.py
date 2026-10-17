"""Eqho UI Interactive Test (EQ-TESTUI).

Exercises every screen, tab, and interactive widget in the CustomTkinter
dashboard in-process with settings and history isolated in a temporary folder.

Key properties:
1. In-process: builds the dashboard window directly (no model inference,
   no background child processes, no global hotkey capture, no system tray).
2. Visits every tab ('general', 'models', 'history', 'settings', including
   embedded Overlay and About sections).
3. Discovers all buttons, switches/checkboxes, dropdowns, segmented buttons,
   sliders, and text entries in the widget tree, exercises each, and pumps
   the Tk event loop.
4. Catches and fails on any Tk callback exceptions via report_callback_exception.
5. Enforces a safety SKIP list (never triggers quit/exit, history deletion,
   dialogs/URLs, Windows autostart, model downloads, or hotkey capture).
6. Captures screenshots of each tab to `<work>/ui-<tab>.png`.
6b. Opens the first-time setup screen and walks every page (mic stream and
    model download stubbed) with one screenshot per page:
    `<work>/ui-setup-<n>-<page>.png`.
6c. Puts the app into every conditional state (download running/failed,
    hotkey capture, empty/no-results/long history, rare setup pages, the
    dictation overlay) -> `<work>/ui-state-<name>.png`, then repeats the
    tabs, setup pages and states in the light theme (`*-light.png`).
7. Prints a comprehensive summary and exits 1 on any error, 0 on success.

Usage:
    python3 tools/test_ui.py --work _test/ui
    "/mnt/d/Vibe Coding/Eqho/venv/Scripts/python.exe" tools/test_ui.py --work _test/ui
"""

import argparse
import json
import logging
import os
import sys
import tempfile
import time
import traceback
from pathlib import Path
from typing import Any, Callable, Dict, List, Optional, Set, Tuple

# Ensure repo root is on sys.path
REPO_ROOT = Path(__file__).resolve().parents[1]
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

log = logging.getLogger("test_ui")

# Safety skip rules: actions that must NEVER be triggered
SKIP_RULES = [
    ("Quit / Exit", "quit, exit, or window destroy actions"),
    ("Delete / Clear History", "Clear All button and per-entry delete (✕) in History"),
    ("Dialogs / URLs / Modal Windows", "Open Folder, Export .txt, Text Replacements / Paste Rules modal editors, website/author links"),
    ("Start with Windows", "HKCU Run / autostart toggle in Settings"),
    ("Model Download / Delete", "Download button (↓) in Models tab"),
    ("Writes Outside Temp Folder", "Settings/history paths redirected to isolated temp folder"),
    ("Run Setup Again", "About's 'Run first-time setup again' (walked separately as the setup screen)"),
    ("Interactive Hotkey Capture", "Global hotkey button in General tab (starts background keyboard hook)"),
    ("Disabled Controls", "Buttons or controls with state == 'disabled'"),
]


def print_skip_list() -> None:
    """Print the configured safety skip list to stdout."""
    print("=" * 64)
    print("SAFETY SKIP LIST (Guaranteed never to be triggered):")
    for category, desc in SKIP_RULES:
        print(f"  - [{category}]: {desc}")
    print("=" * 64)


class UITestRunner:
    """Orchestrates in-process UI testing of the CustomTkinter dashboard."""

    def __init__(self, work_dir: Path, delay: float = 0.04) -> None:
        self.work_dir = work_dir.resolve()
        self.work_dir.mkdir(parents=True, exist_ok=True)
        self.delay = delay

        self.tabs_visited: List[str] = []
        self.controls_exercised: int = 0
        self.controls_skipped: int = 0
        self.errors: List[str] = []
        self.callback_errors: List[str] = []
        self.screenshots: List[Path] = []

        self._temp_dir: Optional[tempfile.TemporaryDirectory] = None
        self._temp_path: Optional[Path] = None
        self.root: Optional[Any] = None
        self.dash: Optional[Any] = None

    def _setup_isolation(self) -> None:
        """Point settings and history at an isolated temporary folder."""
        self._temp_dir = tempfile.TemporaryDirectory(prefix="eqho_ui_test_")
        self._temp_path = Path(self._temp_dir.name)
        # Point ALL of Eqho's config (settings, history, log) at the temp folder
        # before src is imported — module-level paths and default arguments are
        # bound at import time (TranscriptHistory(path=HISTORY_FILE) showed the
        # owner's REAL history in screenshots before this).
        os.environ["EQHO_CONFIG_DIR"] = str(self._temp_path)

        import src.settings as settings_mod
        import src.history as history_mod

        settings_mod.CONFIG_FILE = self._temp_path / "settings.json"
        settings_mod.CONFIG_DIR = self._temp_path
        settings_mod.LEGACY_MODEL_DIR = self._temp_path / "no-legacy-model-cache"
        history_mod.HISTORY_FILE = self._temp_path / "history.jsonl"
        # Belt and braces if src.history was imported earlier: the class's
        # default path argument was bound then.
        defaults = list(history_mod.TranscriptHistory.__init__.__defaults__ or ())
        if defaults:
            defaults[0] = history_mod.HISTORY_FILE
            history_mod.TranscriptHistory.__init__.__defaults__ = tuple(defaults)

        # Pre-seed history entries so HistoryTab renders cards and controls
        seed_entries = [
            {
                "ts": time.time() - 7200,
                "text": "First test transcription recorded for automated UI verification.",
                "duration": 3.2,
                "model": "distil-large-v3",
                "lang": "en",
            },
            {
                "ts": time.time() - 3600,
                "text": "Second dictation entry to test history search and clipboard copy.",
                "duration": 1.8,
                "model": "distil-large-v3",
                "lang": "en",
            },
        ]
        with history_mod.HISTORY_FILE.open("w", encoding="utf-8") as f:
            for entry in seed_entries:
                f.write(json.dumps(entry) + "\n")

        # Pre-seed a dummy cached model folder in temp model dir
        models_dir = self._temp_path / "models"
        models_dir.mkdir(parents=True, exist_ok=True)
        (models_dir / "distil-large-v3").mkdir(parents=True, exist_ok=True)
        (models_dir / "distil-medium.en").mkdir(parents=True, exist_ok=True)

        self.settings = settings_mod.Settings.load()
        self.settings.model_dir = str(models_dir)
        self.settings.model_size = "distil-large-v3"
        self.settings.theme = "system"
        self.settings.save()

    def _on_tk_callback_exception(self, exc: Any, val: Any, tb: Any) -> None:
        """Capture exceptions originating from Tkinter event callbacks."""
        err_msg = "".join(traceback.format_exception(exc, val, tb))
        self.callback_errors.append(err_msg)
        print(f"\n[TK CALLBACK EXCEPTION]:\n{err_msg}", file=sys.stderr)

    def _pump(self, count: int = 4, step_delay: Optional[float] = None) -> None:
        """Pump the Tk event loop to execute idle tasks and pending events."""
        if step_delay is None:
            step_delay = self.delay
        for _ in range(max(1, count)):
            if self.root is not None:
                try:
                    self.root.update_idletasks()
                    self.root.update()
                except Exception as e:
                    self.callback_errors.append(f"Event pump error: {e}")
            if step_delay > 0:
                time.sleep(step_delay)

        # Check and collect any callback errors caught during pump
        if self.callback_errors:
            for err in self.callback_errors:
                self.errors.append(f"Tk Callback Error: {err.strip().splitlines()[-1] if err.strip() else 'unknown'}")
            self.callback_errors.clear()

    def _build_dashboard(self) -> None:
        """Instantiate the CustomTkinter root and Dashboard window."""
        import customtkinter as ctk
        from src.fonts import load_fonts
        from src.ui.dashboard import Dashboard

        load_fonts()  # like App.__init__ (src/main.py): Inter + the Phosphor icon font
        self.root = ctk.CTk()
        self.root.withdraw()
        self.root.report_callback_exception = self._on_tk_callback_exception

        def _mock_settings_changed(reload_model: bool = False) -> None:
            # Mock callback: avoids loading models or touching external audio/hotkeys
            pass

        self.dash = Dashboard(
            settings=self.settings,
            on_settings_changed=_mock_settings_changed,
            parent=self.root,
            initial_tab="general",
        )
        self.dash.report_callback_exception = self._on_tk_callback_exception
        self._pump(count=10, step_delay=0.03)

    def _capture_tab_screenshot(self, tab_name: str) -> Optional[Path]:
        """Capture screenshot of the dashboard window to <work>/ui-<tab>.png."""
        self._pump(count=6, step_delay=0.02)
        target_path = self.work_dir / f"ui-{tab_name}.png"

        try:
            from PIL import ImageGrab

            x = self.dash.winfo_rootx()
            y = self.dash.winfo_rooty()
            w = self.dash.winfo_width()
            h = self.dash.winfo_height()

            if w > 10 and h > 10:
                bbox = (x, y, x + w, y + h)
                img = ImageGrab.grab(bbox=bbox)
                img.save(target_path)
                self.screenshots.append(target_path)
                print(f"  [screenshot] Captured '{tab_name}' -> {target_path.name} ({w}x{h})")
                return target_path
            else:
                print(f"  [screenshot] Warning: Window dimensions invalid ({w}x{h}), skipping grab.")
        except Exception as e:
            print(f"  [screenshot] Warning: ImageGrab unavailable or failed: {e}")

        return None

    def _find_controls(self, container: Any) -> List[Any]:
        """Recursively discover user-interactive controls in the widget tree."""
        import customtkinter as ctk
        import tkinter as tk
        from src.ui.widgets import ThemedDropdown

        controls: List[Any] = []

        def _walk(w: Any) -> None:
            try:
                if not w.winfo_exists():
                    return
            except Exception:
                return

            # Check recognizable high-level controls; do not recurse into their internal components
            if isinstance(w, ThemedDropdown):
                controls.append(w)
                return
            elif isinstance(w, ctk.CTkSegmentedButton):
                controls.append(w)
                return
            elif isinstance(w, ctk.CTkSwitch):
                controls.append(w)
                return
            elif isinstance(w, ctk.CTkCheckBox):
                controls.append(w)
                return
            elif isinstance(w, ctk.CTkButton):
                controls.append(w)
                return
            elif isinstance(w, ctk.CTkSlider):
                controls.append(w)
                return
            elif isinstance(w, ctk.CTkEntry):
                controls.append(w)
                return
            elif isinstance(w, ctk.CTkTextbox):
                controls.append(w)
                return
            elif isinstance(w, (tk.Button, tk.Entry, tk.Text)):
                controls.append(w)
                return

            # For container frames, inspect children
            try:
                children = w.winfo_children()
            except Exception:
                children = []

            for child in children:
                _walk(child)

        _walk(container)
        return controls

    def _should_skip(self, widget: Any, tab_name: str) -> Tuple[bool, str]:
        """Determine if a widget matches the safety SKIP list."""
        import customtkinter as ctk
        import tkinter as tk

        # 1. Check disabled state
        try:
            state = widget.cget("state")
            if state in ("disabled", tk.DISABLED):
                return True, "Control is disabled"
        except Exception:
            pass

        # 2. Extract widget identifiers (text, command name, bound variables)
        text = ""
        try:
            text = str(widget.cget("text") or "").strip()
        except Exception:
            text = str(getattr(widget, "_text", "") or "").strip()

        cmd = getattr(widget, "_command", None)
        cmd_str = str(cmd).lower() if cmd else ""
        cmd_name = getattr(cmd, "__name__", "").lower() if cmd else ""

        if text == "Run first-time setup again":
            return True, "Setup screen is walked separately"

        # 3. Quit / exit
        if any(q in text.lower() for q in ("quit", "exit", "close")):
            return True, "Quit/Exit action"

        # 4. History tab deletion & exports
        if tab_name == "history":
            if text == "Clear All" or "clear_all" in cmd_str or "clear_all" in cmd_name:
                return True, "Clear All history action"
            if text == "✕" or "delete" in cmd_str or "delete" in cmd_name:
                return True, "Delete history entry action"
            if "Export" in text or "export" in cmd_str or "export" in cmd_name:
                return True, "Export file dialog action"

        # 5. Models tab downloads & folder opening
        if tab_name == "models":
            if text == "↓" or "download" in cmd_str or "download" in cmd_name:
                return True, "Model download action"
            if "Open Folder" in text or "open_folder" in cmd_str or "open_folder" in cmd_name:
                return True, "Open storage folder dialog action"

        # 6. General tab global hotkey capture
        if tab_name == "general":
            general_tab = self.dash._tabs.get("general")
            if general_tab and widget is getattr(general_tab, "_hotkey_display", None):
                return True, "Interactive global hotkey capture action"
            if "hotkey" in cmd_str or "hotkey" in cmd_name:
                return True, "Hotkey capture action"

        # 7. Settings tab modal editors & Windows autostart
        if tab_name == "settings":
            settings_tab = self.dash._tabs.get("settings")
            if text.startswith("Edit…") or "rules_editor" in cmd_str or "editor" in cmd_str:
                return True, "Modal rules/replacements editor dialog action"
            if settings_tab and (
                getattr(widget, "_variable", None) is getattr(settings_tab, "_startup_var", None)
                or getattr(widget, "_command", None) == getattr(settings_tab, "_on_startup_changed", None)
            ):
                return True, "Start with Windows autostart toggle"
            if "startup" in cmd_str or "startup" in cmd_name or "autostart" in cmd_str:
                return True, "Start with Windows autostart toggle"

        return False, ""

    def _exercise_single_control(self, widget: Any, tab_name: str) -> None:
        """Safely exercise one discovered control and pump the event loop."""
        import customtkinter as ctk
        from src.ui.widgets import ThemedDropdown

        if not widget.winfo_exists():
            return

        cls_name = widget.__class__.__name__

        # Handle ThemedDropdown
        if isinstance(widget, ThemedDropdown):
            values = [v for v in getattr(widget, "_values", []) if not v.startswith("— ")]
            if not values:
                return
            orig_val = widget._variable.get() if getattr(widget, "_variable", None) else None
            # Select each option to exercise callbacks
            for opt in values:
                widget._select_value(opt)
                self._pump(count=2)
            # Restore original value
            if orig_val and orig_val in values:
                widget._select_value(orig_val)
                self._pump(count=2)
            self.controls_exercised += 1
            print(f"    [OK] Dropdown: cycled {len(values)} options (restored to '{orig_val or values[0]}')")
            return

        # Handle CTkSwitch
        if isinstance(widget, ctk.CTkSwitch):
            orig_state = widget.get()
            widget.toggle()
            self._pump(count=2)
            mid_state = widget.get()
            widget.toggle()  # toggle back to preserve state
            self._pump(count=2)
            self.controls_exercised += 1
            if mid_state == orig_state or widget.get() != orig_state:
                raise RuntimeError(f"switch did not flip: {orig_state} -> {mid_state} -> {widget.get()}")
            print(f"    [OK] Switch: toggled {orig_state} -> {mid_state} -> {widget.get()}")
            return

        # Handle CTkCheckBox
        if isinstance(widget, ctk.CTkCheckBox):
            orig_state = widget.get()
            widget.toggle()
            self._pump(count=2)
            mid_state = widget.get()
            widget.toggle()
            self._pump(count=2)
            self.controls_exercised += 1
            if mid_state == orig_state or widget.get() != orig_state:
                raise RuntimeError(f"checkbox did not flip: {orig_state} -> {mid_state} -> {widget.get()}")
            print(f"    [OK] CheckBox: toggled {orig_state} -> {mid_state} -> {widget.get()}")
            return

        # Handle CTkSegmentedButton (non-rebuilding segments)
        if isinstance(widget, ctk.CTkSegmentedButton):
            values = list(getattr(widget, "_value_list", []))
            orig_val = widget.get()
            for val in values:
                try:
                    widget.set(val, from_button_callback=True)
                except TypeError:
                    widget.set(val)
                    if callable(getattr(widget, "_command", None)):
                        widget._command(val)
                self._pump(count=2)
            if orig_val in values:
                try:
                    widget.set(orig_val, from_button_callback=True)
                except TypeError:
                    widget.set(orig_val)
                    if callable(getattr(widget, "_command", None)):
                        widget._command(orig_val)
                self._pump(count=2)
            self.controls_exercised += 1
            print(f"    [OK] SegmentedButton: cycled {values}")
            return

        # Handle CTkSlider
        if isinstance(widget, ctk.CTkSlider):
            orig_val = widget.get()
            from_ = getattr(widget, "_from_", 0.0)
            to = getattr(widget, "_to", 1.0)
            test_val = round((from_ + to) / 2.0, 2)
            widget.set(test_val)
            if callable(getattr(widget, "_command", None)):
                widget._command(test_val)
            self._pump(count=2)
            widget.set(orig_val)
            if callable(getattr(widget, "_command", None)):
                widget._command(orig_val)
            self._pump(count=2)
            self.controls_exercised += 1
            print(f"    [OK] Slider: exercised range [{from_}, {to}], restored {orig_val}")
            return

        # Handle CTkButton
        if isinstance(widget, ctk.CTkButton):
            btn_text = ""
            try:
                btn_text = str(widget.cget("text") or "").strip()
            except Exception:
                btn_text = str(getattr(widget, "_text", "") or "").strip()

            widget.invoke()
            self._pump(count=2)
            self.controls_exercised += 1
            print(f"    [OK] Button: invoked '{btn_text or cls_name}'")
            return

        # Handle CTkEntry
        if isinstance(widget, ctk.CTkEntry):
            widget.delete(0, "end")
            widget.insert(0, "test query")
            try:
                widget._entry.event_generate("<KeyRelease>")
            except Exception:
                pass
            self._pump(count=3)
            widget.delete(0, "end")
            try:
                widget._entry.event_generate("<KeyRelease>")
            except Exception:
                pass
            self._pump(count=3)
            self.controls_exercised += 1
            print("    [OK] Entry: typed search query and cleared")
            return

        # Handle CTkTextbox
        if isinstance(widget, ctk.CTkTextbox):
            widget.insert("end", "\nEqho UI Automated Test")
            try:
                widget._textbox.event_generate("<FocusOut>")
            except Exception:
                pass
            self._pump(count=2)
            self.controls_exercised += 1
            print("    [OK] Textbox: appended test text and fired FocusOut")
            return

    def _exercise_rebuilding_settings(self) -> None:
        """Safely exercise Theme switching and UI Zoom scaling in Settings tab.

        Theme and UI Zoom call _rebuild_ui(), which recreates the topbar and
        tab frame. Exercising them cleanly in isolation avoids accessing
        destroyed widget references.
        """
        import customtkinter as ctk

        print("  [rebuild-controls] Exercising Theme switching...")
        settings_tab = self.dash._tabs.get("settings")
        if settings_tab and hasattr(settings_tab, "_theme_var"):
            theme_modes = ["Dark", "Light", "System"]
            orig_mode = settings_tab._theme_var.get()
            for mode in theme_modes:
                print(f"    -> Applying appearance mode: {mode}")
                settings_tab._on_theme_changed(mode)
                self._pump(count=8, step_delay=0.03)
                self.controls_exercised += 1
            if orig_mode:
                settings_tab._on_theme_changed(orig_mode)
                self._pump(count=8, step_delay=0.03)

        print("  [rebuild-controls] Exercising UI Zoom scaling...")
        settings_tab = self.dash._tabs.get("settings")
        if settings_tab and hasattr(settings_tab, "_zoom_var"):
            zoom_levels = ["125%", "150%"]
            orig_zoom = settings_tab._zoom_var.get()
            for zoom in zoom_levels:
                print(f"    -> Applying UI Zoom level: {zoom}")
                settings_tab._on_zoom_changed(zoom)
                self._pump(count=8, step_delay=0.03)
                self.controls_exercised += 1
            if orig_zoom:
                settings_tab._on_zoom_changed(orig_zoom)
                self._pump(count=8, step_delay=0.03)

    def _prewarm_alignment_pass(self) -> None:
        """Tabs built by the background prewarm must lay out like click-built ones:
        on Models and General the section labels' left edge equals the first card's (+-2 px)."""
        import customtkinter as ctk
        d = self.dash
        t0 = time.perf_counter()
        while time.perf_counter() - t0 < 20:
            cols = d._get_col_count()
            if not d._warming and not getattr(d, "_resize_job", None) and all(
                    d._tab_built_cols.get(k) == cols for k in ("models", "history", "settings")):
                break
            self._pump(count=1, step_delay=0.005)
        print("\nPrewarm alignment check (models prewarmed, general rebuilt after):")
        for tab in ("models", "general"):
            if tab == "general":
                d._show_tab("models")
                d.rebuild_tab("general")   # general was click-built; rebuild it hidden ...
                d._tab_built_cols["general"] = -1
                d._schedule_prewarm()      # ... and let the prewarm build it instead
                t0 = time.perf_counter()
                while (d._tab_built_cols.get("general") != d._get_col_count() or d._warming) \
                        and time.perf_counter() - t0 < 20:
                    self._pump(count=1, step_delay=0.005)
            d._show_tab(tab)
            self._pump(count=10, step_delay=0.03)
            def _is_card(w: Any) -> bool:
                return isinstance(w, ctk.CTkFrame) and w.cget("fg_color") != "transparent"

            def _first_card(w: Any) -> Any:
                if _is_card(w):
                    return w
                for c in w.winfo_children():
                    if c.winfo_ismapped() and isinstance(c, ctk.CTkFrame):
                        found = _first_card(c)
                        if found is not None:
                            return found
                return None

            pairs = []  # (section label, first card below it), outside any card

            def _walk(w: Any) -> None:
                kids = [c for c in w.winfo_children() if c.winfo_ismapped()]
                if w.winfo_manager() != "grid":
                    kids.sort(key=lambda c: c.winfo_rooty())
                for i, c in enumerate(kids):
                    if isinstance(c, ctk.CTkLabel) and c.winfo_manager() == "pack" \
                            and str(c.cget("text")).strip():
                        below = next((_first_card(k) for k in kids[i + 1:]
                                      if isinstance(k, ctk.CTkFrame) and _first_card(k) is not None), None)
                        if below is not None:
                            pairs.append((c, below))
                    elif isinstance(c, ctk.CTkFrame) and not _is_card(c):
                        _walk(c)

            _walk(d._tab_frames[tab])
            if not pairs:
                self.errors.append(f"alignment[{tab}]: no section label above a card found")
                continue
            for lab, card in pairs:
                lx, cx = lab.winfo_rootx(), card.winfo_rootx()
                text = str(lab.cget("text"))[:30]
                print(f"  {tab}: '{text}' x={lx}, card below x={cx}")
                if abs(lx - cx) > 2:
                    self.errors.append(f"alignment[{tab}]: label '{text}' x={lx} vs card below x={cx}")

    def _check_cutoff(self, tab_name: str, where: str, root: Any = None) -> None:
        """Fail on any visible CTkLabel whose text does not fit the width it got.

        A freshly built tab can still be mid-layout, so offenders are only
        reported if they persist after the event loop has settled.
        """
        for attempt in range(3):
            found = self._find_cutoff(tab_name, where, report=(attempt == 2), root=root)
            if not found:
                return
            self._pump(count=20, step_delay=0.05)

    def _find_cutoff(self, tab_name: str, where: str, report: bool, root: Any = None) -> int:
        import customtkinter as ctk

        frame = root if root is not None else self.dash._tab_frames.get(tab_name)
        if not frame:
            return 0
        self._pump(count=6, step_delay=0.02)
        offenders = 0
        checked = 0

        def _walk(w: Any) -> None:
            nonlocal offenders, checked
            if isinstance(w, ctk.CTkLabel):
                inner = getattr(w, "_label", None)
                try:
                    text = str(w.cget("text") or "")
                    visible = inner is not None and inner.winfo_ismapped()
                except Exception:
                    text, visible = "", False
                if text.strip() and visible:
                    checked += 1
                    req, got = inner.winfo_reqwidth(), inner.winfo_width()
                    parent = w.master
                    right = w.winfo_rootx() + w.winfo_width()
                    p_right = parent.winfo_rootx() + parent.winfo_width()
                    if req > got + 2 or right > p_right + 2:
                        offenders += 1
                        msg = (f"Cut-off text on tab '{tab_name}' ({where}): {text!r} "
                               f"needs {req}px, got {got}px (right edge {right} > {p_right})"
                               if right > p_right + 2 else
                               f"Cut-off text on tab '{tab_name}' ({where}): {text!r} "
                               f"needs {req}px, got {got}px")
                        if report:
                            print(f"    [CUT-OFF] {msg}")
                            self.errors.append(msg)
            for child in w.winfo_children():
                _walk(child)

        _walk(frame)
        if report or not offenders:
            print(f"  [cut-off] {where}: {checked} labels checked, {offenders} cut off")
        return offenders

    def run(self) -> int:
        """Execute the full UI test suite."""
        print_skip_list()
        print("\nStarting Eqho UI Interactive Test...\n")

        try:
            self._setup_isolation()
            self._build_dashboard()
            self._check_fits_screen("start size")
            self._prewarm_alignment_pass()

            # The 4 primary dashboard tabs
            tabs_to_test = ["general", "models", "history", "settings"]

            for tab_name in tabs_to_test:
                print(f"\n==================================================")
                print(f"Visiting Tab: '{tab_name.upper()}'")
                print(f"==================================================")

                self.dash._show_tab(tab_name)
                self._pump(count=6, step_delay=0.03)
                self.tabs_visited.append(tab_name)

                # Capture clean screenshot of the tab
                self._capture_tab_screenshot(tab_name)
                self._check_cutoff(tab_name, f"{self.dash.winfo_width()}px")

                # Discover and exercise tab controls
                tab_frame = self.dash._tab_frames.get(tab_name)
                if not tab_frame:
                    self.errors.append(f"Tab frame '{tab_name}' not found after _show_tab")
                    continue

                controls = self._find_controls(tab_frame)
                print(f"  Discovered {len(controls)} controls in widget tree.")

                for ctrl in controls:
                    # In Settings tab, postpone theme/zoom segmented controls to avoid rebuilding under loop
                    if tab_name == "settings":
                        settings_tab = self.dash._tabs.get("settings")
                        if settings_tab:
                            theme_segmented = getattr(settings_tab, "_theme_var", None)
                            zoom_segmented = getattr(settings_tab, "_zoom_var", None)
                            ctrl_var = getattr(ctrl, "_variable", None)
                            if ctrl_var in (theme_segmented, zoom_segmented):
                                continue

                    should_skip, reason = self._should_skip(ctrl, tab_name)
                    if should_skip:
                        self.controls_skipped += 1
                        widget_desc = ctrl.cget("text") if hasattr(ctrl, "cget") else ctrl.__class__.__name__
                        print(f"    [SKIP] {ctrl.__class__.__name__} ('{widget_desc}'): {reason}")
                        continue

                    try:
                        self._exercise_single_control(ctrl, tab_name)
                    except Exception as e:
                        err_text = f"Error exercising {ctrl.__class__.__name__} on tab '{tab_name}': {e}"
                        print(f"    [ERROR] {err_text}", file=sys.stderr)
                        self.errors.append(err_text)

                # If on settings tab, exercise theme and zoom rebuild controls safely
                if tab_name == "settings":
                    self._exercise_rebuilding_settings()

            self._langwarn_pass()
            self._selvocab_pass()

            # Re-check every tab at the MINIMUM window width (logical px, so
            # the same layout is tested at every Windows scaling)
            self._narrow_pass("zoom as saved")
            self._walk_onboarding()

            # Conditional states ("only sometimes" screens), then everything
            # again in the light theme
            self._states_pass()
            self._overlay_pass()
            self._overlay_variants_pass()
            self._light_pass()

            # Second pass at Eqho zoom 1.5: on a 125 % Windows PC that is the
            # 14" laptop's 1.875x total (1920x1080 at 150 %, zoom 1.25)
            print(f"\n==================================================")
            print("Second pass at dashboard zoom 1.5...")
            print(f"==================================================")
            orig_zoom = self.settings.ui_scale
            self.dash.set_ui_scale(1.5)
            self._pump(count=10, step_delay=0.03)
            self._check_fits_screen("zoom 1.5, start size")
            for tab_name in tabs_to_test:
                self.dash._show_tab(tab_name)
                self._pump(count=6, step_delay=0.03)
                self._check_cutoff(tab_name, f"zoom 1.5, {self.dash.winfo_width()}px")
                self._capture_tab_screenshot(f"{tab_name}-zoom150")
            self._narrow_pass("zoom 1.5")
            self._walk_onboarding(tag="zoom150")
            self.dash.set_ui_scale(orig_zoom)
            self._pump(count=10, step_delay=0.03)

            # Test top bar actions
            print(f"\n==================================================")
            print("Exercising Top Bar Controls...")
            print(f"==================================================")
            if hasattr(self.dash, "_theme_btn") and self.dash._theme_btn.winfo_exists():
                print("  Invoking top bar theme toggle button...")
                self.dash._theme_btn.invoke()
                self._pump(count=6, step_delay=0.03)
                self.dash._theme_btn.invoke()  # toggle back
                self._pump(count=6, step_delay=0.03)
                self.controls_exercised += 1
                print("    [OK] Top bar theme button toggle: OK")

        except Exception as e:
            err_msg = f"Fatal UI Test Exception: {e}\n{traceback.format_exc()}"
            print(f"\n[FATAL ERROR]: {err_msg}", file=sys.stderr)
            self.errors.append(err_msg)
        finally:
            self._teardown()

        return self._print_summary()

    def _langwarn_pass(self) -> None:
        """General tab with a non-English language on an English-only model:
        the warning under the language picker must show and not be cut."""
        print("\nLanguage warning: German on an English-only model...")
        orig = (self.settings.language, self.settings.model_size)
        try:
            self.settings.language, self.settings.model_size = "de", "distil-small.en"
            self.dash._show_tab("general")
            self._pump(count=4, step_delay=0.03)
            # Look the tab up AFTER showing it: the zoom pass rebuilds tabs lazily.
            gen = self.dash._tabs.get("general")
            gen._update_lang_warning()
            self._pump(count=6, step_delay=0.03)
            if not gen._lang_warn.winfo_ismapped():
                self.errors.append("General: English-only warning not shown for German")
            self._check_cutoff("general", f"langwarn, {self.dash.winfo_width()}px")
            self._capture_tab_screenshot("general-langwarn")
        finally:
            self.settings.language, self.settings.model_size = orig
            if gen:
                gen._update_lang_warning()
            self._pump(count=4)

    def _selvocab_pass(self) -> None:
        """EQ-SELVOCAB: (a) Selecting a model via its Select button moves the
        card highlight immediately (2 px accent border on the new card, 1 px on
        the old one); (b) the empty Custom Vocabulary box shows a grey hint
        that clears on focus, restores on blur, and is NEVER saved into
        settings.initial_prompt."""
        print("\nModel selection highlight + vocabulary hint checks...")
        try:
            # -- (a) Models tab: in-place highlight swap ------------------------
            self.dash._show_tab("models")
            self._pump(count=6, step_delay=0.03)
            models_tab = self.dash._tabs.get("models")
            old_key = self.settings.model_size
            new_key = next(k for k in ("distil-large-v3", "distil-medium.en")
                           if k != old_key and models_tab._is_model_cached(k))
            old_refs = models_tab._card_refs[old_key]
            new_refs = models_tab._card_refs[new_key]

            new_refs["select"].invoke()
            self._pump(count=4, step_delay=0.03)

            if int(new_refs["card"].cget("border_width")) != 2:
                self.errors.append(f"Models: '{new_key}' card not highlighted (2 px) after Select")
            if str(new_refs["card"].cget("border_color")) != str(models_tab._colors.accent):
                self.errors.append(f"Models: '{new_key}' card border not accent after Select")
            if not str(new_refs["name"].cget("text")).endswith("  ●"):
                self.errors.append(f"Models: '{new_key}' title missing the selected dot")
            if new_refs["actions"].winfo_ismapped():
                self.errors.append(f"Models: '{new_key}' actions row still visible when selected")
            if int(old_refs["card"].cget("border_width")) != 1:
                self.errors.append(f"Models: '{old_key}' card still highlighted (2 px) after switch")
            if str(old_refs["card"].cget("border_color")) != str(models_tab._colors.border_subtle):
                self.errors.append(f"Models: '{old_key}' card border not subtle after switch")
            if str(old_refs["name"].cget("text")).endswith("  ●"):
                self.errors.append(f"Models: '{old_key}' title kept the selected dot")
            if not old_refs["actions"].winfo_ismapped():
                self.errors.append(f"Models: '{old_key}' actions row not visible after deselect")
            if self.settings.model_size != new_key:
                self.errors.append("Models: Select did not save the new model_size")
            self._capture_tab_screenshot("models-selected")

            # Restore the original selection the same way (via Select button)
            old_refs["select"].invoke()
            self._pump(count=4, step_delay=0.03)
            if self.settings.model_size != old_key:
                self.errors.append("Models: could not restore the original model selection")

            # -- (b) General tab: vocabulary hint, never saved ------------------
            from src.ui.tabs.general import VOCAB_HINT

            self.settings.initial_prompt = ""
            self.settings.save()
            self.dash._show_tab("general")
            self._pump(count=4, step_delay=0.03)
            self.dash.rebuild_tab("general")  # deterministic: box built empty
            self._pump(count=6, step_delay=0.03)
            gen = self.dash._tabs.get("general")
            box = gen._vocab_box

            if not gen._vocab_hint_active:
                self.errors.append("General: vocabulary hint not shown on empty box")
            if str(box.get("1.0", "end").strip()) != VOCAB_HINT:
                self.errors.append("General: vocabulary hint text wrong or missing")
            if str(box.cget("text_color")) != str(gen._colors.fg_muted):
                self.errors.append("General: vocabulary hint not in the muted grey color")

            box._textbox.event_generate("<FocusIn>")
            self._pump(count=2)
            if gen._vocab_hint_active or str(box.get("1.0", "end").strip()):
                self.errors.append("General: vocabulary hint not cleared on focus")

            box._textbox.event_generate("<FocusOut>")
            self._pump(count=2)
            if not gen._vocab_hint_active or str(box.get("1.0", "end").strip()) != VOCAB_HINT:
                self.errors.append("General: vocabulary hint not restored on blur when empty")
            if self.settings.initial_prompt != "":
                self.errors.append("General: vocabulary hint leaked into settings.initial_prompt")
            self._capture_tab_screenshot("general-vocabhint")
        except Exception as e:
            msg = f"Selvocab pass failed: {e}"
            print(f"    [ERROR] {msg}\n{traceback.format_exc()}", file=sys.stderr)
            self.errors.append(msg)
        finally:
            self._pump(count=4)

    # -- Conditional states (EQ-STATES) ------------------------------------------

    def _state(self, tab: str, name: str, sfx: str, root: Any = None) -> None:
        """Cut-off check + screenshot `ui-state-<name><sfx>.png` for one state."""
        self._pump(count=8, step_delay=0.03)
        where = f"state '{name}{sfx}', {self.dash.winfo_width()}px"
        self._check_cutoff(tab, where, root=root)
        self._capture_tab_screenshot(f"state-{name}{sfx}")

    def _run_state(self, label: str, fn: Callable[[str], None], sfx: str) -> None:
        print(f"\n  State group: {label}{sfx}")
        try:
            fn(sfx)
        except Exception as e:
            msg = f"State '{label}{sfx}' failed: {e}"
            print(f"    [ERROR] {msg}\n{traceback.format_exc()}", file=sys.stderr)
            self.errors.append(msg)
        self._pump(count=4)

    def _states_pass(self, sfx: str = "") -> None:
        """Every conditional state a user can meet, at the current size/zoom."""
        print(f"\n==================================================")
        print(f"Conditional states{(' (' + sfx.strip('-') + ')') if sfx else ''}...")
        print(f"==================================================")
        self._run_state("models download", self._state_models, sfx)
        self._run_state("hotkey capture", self._state_hotkey, sfx)
        self._run_state("history", self._state_history, sfx)
        self._run_state("setup", self._state_setup, sfx)

    def _state_models(self, sfx: str) -> None:
        """A card mid-download (40 %) and the same card after a failed download."""
        from src import modelstore
        import src.ui.tabs.models as models_mod

        self.dash._show_tab("models")
        self._pump(count=6, step_delay=0.03)
        tab = self.dash._tabs["models"]
        key = next(k for k in models_mod._EN_MODELS + models_mod._ML_MODELS
                   if not tab._is_model_cached(k))
        orig_start = models_mod.start_download

        def _fake_start(_settings, model_key):
            with modelstore._lock:
                modelstore._downloads[model_key] = {"progress": 0.4, "done": False, "error": None}
            return True

        models_mod.start_download = _fake_start
        try:
            tab._start_download(key)
            self._pump(count=25, step_delay=0.03)  # one 500 ms poll tick
            refs = tab._card_refs[key]
            if str(refs["status"].cget("text")) != "Downloading…" or key not in tab._progress_widgets:
                self.errors.append(f"Models: '{key}' shows no download progress")
            self._state("models", "models-downloading", sfx)
            with modelstore._lock:
                modelstore._downloads[key].update(done=True, error="test: simulated failure")
            self._pump(count=25, step_delay=0.03)
            if str(refs["status"].cget("text")) != models_mod.FAILED_TEXT:
                self.errors.append(f"Models: '{key}' does not show the failed download")
            retry = refs.get("retry_button")
            if retry is None or not retry.winfo_ismapped():
                self.errors.append(f"Models: '{key}' failed card has no visible 'Try again'")
            self._state("models", "models-failed", sfx)
        finally:
            models_mod.start_download = orig_start
            with modelstore._lock:
                modelstore._downloads.pop(key, None)
            self.dash.rebuild_tab("models")

    def _state_hotkey(self, sfx: str) -> None:
        """Hotkey capture mode ("Press a key combo..." + "Esc to cancel"), entered
        like a click does but with the global keyboard hook stubbed out."""
        import types

        self.dash._show_tab("general")
        self._pump(count=6, step_delay=0.03)
        gen = self.dash._tabs["general"]
        fake = types.ModuleType("keyboard")
        fake.hook = lambda *a, **k: object()
        fake.unhook = lambda *a, **k: None
        fake.KEY_DOWN, fake.KEY_UP = "down", "up"
        saved = sys.modules.get("keyboard")
        sys.modules["keyboard"] = fake
        try:
            gen._hotkey_display.invoke()
            self._pump(count=6, step_delay=0.03)
            if not gen._hotkey_capturing:
                self.errors.append("General: hotkey capture mode did not start")
            self._state("general", "general-hotkey-capture", sfx)
            gen._stop_hotkey_capture(cancelled=True)
            self._pump(count=6, step_delay=0.03)
            if gen._hotkey_capturing:
                self.errors.append("General: hotkey capture did not cancel")
        finally:
            if gen._hotkey_capturing:
                gen._hotkey_capturing = False
            if saved is not None:
                sys.modules["keyboard"] = saved
            else:
                sys.modules.pop("keyboard", None)

    def _state_history(self, sfx: str) -> None:
        """Empty history, a search with no results, one very long entry."""
        import src.history as history_mod

        path = history_mod.HISTORY_FILE
        saved = path.read_text(encoding="utf-8") if path.exists() else ""

        def _show() -> Any:
            self.dash.rebuild_tab("history")
            self.dash._show_tab("history")
            self._pump(count=8, step_delay=0.03)
            return self.dash._tabs["history"]

        try:
            path.write_text("", encoding="utf-8")
            _show()
            self._state("history", "history-empty", sfx)

            path.write_text(saved, encoding="utf-8")
            tab = _show()
            tab._search_entry.insert(0, "zzqx no such words")
            tab._render_entries()
            self._state("history", "history-no-results", sfx)

            long_text = " ".join(
                ["This is one very long dictation that keeps going so the card has to wrap "
                 "over many lines without cutting any words off at the right edge."] * 12
                + ["Supercalifragilisticexpialidocious_without_any_spaces_" * 3])
            path.write_text(json.dumps({"ts": time.time(), "text": long_text, "duration": 95.0,
                                        "model": "distil-large-v3", "lang": "en"}) + "\n",
                            encoding="utf-8")
            _show()
            self._state("history", "history-long-entry", sfx)
        finally:
            path.write_text(saved, encoding="utf-8")
            self.dash.rebuild_tab("history")
            self._pump(count=4)

    def _state_setup(self, sfx: str) -> None:
        """Setup pages in their rarer states: model already present, failed
        download, mic blocked by Windows, no mic at all. No real mic/download."""
        from src import modelstore, oskit
        from src.ui import onboarding as ob

        cls = ob.OnboardingScreen
        saved_methods = (cls._open_mic_stream, cls._start_download)
        saved_dir = self.settings.model_dir
        kit = oskit.get()
        failed_model = self.settings.model_size

        def _fake_fail(screen, model):
            with modelstore._lock:
                modelstore._downloads[model] = {"progress": 0.3, "done": True,
                                                "error": "test: simulated failure"}

        cls._open_mic_stream = lambda screen, device: False
        cls._start_download = _fake_fail
        try:
            self.settings.model_dir = str(self._temp_path / "models")  # model is present
            self.dash.show_onboarding()
            self._pump(count=8, step_delay=0.03)
            screen = self.dash._onboarding
            if screen is None:
                self.errors.append("Setup screen did not open (states)")
                return

            screen.show_page(ob.PAGES.index("model"))
            self._state("onboarding", "setup-model-present", sfx, root=screen.frame)

            self.settings.model_dir = str(self._temp_path / "empty-models")
            screen.show_page(ob.PAGES.index("model"))
            self._pump(count=4)
            screen._dl_btn.invoke()
            self._pump(count=10, step_delay=0.03)
            if str(screen._dl_msg.cget("text")) != ob.DOWNLOAD_FAILED_TEXT:
                self.errors.append("Setup: failed download message not shown")
            self._state("onboarding", "setup-model-failed", sfx, root=screen.frame)
            with modelstore._lock:
                modelstore._downloads.pop(failed_model, None)

            for name, blocked, text in (("setup-mic-blocked", True, ob.MIC_BLOCKED_TEXT),
                                        ("setup-mic-none", False, ob.NO_MIC_TEXT)):
                kit.mic_access_blocked = (lambda b=blocked: b)
                screen.show_page(ob.PAGES.index("microphone"))
                self._pump(count=6, step_delay=0.03)
                if str(screen._mic_msg.cget("text")) != text:
                    self.errors.append(f"Setup: '{name}' message not shown")
                self._state("onboarding", name, sfx, root=screen.frame)
        finally:
            kit.__dict__.pop("mic_access_blocked", None)
            cls._open_mic_stream, cls._start_download = saved_methods
            with modelstore._lock:
                modelstore._downloads.pop(failed_model, None)
            self.settings.model_dir = saved_dir
            if self.dash._onboarding is not None:
                self.dash._onboarding._leave_page()
                self.dash._end_onboarding(skipped=False, model_changed=False)
            self._pump(count=8, step_delay=0.03)

    def _overlay_pass(self, sfx: str = "") -> None:
        """The dictation overlay (own Tk root on its own thread, like the app).
        Only show()/update_text()/hide() and root.after() cross threads — the
        same calls the app makes; geometry is read ON the overlay thread."""
        import threading
        import tkinter as tk
        from src import overlay as ov

        # The overlay strings live in src/main.py; read them without importing
        # it (that would pull in the tray, hotkey and transcriber modules).
        import ast
        consts = {}
        for node in ast.parse((REPO_ROOT / "src" / "main.py").read_text(encoding="utf-8")).body:
            if (isinstance(node, ast.Assign) and len(node.targets) == 1
                    and isinstance(node.targets[0], ast.Name)
                    and isinstance(node.value, ast.Constant)):
                consts[node.targets[0].id] = node.value.value
        WRITING_TEXT = consts.get("WRITING_TEXT", "Writing it down…")
        STILL_WRITING_TEXT = consts.get("STILL_WRITING_TEXT",
                                        "Still writing your last dictation — one moment.")

        print(f"\n  State group: overlay{sfx}")
        saved_default = tk._default_root
        saved_enabled = self.settings.overlay_enabled
        self.settings.overlay_enabled = True
        overlay = ov.TranscriptionOverlay(self.settings)
        try:
            overlay.start()
            # The overlay drops tkinter's default root (GOTCHAS); give the
            # dashboard's back so the walker's later widgets bind to it.
            tk._default_root = saved_default
            if overlay._root is None:
                self.errors.append("Overlay did not start")
                return
            long_partial = " ".join(["and then I kept talking about the quarterly plan, "
                                     "the budget and who owns which part of it"] * 6)
            states = (("overlay-writing", WRITING_TEXT),
                      ("overlay-still-writing", STILL_WRITING_TEXT),
                      ("overlay-long-partial", long_partial))
            for name, text in states:
                overlay.show(text)
                time.sleep(0.6)  # fade-in
                box: Dict[str, Any] = {}
                done = threading.Event()

                def _probe() -> None:
                    try:
                        r, p, lbl = overlay._root, overlay._panel, overlay._panel_label
                        boxes = [(r.winfo_rootx(), r.winfo_rooty(), r.winfo_width(), r.winfo_height())]
                        if overlay._panel_shown:
                            boxes.append((p.winfo_rootx(), p.winfo_rooty(),
                                          p.winfo_width(), p.winfo_height()))
                            box["label"] = (str(lbl.cget("text")),
                                            (lbl.winfo_rootx(), lbl.winfo_rooty(),
                                             lbl.winfo_width(), lbl.winfo_height()),
                                            (lbl.winfo_reqwidth(), lbl.winfo_reqheight()))
                        box["boxes"] = boxes
                        box["pill"] = (r.winfo_width(), r.winfo_height())
                    except Exception as e:
                        box["error"] = str(e)
                    finally:
                        done.set()

                overlay._root.after(0, _probe)
                if not done.wait(timeout=3):
                    self.errors.append(f"Overlay: '{name}' probe timed out")
                    continue
                if "error" in box:
                    self.errors.append(f"Overlay: '{name}' probe failed: {box['error']}")
                    continue
                want = ov.pill_size(self.settings.overlay_pill_scale)
                if box["pill"] != want:
                    self.errors.append(f"Overlay: '{name}{sfx}' pill {box['pill']} != {want} "
                                       f"at scale {self.settings.overlay_pill_scale}")
                if "label" not in box:
                    self.errors.append(f"Overlay: '{name}' text panel not shown")
                else:
                    # The label's on-screen rectangle must lie inside the
                    # panel window with at least half the padding on every
                    # side, and be no smaller than the text it has to show.
                    shown, (lx, ly, lw, lh), (req_w, req_h) = box["label"]
                    px, py, pw, ph = box["boxes"][1]
                    gaps = {"left": lx - px, "top": ly - py,
                            "right": (px + pw) - (lx + lw), "bottom": (py + ph) - (ly + lh)}
                    need = {"left": ov._PADDING_X // 2, "right": ov._PADDING_X // 2,
                            "top": ov._PADDING_Y // 2, "bottom": ov._PADDING_Y // 2}
                    bad = [f"{k} {gaps[k]}px < {need[k]}px" for k in gaps if gaps[k] < need[k]]
                    if req_w > lw or req_h > lh:
                        bad.append(f"label {lw}x{lh}px < text {req_w}x{req_h}px")
                    geo = (f"label {lw}x{lh}+{lx}+{ly} in panel {pw}x{ph}+{px}+{py}, "
                           f"gaps L{gaps['left']} T{gaps['top']} R{gaps['right']} B{gaps['bottom']}")
                    if bad:
                        msg = (f"Overlay text outside its panel ('{name}{sfx}'): {shown[:60]!r}… "
                               f"{'; '.join(bad)} ({geo})")
                        print(f"    [OVERFLOW] {msg}")
                        self.errors.append(msg)
                    else:
                        print(f"  [contain] overlay '{name}{sfx}': text inside panel ({geo})")
                self._grab_boxes(box["boxes"], f"state-{name}{sfx}")
            self._pill_voice_state(overlay, sfx)
            overlay.hide()
            time.sleep(0.5)
        except Exception as e:
            self.errors.append(f"Overlay states failed: {e}")
        finally:
            overlay.shutdown()
            time.sleep(0.3)
            tk._default_root = saved_default
            self.settings.overlay_enabled = saved_enabled
            self._pump(count=4)

    def _overlay_variants_pass(self) -> None:
        """Overlay at the smallest and largest pill, and with a nearly solid
        text box; the user's values are restored afterwards."""
        saved = (self.settings.overlay_pill_scale, self.settings.overlay_panel_opacity)
        try:
            for scale, panel, sfx in ((0.6, saved[1], "-pill60"), (1.4, saved[1], "-pill140"),
                                      (saved[0], 0.98, "-solidbox")):
                self.settings.overlay_pill_scale = scale
                self.settings.overlay_panel_opacity = panel
                self._overlay_pass(sfx)
        finally:
            self.settings.overlay_pill_scale, self.settings.overlay_panel_opacity = saved

    def _pill_voice_state(self, overlay: Any, sfx: str) -> None:
        """The pill while someone talks: feed a synthetic speech-like waveform
        (50 ms blocks, like the mic) and screenshot the voice-shaped lines."""
        import threading

        import numpy as np
        from src import transcriber as tr
        rate, blk = tr.SAMPLE_RATE, tr.WAVE_BLOCK
        t = np.arange(int(1.5 * rate)) / rate
        ph = 2 * np.pi * np.cumsum(140 + 20 * np.sin(2 * np.pi * 0.7 * t)) / rate
        voice = sum(np.sin(k * ph) / k for k in range(1, 8))
        voice = (0.12 * voice * (0.6 + 0.4 * np.sin(2 * np.pi * 4 * t))).astype(np.float32)
        for i in range(0, len(voice) - blk + 1, blk):
            b = voice[i:i + blk]
            overlay.set_level(min(1.0, (float(np.sqrt(np.mean(b ** 2))) / 0.012) ** 0.6))
            overlay.set_waveform(tr.voice_features(b))
            time.sleep(blk / rate)
        box: Dict[str, Any] = {}
        done = threading.Event()

        def _probe() -> None:
            try:
                r = overlay._root
                box["boxes"] = [(r.winfo_rootx(), r.winfo_rooty(), r.winfo_width(), r.winfo_height())]
            except Exception as e:
                box["error"] = str(e)
            finally:
                done.set()

        overlay._root.after(0, _probe)
        if not done.wait(timeout=3) or "error" in box:
            self.errors.append(f"Overlay: pill-voice probe failed: {box.get('error', 'timeout')}")
            return
        self._grab_boxes(box["boxes"], f"state-overlay-pill-voice{sfx}")

    def _grab_boxes(self, boxes: List[Tuple[int, int, int, int]], name: str) -> None:
        """Screenshot the union of screen rectangles (x, y, w, h) to ui-<name>.png."""
        target = self.work_dir / f"ui-{name}.png"
        try:
            from PIL import ImageGrab
            x0 = min(b[0] for b in boxes)
            y0 = min(b[1] for b in boxes)
            x1 = max(b[0] + b[2] for b in boxes)
            y1 = max(b[1] + b[3] for b in boxes)
            ImageGrab.grab(bbox=(x0 - 8, y0 - 8, x1 + 8, y1 + 8)).save(target)
            self.screenshots.append(target)
            print(f"  [screenshot] Captured '{name}' -> {target.name} ({x1 - x0}x{y1 - y0})")
        except Exception as e:
            print(f"  [screenshot] Warning: overlay grab failed: {e}")

    def _light_pass(self) -> None:
        """Default tabs, setup pages, conditional states and overlay in the
        LIGHT theme (`*-light.png`), then the theme is restored."""
        print(f"\n==================================================")
        print("Light theme pass...")
        print(f"==================================================")
        orig_theme = self.settings.theme
        try:
            self.dash._set_theme("light")
            self._pump(count=10, step_delay=0.03)
            for tab_name in ("general", "models", "history", "settings"):
                self.dash._show_tab(tab_name)
                self._pump(count=6, step_delay=0.03)
                self._check_cutoff(tab_name, f"light, {self.dash.winfo_width()}px")
                self._capture_tab_screenshot(f"{tab_name}-light")
            self._walk_onboarding(tag="light")
            self._states_pass("-light")
            self._overlay_pass("-light")
        finally:
            self.dash._set_theme(orig_theme)
            self._pump(count=10, step_delay=0.03)

    def _narrow_pass(self, label: str) -> None:
        """Every tab at the minimum window width (logical px)."""
        from src.ui.dashboard import MIN_W
        print(f"\n==================================================")
        print(f"Cut-off check at the minimum width ({MIN_W} logical px, {label})...")
        print(f"==================================================")
        scale = self.dash._get_window_scaling()
        self.dash.geometry(f"{MIN_W}x{round(self.dash.winfo_height() / scale)}")
        self._pump(count=10, step_delay=0.03)
        for tab_name in ("general", "models", "history", "settings"):
            self.dash._show_tab(tab_name)
            self._pump(count=6, step_delay=0.03)
            self._check_cutoff(tab_name, f"{label}, {self.dash.winfo_width()}px")
            self._capture_tab_screenshot(f"{tab_name}-narrow-{label.replace(' ', '')}")

    def _work_area(self) -> Tuple[int, int, int, int]:
        from src import oskit
        cx = self.dash.winfo_rootx() + self.dash.winfo_width() // 2
        cy = self.dash.winfo_rooty() + self.dash.winfo_height() // 2
        area = oskit.get().work_area((cx, cy))
        return area or (0, 0, self.dash.winfo_screenwidth(), self.dash.winfo_screenheight())

    def _check_fits_screen(self, where: str, buttons: Tuple[Any, ...] = ()) -> None:
        """The window's outer rectangle (title bar + borders included) lies
        inside the monitor work area, and so does every given button."""
        self._pump(count=6, step_delay=0.02)
        left, top, right, bottom = self._work_area()
        d = self.dash
        border = max(0, d.winfo_rootx() - d.winfo_x())
        outer = (d.winfo_x(), d.winfo_y(),
                 d.winfo_rootx() + d.winfo_width() + border,
                 d.winfo_rooty() + d.winfo_height() + border)
        # Windows 10/11 count an invisible ~7 px resize border in the frame.
        slack = 8
        ok = (outer[0] >= left - slack and outer[1] >= top - slack
              and outer[2] <= right + slack and outer[3] <= bottom + slack)
        msg = f"window {outer} vs work area {(left, top, right, bottom)}"
        if ok:
            print(f"  [fits] {where}: {msg}")
        else:
            print(f"    [OFF-SCREEN] {where}: {msg}")
            self.errors.append(f"Window does not fit the screen ({where}): {msg}")
        for b in buttons:
            try:
                if not b.winfo_ismapped():
                    self.errors.append(f"Setup button {b.cget('text')!r} not shown ({where})")
                    continue
                bx, by = b.winfo_rootx(), b.winfo_rooty()
                br, bb = bx + b.winfo_width(), by + b.winfo_height()
                inside = (bx >= max(left, d.winfo_rootx()) and by >= max(top, d.winfo_rooty())
                          and br <= min(right, d.winfo_rootx() + d.winfo_width())
                          and bb <= min(bottom, d.winfo_rooty() + d.winfo_height()))
                if not inside:
                    m = (f"Setup button {b.cget('text')!r} outside the visible area ({where}): "
                         f"{(bx, by, br, bb)}")
                    print(f"    [OFF-SCREEN] {m}")
                    self.errors.append(m)
            except Exception as e:
                self.errors.append(f"Setup button check failed ({where}): {e}")

    def _walk_onboarding(self, tag: str = "") -> None:
        """Show the first-time setup screen and walk every page. No mic
        stream (the level meter is fed a fake value) and no download (the
        modelstore entry is marked done instantly)."""
        print(f"\n==================================================")
        print("Walking First-Time Setup...")
        print(f"==================================================")
        from src import modelstore
        from src.ui import onboarding as ob

        def _fake_mic(screen, device):
            screen._level = 0.55
            return True

        def _fake_download(screen, model):
            with modelstore._lock:
                modelstore._downloads[model] = {"progress": 1.0, "done": True, "error": None}

        ob.OnboardingScreen._open_mic_stream = _fake_mic
        ob.OnboardingScreen._start_download = _fake_download
        self.settings.model_dir = str(self._temp_path / "empty-models")  # model page offers the download
        self.dash.show_onboarding()
        self._pump(count=8, step_delay=0.03)
        screen = self.dash._onboarding
        if screen is None:
            self.errors.append("Setup screen did not open")
            return
        for n, page in enumerate(ob.PAGES, 1):
            if screen.page_name != page:
                self.errors.append(f"Setup: expected page '{page}', got '{screen.page_name}'")
                return
            suffix = f"-{tag}" if tag else ""
            self._capture_tab_screenshot(f"setup-{n}-{page}{suffix}")
            where = f"setup '{page}'{(' ' + tag) if tag else ''}, {self.dash.winfo_width()}px"
            self._check_cutoff("onboarding", where, root=screen.frame)
            self._check_fits_screen(where, (screen._skip_btn, screen._back_btn, screen._next_btn))
            if page == "model" and hasattr(screen, "_dl_btn"):
                screen._dl_btn.invoke()
                self._pump(count=30, step_delay=0.03)
                self._capture_tab_screenshot(f"setup-{n}-{page}-done{suffix}")
            if page == "sound":
                screen._sound_btns["mute"].invoke()
                screen._sound_btns["off"].invoke()
            if str(screen._next_btn.cget("state")) == "disabled":
                self.errors.append(f"Setup: Next disabled on page '{page}'")
                return
            if n > 1:
                screen.back()
                self._pump(count=4, step_delay=0.02)
                screen.next()
                self._pump(count=4, step_delay=0.02)
                if page == "model" and hasattr(screen, "_later_btn"):
                    screen._later_btn.invoke()
            screen._next_btn.invoke()
            self._pump(count=8, step_delay=0.03)
            self.controls_exercised += 1
            print(f"    [OK] Setup page '{page}'")
        if self.dash._onboarding is not None or not self.settings.onboarding_done:
            self.errors.append("Setup: Finish did not end setup")
        elif "general" not in self.dash._tab_frames:
            self.errors.append("Setup: tabs not shown after Finish")
        else:
            print("    [OK] Finish -> tabs shown, onboarding_done saved")

    def _teardown(self) -> None:
        """Cleanly close the dashboard window and remove temp files."""
        print("\nTearing down UI test runner...")
        if self.dash is not None:
            try:
                self.dash._on_close()
            except Exception:
                pass
        if self.root is not None:
            try:
                self.root.quit()
                self.root.destroy()
            except Exception:
                pass
        if self._temp_dir is not None:
            try:
                self._temp_dir.cleanup()
            except Exception:
                pass

    def _print_summary(self) -> int:
        """Print test summary to stdout and return process exit code."""
        print("\n" + "=" * 64)
        print("EQHO UI INTERACTIVE TEST SUMMARY")
        print("=" * 64)
        print(f"Tabs visited:       {len(self.tabs_visited)} ({', '.join(self.tabs_visited)})")
        print(f"Controls exercised: {self.controls_exercised}")
        print(f"Controls skipped:   {self.controls_skipped}")
        print(f"Errors encountered: {len(self.errors)}")
        print(f"Output directory:   {self.work_dir}")
        print(f"Screenshots saved:  {len(self.screenshots)}")
        for s in self.screenshots:
            print(f"  - {s.name}")

        if self.errors:
            print("\nERRORS:")
            for err in self.errors:
                print(f"  FAILED: {err}")
            print("\nRESULT: FAILED (exit 1)")
            print("=" * 64)
            return 1
        else:
            print("\nRESULT: ALL CHECKS PASSED (exit 0)")
            print("=" * 64)
            return 0


def main() -> int:
    # Output may be redirected to a file (cp1252 on Windows): never crash on symbols like "✕".
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(encoding="utf-8", errors="replace")
        except Exception:
            pass
    parser = argparse.ArgumentParser(description="Eqho UI Interactive Test (EQ-TESTUI)")
    parser.add_argument(
        "--work",
        type=Path,
        default=Path("_test/ui"),
        help="Output directory for screenshots and test artifacts (default: _test/ui)",
    )
    parser.add_argument(
        "--delay",
        type=float,
        default=0.03,
        help="Delay in seconds between control operations (default: 0.03)",
    )
    args = parser.parse_args()

    runner = UITestRunner(work_dir=args.work, delay=args.delay)
    return runner.run()


if __name__ == "__main__":
    sys.exit(main())
