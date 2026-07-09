"""Eqho -- always-on dictation app entry point.

Wires together: settings, transcriber, overlay, hotkey, tray, and injector.
"""

import logging
import threading
import time
import warnings
from typing import Optional

from .fonts import load_fonts, unload_fonts
from .settings import Settings, VOLUME_DUCK_OPTIONS
from .transcriber import VoiceTranscriber
from .overlay import TranscriptionOverlay
from .hotkey import HotkeyManager
from .injector import type_text, get_foreground_window, set_foreground_window

# Silent volume control via Windows Core Audio API (pycaw)
try:
    from pycaw.pycaw import AudioUtilities
    from comtypes import GUID, CoInitialize
    _GUID_NULL = GUID()
    _HAS_VOLUME_CTL = True
except Exception:
    _HAS_VOLUME_CTL = False


def _get_volume_ctl():
    """Get a fresh volume endpoint (must be called per-thread due to COM)."""
    try:
        CoInitialize()
        return AudioUtilities.GetSpeakers().EndpointVolume
    except Exception:
        return None
from .tray import TrayApp

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s [%(levelname)s] %(name)s: %(message)s",
    datefmt="%H:%M:%S",
)
# Silence noisy libraries
for _quiet in ("PIL", "httpx", "httpcore", "urllib3"):
    logging.getLogger(_quiet).setLevel(logging.WARNING)
# huggingface_hub emits a WARNING about missing HF_TOKEN on every download —
# harmless (downloads work fine without auth) but confusing to users
logging.getLogger("huggingface_hub").setLevel(logging.ERROR)
warnings.filterwarnings("ignore", message=".*HF_TOKEN.*")
warnings.filterwarnings("ignore", message=".*unauthenticated.*")
log = logging.getLogger("eqho")


