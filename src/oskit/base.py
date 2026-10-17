"""OS abstraction layer ("oskit" — `platform` would clash with the stdlib).

`get()` returns the singleton for the running OS. Every capability on the
base class is a SAFE NO-OP, so a platform that hasn't implemented something
degrades gracefully instead of crashing — core dictation must work everywhere,
extras light up per-OS.
"""

import logging
import os
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

    def get_window_process_name(self, handle) -> Optional[str]:
        """Executable name (e.g. 'notepad.exe') of the window's process, for
        per-app rules. None where unsupported."""
        return None

    # -- Autostart on login ----------------------------------------------------
    def set_autostart(self, enabled: bool, command: str) -> bool:
        return False

    # -- Bundled fonts -----------------------------------------------------------
    def load_fonts(self, font_dir: Path) -> int:
        return 0

    def unload_fonts(self, font_dir: Path) -> None:
        pass

    # -- Screen geometry -----------------------------------------------------------
    def work_area(self, point=None):
        """(left, top, right, bottom) of the desktop excluding taskbar/dock,
        or None when unknown (callers fall back to the full screen). With
        `point` (x, y): the work area of the monitor showing that point."""
        return None

    # -- Theme detection ---------------------------------------------------------
    def system_theme(self) -> str:
        """'dark' or 'light' for app surfaces."""
        return "dark"

    def taskbar_theme(self) -> str:
        """'dark' or 'light' for the tray icon (Windows can differ from apps)."""
        return self.system_theme()

    # -- Injection / clipboard -----------------------------------------------------
    def paste_modifier(self) -> str:
        """The modifier key used for pasting text ('ctrl' or 'cmd')."""
        return "ctrl"

    def held_modifiers(self) -> Optional[set]:
        """Names of the modifier keys physically held right now (e.g.
        {"ctrl_l", "alt_r"}), or None when the OS can't tell."""
        return None

    def set_clipboard_text(self, text: str, private: bool = True) -> bool:
        """Put *text* on the clipboard; *private* keeps it out of clipboard
        history / cloud sync / monitors where the OS supports that. False =
        not supported or failed (caller falls back). Never raises."""
        return False

    def get_clipboard_text(self) -> Optional[str]:
        """Clipboard text, or None when unsupported / empty / failed."""
        return None

    # -- Microphone privacy ------------------------------------------------------

    def mic_access_blocked(self) -> Optional[bool]:
        """True when the OS blocks microphone access for apps (e.g. the
        Windows privacy switch), False when clearly allowed, None when
        unknown. Read-only — never changes any setting."""
        return None

    def open_mic_privacy_settings(self) -> bool:
        """Open the OS microphone privacy settings page. True on success."""
        return False

    # -- Memory --------------------------------------------------------------------
    def free_memory_mb(self) -> Optional[int]:
        """Memory the PC can still hand to a new program, in MB (None when
        unknown). Used to warn before loading a speech model on the CPU."""
        return None

    # -- Misc OS behaviors ---------------------------------------------------------
    def disable_os_mic_ducking(self) -> None:
        """Stop the OS from auto-ducking other audio while the mic is open."""
        pass


def autostart_command() -> str:
    """The command that relaunches this app at login (frozen exe or script)."""
    appimage = os.environ.get("APPIMAGE")
    if appimage and os.path.isfile(appimage):
        return f'"{appimage}"'
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
