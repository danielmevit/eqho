"""Linux implementation. Volume via wpctl (PipeWire) with pactl fallback;
autostart via XDG desktop entry; fonts via ~/.local/share/fonts + fc-cache;
theme via gsettings; focus restore via xdotool when present (X11 only —
Wayland compositors don't allow it, dictation still works without it)."""

import logging
import shutil
import subprocess
from pathlib import Path
from typing import Optional

from .base import OsKit

log = logging.getLogger(__name__)

_AUTOSTART_FILE = Path.home() / ".config" / "autostart" / "eqho.desktop"
_FONT_DEST = Path.home() / ".local" / "share" / "fonts" / "Eqho"


def _run(cmd: list, timeout: float = 3.0) -> Optional[str]:
    try:
        out = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout)
        if out.returncode == 0:
            return out.stdout.strip()
    except Exception:
        pass
    return None


class LinuxOsKit(OsKit):
    name = "linux"

    def __init__(self):
        self._wpctl = shutil.which("wpctl")
        self._pactl = shutil.which("pactl")
        self._xdotool = shutil.which("xdotool")

    # -- volume ---------------------------------------------------------------

    def get_volume(self) -> Optional[float]:
        if self._wpctl:
            out = _run([self._wpctl, "get-volume", "@DEFAULT_AUDIO_SINK@"])
            if out:  # "Volume: 0.45" or "Volume: 0.45 [MUTED]"
                try:
                    return float(out.split()[1])
                except (IndexError, ValueError):
                    pass
        if self._pactl:
            out = _run([self._pactl, "get-sink-volume", "@DEFAULT_SINK@"])
            if out and "%" in out:
                try:
                    percent = int(out.split("%")[0].rsplit(" ", 1)[-1].strip("/ "))
                    return percent / 100.0
                except ValueError:
                    pass
        return None

    def set_volume(self, level: float) -> bool:
        level = max(0.0, min(1.0, level))
        if self._wpctl:
            return _run([self._wpctl, "set-volume", "@DEFAULT_AUDIO_SINK@", f"{level:.2f}"]) is not None
        if self._pactl:
            return _run([self._pactl, "set-sink-volume", "@DEFAULT_SINK@", f"{int(level * 100)}%"]) is not None
        return False

    def set_mute(self, muted: bool) -> bool:
        flag = "1" if muted else "0"
        if self._wpctl:
            return _run([self._wpctl, "set-mute", "@DEFAULT_AUDIO_SINK@", flag]) is not None
        if self._pactl:
            return _run([self._pactl, "set-sink-mute", "@DEFAULT_SINK@", flag]) is not None
        return False

    # -- window focus (X11 best-effort) ------------------------------------------

    def get_foreground_window(self):
        if self._xdotool:
            out = _run([self._xdotool, "getactivewindow"])
            if out:
                return out
        return None

    def set_foreground_window(self, handle) -> None:
        if handle and self._xdotool:
            _run([self._xdotool, "windowactivate", str(handle)])

    # -- autostart (XDG) -------------------------------------------------------------

    def set_autostart(self, enabled: bool, command: str) -> bool:
        try:
            if enabled:
                _AUTOSTART_FILE.parent.mkdir(parents=True, exist_ok=True)
                _AUTOSTART_FILE.write_text(
                    "[Desktop Entry]\n"
                    "Type=Application\n"
                    "Name=Eqho\n"
                    "Comment=Local voice-to-text dictation\n"
                    f"Exec={command}\n"
                    "Terminal=false\n"
                    "X-GNOME-Autostart-enabled=true\n",
                    encoding="utf-8",
                )
            else:
                _AUTOSTART_FILE.unlink(missing_ok=True)
            return True
        except Exception as e:
            log.error("Failed to update autostart entry: %s", e)
            return False

    # -- fonts ---------------------------------------------------------------------------

    def load_fonts(self, font_dir: Path) -> int:
        try:
            fonts = sorted(font_dir.glob("*.otf")) + sorted(font_dir.glob("*.ttf"))
            if not fonts:
                return 0
            _FONT_DEST.mkdir(parents=True, exist_ok=True)
            copied = 0
            for font_file in fonts:
                dest = _FONT_DEST / font_file.name
                if not dest.exists():
                    shutil.copy2(font_file, dest)
                    copied += 1
            if copied and shutil.which("fc-cache"):
                _run(["fc-cache", "-f", str(_FONT_DEST)], timeout=10)
            return len(fonts)
        except Exception as e:
            log.debug("Font install failed: %s", e)
            return 0

    # (no unload — fontconfig fonts persist per-user, which is fine)

    # -- theme -----------------------------------------------------------------------------

    def system_theme(self) -> str:
        out = _run(["gsettings", "get", "org.gnome.desktop.interface", "color-scheme"])
        if out and "dark" in out.lower():
            return "dark"
        if out:  # key exists and isn't dark
            return "light"
        return "dark"