class App:
    """Top-level application controller."""

    def __init__(self) -> None:
        self.settings = Settings.load()
        self.transcriber = VoiceTranscriber(self.settings)
        self.overlay = TranscriptionOverlay(self.settings)
        self._pending_text: list[str] = []
        self._lock = threading.Lock()
        self._target_hwnd: int = 0  # window to paste into
        self._saved_volume: Optional[float] = None  # for volume ducking

        self.transcriber.set_callbacks(
            on_partial=self._on_partial,
            on_complete=self._on_complete,
            on_status=self._on_status,
        )

        self.hotkey = HotkeyManager(
            self.settings,
            on_activate=self.activate,
            on_deactivate=self.deactivate,
            on_toggle=self.toggle,  # toggle-mode state authority lives here
        )

        self.tray = TrayApp(
            self.settings,
            on_toggle=self.toggle,
            on_quit=self.quit,
            on_settings_changed=self._on_settings_changed,
        )

    # -- Transcription callbacks -----------------------------------------------

    def _on_partial(self, text: str) -> None:
        self.overlay.update_text(text)

    def _on_complete(self, text: str) -> None:
        with self._lock:
            self._pending_text.append(text)
            full_so_far = " ".join(self._pending_text)
        self.overlay.update_text(full_so_far)

    def _on_status(self, message: str) -> None:
        """Transcriber lifecycle messages (e.g. model finished loading)."""
        self.overlay.update_text(message)

    # -- Activation control ----------------------------------------------------

    def _duck_volume(self) -> None:
        """Silently lower system volume based on user setting."""
        if not _HAS_VOLUME_CTL:
            return
        multiplier = VOLUME_DUCK_OPTIONS.get(self.settings.volume_duck)
        if multiplier is None:  # "off"
            return
        try:
            ctl = _get_volume_ctl()
            if not ctl:
                return
            saved = ctl.GetMasterVolumeLevelScalar()
            with self._lock:
                self._saved_volume = saved
            if multiplier == 0.0:
                ctl.SetMute(True, _GUID_NULL)
            else:
                ctl.SetMasterVolumeLevelScalar(saved * multiplier, _GUID_NULL)
            log.info("Volume ducked: %.0f%% → %s", saved * 100,
                     "muted" if multiplier == 0.0 else f"{saved * multiplier * 100:.0f}%")
        except Exception as e:
            log.debug("Volume duck failed: %s", e)

    def _restore_volume(self) -> None:
        """Silently restore system volume to previous level."""
        if not _HAS_VOLUME_CTL:
            return
        with self._lock:
            saved = self._saved_volume
        if saved is None:
            return
        try:
            ctl = _get_volume_ctl()
            if not ctl:
                return
            ctl.SetMute(False, _GUID_NULL)
            ctl.SetMasterVolumeLevelScalar(saved, _GUID_NULL)
            with self._lock:
                self._saved_volume = None
            log.info("Volume restored to %.0f%%.", saved * 100)
        except Exception as e:
            log.debug("Volume restore failed: %s", e)

    def activate(self) -> None:
        hwnd = get_foreground_window()
        with self._lock:
            self._target_hwnd = hwnd
            self._pending_text.clear()
        log.info("Dictation activated (target window: %s)", hwnd)
        self._duck_volume()
        self.overlay.show(
            "Listening..." if self.transcriber.is_model_ready() else "Loading model…"
        )
        self.tray.set_active(True)
        self.transcriber.start()
        mic_error = self.transcriber.consume_mic_error()
        if mic_error:
            if not self.transcriber.is_running():
                self.tray.notify(f"No microphone available: {mic_error}")
                self.overlay.hide()
                self.tray.set_active(False)
                self._restore_volume()
            else:
                self.tray.notify("Selected mic unavailable, using default microphone.")

    def deactivate(self) -> None:
        log.info("Dictation deactivated")
        self.transcriber.stop()
        self._restore_volume()
        self.tray.set_active(False)
        self.overlay.hide()

        with self._lock:
            full_text = " ".join(t.strip() for t in self._pending_text if t.strip())
            self._pending_text.clear()
            target_hwnd = self._target_hwnd

        if full_text:
            time.sleep(0.4)  # wait for modifier keys to fully release
            if target_hwnd:
                set_foreground_window(target_hwnd)
                time.sleep(0.15)  # let the window come to focus
            type_text(full_text, use_clipboard=self.settings.auto_paste)
            log.info("Injected text: %s", full_text)

    def toggle(self) -> None:
        if self.transcriber.is_running():
            self.deactivate()
        else:
            self.activate()

    # -- Settings change -------------------------------------------------------

    def _on_settings_changed(self, reload_model: bool = False) -> None:
        log.info("Settings changed, re-registering hotkey%s", " and reloading model" if reload_model else "")
        self.hotkey.unregister()
        self.hotkey.register()
        self.tray.set_active(self._is_active())
        if reload_model:
            self.transcriber.reload_model()

    def _is_active(self) -> bool:
        return self.transcriber.is_running()

    # -- Lifecycle -------------------------------------------------------------

    def run(self) -> None:
        load_fonts()
        log.info("Eqho starting...")
        log.info(
            "Model: %s | Hotkey: %s (%s mode)",
            self.settings.model_size,
            self.settings.hotkey,
            self.settings.hotkey_mode,
        )

        self.overlay.start()
        self.hotkey.register()

        threading.Thread(target=self._preload_model, daemon=True).start()

        self.tray.run()

    def _model_is_cached(self) -> bool:
        """Check if the current model has already been downloaded."""
        from .settings import is_model_cached
        return is_model_cached(self.settings, self.settings.model_size)

    def _preload_model(self) -> None:
        try:
            needs_download = not self._model_is_cached()
            if needs_download:
                log.info("Model not cached, downloading (this may take a few minutes)...")
                self.tray.notify(f"Downloading model '{self.settings.model_size}'... this may take a few minutes.")
            else:
                log.info("Pre-loading model from cache...")
            self.transcriber._ensure_model()
            log.info("Model pre-loaded and ready.")
            if needs_download:
                self.tray.notify("Model downloaded and ready!")
        except Exception as e:
            log.error("Failed to pre-load model: %s", e)
            self.tray.notify(f"Failed to load model: {e}")

    def quit(self) -> None:
        log.info("Shutting down...")
        # Close dashboard first to avoid tkinter Variable.__del__ errors
        from .ui import shutdown_dashboard
        shutdown_dashboard()
        self._restore_volume()  # always unmute on exit
        self.hotkey.unregister()
        self.transcriber.shutdown()
        self.overlay.shutdown()
        self.settings.save()
        unload_fonts()


def _emergency_unmute() -> None:
    """Last resort: unmute system audio on any exit."""
    try:
        if _HAS_VOLUME_CTL:
            ctl = _get_volume_ctl()
            if ctl:
                ctl.SetMute(False, _GUID_NULL)
    except Exception:
        pass


def _suppress_tk_variable_del() -> None:
    """Patch tkinter Variable.__del__ to suppress RuntimeError on shutdown.

    When the dashboard thread's Tcl interpreter is gone, garbage-collected
    StringVar/BooleanVar objects raise 'main thread is not in main loop'.
    This is harmless — suppress it to avoid log spam.
    """
    try:
        import tkinter as tk
        _original_del = tk.Variable.__del__

        def _safe_del(self):
            try:
                _original_del(self)
            except (RuntimeError, AttributeError, TypeError):
                pass

        tk.Variable.__del__ = _safe_del
    except Exception:
        pass


def main() -> None:
    import atexit
    atexit.register(_emergency_unmute)
    _suppress_tk_variable_del()

    app = App()
    try:
        app.run()
    except KeyboardInterrupt:
        app.quit()
    finally:
        _emergency_unmute()


if __name__ == "__main__":
    main()
