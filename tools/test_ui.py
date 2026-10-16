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

        import src.settings as settings_mod
        import src.history as history_mod

        settings_mod.CONFIG_FILE = self._temp_path / "settings.json"
        settings_mod.CONFIG_DIR = self._temp_path
        settings_mod.LEGACY_MODEL_DIR = self._temp_path / "no-legacy-model-cache"
        history_mod.HISTORY_FILE = self._temp_path / "history.jsonl"

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

    def run(self) -> int:
        """Execute the full UI test suite."""
        print_skip_list()
        print("\nStarting Eqho UI Interactive Test...\n")

        try:
            self._setup_isolation()
            self._build_dashboard()

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
