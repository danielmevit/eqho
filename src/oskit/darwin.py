"""macOS implementation. Volume via osascript; autostart via a LaunchAgent
plist; theme via AppleInterfaceStyle. Window-focus restore is a no-op in v1
(injection goes to the frontmost app, which is normally the dictation target).

Note: global hotkeys and keystroke injection need the app to be granted
Accessibility + Input Monitoring in System Settings → Privacy & Security.
"""

import logging
import subprocess
from pathlib import Path
from typing import Optional

from .base import OsKit

log = logging.getLogger(__name__)

_AGENT_FILE = Path.home() / "Library" / "LaunchAgents" / "xyz.damt.eqho.plist"


def _osascript(script: str) -> Optional[str]:
    try:
        out = subprocess.run(["osascript", "-e", script],
                             capture_output=True, text=True, timeout=3)
        if out.returncode == 0:
            return out.stdout.strip()
    except Exception:
        pass
    return None


class DarwinOsKit(OsKit):
    name = "darwin"

    # -- volume (osascript uses a 0–100 scale) ---------------------------------

    def get_volume(self) -> Optional[float]:
        out = _osascript("output volume of (get volume settings)")
        try:
            return int(out) / 100.0 if out is not None else None
        except ValueError:
            return None

    def set_volume(self, level: float) -> bool:
        level = max(0.0, min(1.0, level))
        return _osascript(f"set volume output volume {int(level * 100)}") is not None

    def set_mute(self, muted: bool) -> bool:
        flag = "true" if muted else "false"
        return _osascript(f"set volume output muted {flag}") is not None

    # -- autostart (LaunchAgent) ---------------------------------------------------

    def set_autostart(self, enabled: bool, command: str) -> bool:
        try:
            if enabled:
                # Split naive "quoted program" args back apart for ProgramArguments
                parts = [p for p in command.replace('"', "").split(" ") if p]
                args = "\n".join(
                    f"        <string>{p}</string>" for p in parts
                )
                _AGENT_FILE.parent.mkdir(parents=True, exist_ok=True)
                _AGENT_FILE.write_text(
                    '<?xml version="1.0" encoding="UTF-8"?>\n'
                    '<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" '
                    '"http://www.apple.com/DTDs/PropertyList-1.0.dtd">\n'
                    '<plist version="1.0">\n<dict>\n'
                    "    <key>Label</key>\n    <string>xyz.damt.eqho</string>\n"
                    "    <key>ProgramArguments</key>\n    <array>\n"
                    f"{args}\n"
                    "    </array>\n"
                    "    <key>RunAtLoad</key>\n    <true/>\n"
                    "</dict>\n</plist>\n",
                    encoding="utf-8",
                )
            else:
                _AGENT_FILE.unlink(missing_ok=True)
            return True
        except Exception as e:
            log.error("Failed to update LaunchAgent: %s", e)
            return False

    # -- theme ------------------------------------------------------------------------

    def system_theme(self) -> str:
        try:
            out = subprocess.run(
                ["defaults", "read", "-g", "AppleInterfaceStyle"],
                capture_output=True, text=True, timeout=3,
            )
            # Key exists only in dark mode
            return "dark" if out.returncode == 0 and "dark" in out.stdout.lower() else "light"
        except Exception:
            return "dark"
