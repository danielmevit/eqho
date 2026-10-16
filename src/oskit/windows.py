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
_MIC_CONSENT_KEY = (r"SOFTWARE\Microsoft\Windows\CurrentVersion"
                    r"\CapabilityAccessManager\ConsentStore\microphone")


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

    # -- injection ----------------------------------------------------------------

    _MODIFIER_VKS = {
        "ctrl_l": 0xA2, "ctrl_r": 0xA3,
        "shift_l": 0xA0, "shift_r": 0xA1,
        "alt_l": 0xA4, "alt_r": 0xA5,
        "cmd_l": 0x5B, "cmd_r": 0x5C,
    }

    def held_modifiers(self) -> Optional[set]:
        try:
            import ctypes
            state = ctypes.windll.user32.GetAsyncKeyState
            return {name for name, vk in self._MODIFIER_VKS.items() if state(vk) & 0x8000}
        except Exception as e:
            log.debug("held_modifiers failed: %s", e)
            return None

    def get_window_process_name(self, handle) -> Optional[str]:
        if not handle:
            return None
        try:
            import ctypes
            from ctypes import wintypes
            pid = wintypes.DWORD()
            ctypes.windll.user32.GetWindowThreadProcessId(handle, ctypes.byref(pid))
            PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
            h = ctypes.windll.kernel32.OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION, False, pid.value)
            if not h:
                return None
            try:
                buf = ctypes.create_unicode_buffer(260)
                size = wintypes.DWORD(260)
                if ctypes.windll.kernel32.QueryFullProcessImageNameW(
                        h, 0, buf, ctypes.byref(size)):
                    import os as _os
                    return _os.path.basename(buf.value)
            finally:
                ctypes.windll.kernel32.CloseHandle(h)
        except Exception as e:
            log.debug("get_window_process_name failed: %s", e)
        return None

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

    # -- screen geometry ------------------------------------------------------------

    def work_area(self):
        try:
            import ctypes
            from ctypes import byref, wintypes
            rect = wintypes.RECT()
            SPI_GETWORKAREA = 0x0030
            if ctypes.windll.user32.SystemParametersInfoW(SPI_GETWORKAREA, 0, byref(rect), 0):
                return (rect.left, rect.top, rect.right, rect.bottom)
        except Exception:
            pass
        return None

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

    # -- microphone privacy (read-only registry check) ----------------------------

    def _consent_value(self, root, subkey: str) -> Optional[str]:
        """Read a ConsentStore key's 'Value' ('Allow'/'Deny'), or None when
        the key or value can't be read. Never writes."""
        try:
            import winreg
            with winreg.OpenKey(root, subkey, 0, winreg.KEY_READ) as key:
                value, _ = winreg.QueryValueEx(key, "Value")
                return str(value)
        except Exception:
            return None

    def mic_access_blocked(self) -> Optional[bool]:
        """The Windows microphone privacy switch. 'Deny' for the user, for
        desktop apps (NonPackaged) or machine-wide means apps can't open
        the mic; user AND desktop apps both readable 'Allow' means fine;
        anything else (unreadable, partial) is unknown."""
        import winreg
        user = self._consent_value(winreg.HKEY_CURRENT_USER, _MIC_CONSENT_KEY)
        non_packaged = self._consent_value(
            winreg.HKEY_CURRENT_USER, _MIC_CONSENT_KEY + r"\NonPackaged")
        machine = self._consent_value(winreg.HKEY_LOCAL_MACHINE, _MIC_CONSENT_KEY)
        if "Deny" in (user, non_packaged, machine):
            return True
        if user == "Allow" and non_packaged == "Allow":
            return False
        return None

    def open_mic_privacy_settings(self) -> bool:
        try:
            import os
            os.startfile("ms-settings:privacy-microphone")
            return True
        except Exception as e:
            log.debug("Could not open microphone privacy settings: %s", e)
            return False

    # -- misc ------------------------------------------------------------------------------

    def free_memory_mb(self) -> Optional[int]:
        try:
            import ctypes
            from ctypes import wintypes

            class _MemStatus(ctypes.Structure):
                _fields_ = [("dwLength", wintypes.DWORD),
                            ("dwMemoryLoad", wintypes.DWORD),
                            ("ullTotalPhys", ctypes.c_ulonglong),
                            ("ullAvailPhys", ctypes.c_ulonglong),
                            ("ullTotalPageFile", ctypes.c_ulonglong),
                            ("ullAvailPageFile", ctypes.c_ulonglong),
                            ("ullTotalVirtual", ctypes.c_ulonglong),
                            ("ullAvailVirtual", ctypes.c_ulonglong),
                            ("ullAvailExtendedVirtual", ctypes.c_ulonglong)]

            status = _MemStatus()
            status.dwLength = ctypes.sizeof(_MemStatus)
            if not ctypes.windll.kernel32.GlobalMemoryStatusEx(ctypes.byref(status)):
                return None
            return int(status.ullAvailPhys // (1024 * 1024))
        except Exception:
            log.debug("GlobalMemoryStatusEx failed", exc_info=True)
            return None

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
