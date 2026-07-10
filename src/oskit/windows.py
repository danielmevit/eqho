"""Windows implementation — absorbed from main.py (pycaw), tray.py/general.py
(winreg autostart + theme), fonts.py (AddFontResourceEx), injector.py (focus),
transcriber.py (communications ducking)."""

import logging
from pathlib import Path
from typing import Optional

from .base import OsKit

log = logging.getLogger(__name__)

_FR_PRIVATE = 0x10
_RUN_KEY = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Run"
_THEME_KEY = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Themes\Personalize"


class WindowsOsKit(OsKit):
    name = "windows"

    # -- volume (pycaw; COM must be initialized per-thread) --------------------

    def _volume_ctl(self):
        try:
            from comtypes import CoInitialize
            from pycaw.pycaw import AudioUtilities
            CoInitialize()
            return AudioUtilities.GetSpeakers().EndpointVolume
        except Exception:
            return None

    def _guid_null(self):
        from comtypes import GUID
        return GUID()

    def get_volume(self) -> Optional[float]:
        ctl = self._volume_ctl()
        try:
            return ctl.GetMasterVolumeLevelScalar() if ctl else None
        except Exception:
            return None

    def set_volume(self, level: float) -> bool:
        ctl = self._volume_ctl()
        try:
            if ctl:
                ctl.SetMasterVolumeLevelScalar(max(0.0, min(1.0, level)), self._guid_null())
                return True
        except Exception as e:
            log.debug("set_volume failed: %s", e)
        return False

    def set_mute(self, muted: bool) -> bool:
        ctl = self._volume_ctl()
        try:
            if ctl:
                ctl.SetMute(bool(muted), self._guid_null())
                return True
        except Exception as e:
            log.debug("set_mute failed: %s", e)
        return False

    # -- window focus -------------------------------------------------------------

    def get_foreground_window(self):
        import ctypes
        return ctypes.windll.user32.GetForegroundWindow()

    def set_foreground_window(self, handle) -> None:
        import ctypes
        if handle:
            ctypes.windll.user32.SetForegroundWindow(handle)

    # -- autostart (HKCU Run key, value "Eqho" — same one the installer uses) -----

    def set_autostart(self, enabled: bool, command: str) -> bool:
        try:
            import winreg
            with winreg.OpenKey(winreg.HKEY_CURRENT_USER, _RUN_KEY, 0,
                                winreg.KEY_SET_VALUE | winreg.KEY_READ) as key:
                if enabled:
                    winreg.SetValueEx(key, "Eqho", 0, winreg.REG_SZ, command)
                    log.info("Added Eqho to Windows startup.")
                else:
                    try:
                        winreg.DeleteValue(key, "Eqho")
                        log.info("Removed Eqho from Windows startup.")
                    except FileNotFoundError:
                        pass
            return True
        except Exception as e:
            log.error("Failed to update startup registry: %s", e)
            return False

    # -- fonts (private, per-process) -----------------------------------------------

    def load_fonts(self, font_dir: Path) -> int:
        try:
            import ctypes
            loaded = 0
            for font_file in sorted(font_dir.glob("*.otf")) + sorted(font_dir.glob("*.ttf")):
                if ctypes.windll.gdi32.AddFontResourceExW(str(font_file), _FR_PRIVATE, 0):
                    loaded += 1
            return loaded
        except Exception as e:
            log.debug("Font load failed: %s", e)
            return 0

    def unload_fonts(self, font_dir: Path) -> None:
        try:
            import ctypes
            for font_file in sorted(font_dir.glob("*.otf")) + sorted(font_dir.glob("*.ttf")):
                ctypes.windll.gdi32.RemoveFontResourceExW(str(font_file), _FR_PRIVATE, 0)
        except Exception:
            pass

    # -- theme -------------------------------------------------------------------------

    def _theme_value(self, value_name: str) -> str:
        try:
            import winreg
            key = winreg.OpenKey(winreg.HKEY_CURRENT_USER, _THEME_KEY)
            value, _ = winreg.QueryValueEx(key, value_name)
            winreg.CloseKey(key)
            return "light" if value == 1 else "dark"
        except Exception:
            return "dark"

    def system_theme(self) -> str:
        return self._theme_value("AppsUseLightTheme")

    def taskbar_theme(self) -> str:
        return self._theme_value("SystemUsesLightTheme")

    # -- misc ------------------------------------------------------------------------------

    def disable_os_mic_ducking(self) -> None:
        """Set Windows Communications Activity to "Do nothing" so it stops
        adjusting other apps' volume when the mic opens."""
        try:
            import winreg
            key_path = r"SOFTWARE\Microsoft\Multimedia\Audio"
            with winreg.OpenKey(winreg.HKEY_CURRENT_USER, key_path, 0,
                                winreg.KEY_READ | winreg.KEY_WRITE) as key:
                try:
                    current, _ = winreg.QueryValueEx(key, "UserDuckingPreference")
                    if current == 3:  # already "Do nothing"
                        return
                except FileNotFoundError:
                    pass
                # 3 = "Do nothing" (0=mute, 1=reduce 80%, 2=reduce 50%)
                winreg.SetValueEx(key, "UserDuckingPreference", 0, winreg.REG_DWORD, 3)
                log.info("Disabled Windows audio ducking (Communications Activity → Do nothing).")
        except Exception as e:
            log.debug("Could not disable audio ducking: %s", e)
