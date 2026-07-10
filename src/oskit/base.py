"""OS abstraction layer ("oskit" — `platform` would clash with the stdlib).

`get()` returns the singleton for the running OS. Every capability on the
base class is a SAFE NO-OP, so a platform that hasn't implemented something
degrades gracefully instead of crashing — core dictation must work everywhere,
extras light up per-OS.
"""

import logging
import sys
from pathlib import Path
from typing import Optional

log = logging.getLogger(__name__)


class OsKit:
    """Base implementation: every capability is a safe no-op."""

    name = "generic"

    # -- System output volume (for ducking) — levels are 0.0..1.0 -------------
    def get_volume(self) -> Optional[float]:
        return None

    def set_volume(self, level: float) -> bool:
        return False

    def set_mute(self, muted: bool) -> bool:
        return False

    # -- Window focus (capture before dictation, restore before injection) ----
    def get_foreground_window(self):
        return None

    def set_foreground_window(self, handle) -> None:
        pass

    # -- Autostart on login ----------------------------------------------------
    def set_autostart(self, enabled: bool, command: str) -> bool:
        return False

    # -- Bundled fonts -----------------------------------------------------------
    def load_fonts(self, font_dir: Path) -> int:
        return 0

    def unload_fonts(self, font_dir: Path) -> None:
        pass

    # -- Theme detection ---------------------------------------------------------
    def system_theme(self) -> str:
        """'dark' or 'light' for app surfaces."""
        return "dark"

    def taskbar_theme(self) -> str:
        """'dark' or 'light' for the tray icon (Windows can differ from apps)."""
        return self.system_theme()

    # -- Misc OS behaviors ---------------------------------------------------------
    def disable_os_mic_ducking(self) -> None:
        """Stop the OS from auto-ducking other audio while the mic is open."""
        pass


def autostart_command() -> str:
    """The command that relaunches this app at login (frozen exe or script)."""
    import sys
    if getattr(sys, "frozen", False):
        return f'"{sys.executable}"'
    script = Path(sys.argv[0]).resolve()
    return f'"{sys.executable}" "{script}"'


_instance: Optional[OsKit] = None


def get() -> OsKit:
    global _instance
    if _instance is None:
        if sys.platform == "win32":
            from .windows import WindowsOsKit as Impl
        elif sys.platform == "darwin":
            from .darwin import DarwinOsKit as Impl
        else:
            from .linux import LinuxOsKit as Impl
        _instance = Impl()
        log.info("OS kit: %s", _instance.name)
    return _instance
