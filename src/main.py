"""Eqho -- always-on dictation app entry point.

Wires together: settings, transcriber, overlay, hotkey, tray, and injector.
"""

import logging
import threading
import time
import warnings
from typing import Optional

from . import chime, oskit, textproc
from .fonts import load_fonts, unload_fonts
from .history import TranscriptHistory
from .settings import Settings, VOLUME_DUCK_OPTIONS, CONFIG_DIR
from .transcriber import VoiceTranscriber
from .overlay import TranscriptionOverlay
from .hotkey import HotkeyManager
from .injector import type_text, get_foreground_window, set_foreground_window, send_backspaces
from .tray import TrayApp

_LOG_FORMAT = "%(asctime)s [%(levelname)s] %(name)s: %(message)s"
logging.basicConfig(
    level=logging.INFO,
    format=_LOG_FORMAT,
    datefmt="%H:%M:%S",
)
# File log — the packaged exe has no console, so without this, crashes and
# errors leave no trace at all. Lives next to settings.json.
try:
    from logging.handlers import RotatingFileHandler
    CONFIG_DIR.mkdir(parents=True, exist_ok=True)
    _file_handler = RotatingFileHandler(
        CONFIG_DIR / "eqho.log", maxBytes=1_000_000, backupCount=2, encoding="utf-8",
    )
    _file_handler.setFormatter(logging.Formatter(_LOG_FORMAT, datefmt="%Y-%m-%d %H:%M:%S"))
    logging.getLogger().addHandler(_file_handler)
except Exception:
    pass
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
        self.history = TranscriptHistory()
        self._dictation_start: float = 0.0
        self._last_injected: str = ""  # for the "delete that" voice command
        self._settings_apply_lock = threading.Lock()  # serialize settings applies

        self.transcriber.set_callbacks(
            on_partial=self._on_partial,
            on_complete=self._on_complete,
            on_status=self._on_status,
            on_level=self._on_level,
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
        if self.settings.voice_commands:
            if textproc.is_delete_command(text):
                self._handle_delete_command()
                return
            command_text = textproc.match_command(text)
            if command_text is not None:
                text = command_text
        with self._lock:
            self._pending_text.append(text)
            full_so_far = textproc.smart_join(self._pending_text)
        self.overlay.update_text(full_so_far)

    def _handle_delete_command(self) -> None:
        """'Delete that': drop the previous utterance — the pending one if
        dictation is mid-flight, else backspace the last injected text."""
        with self._lock:
            if self._pending_text:
                self._pending_text.pop()
                remaining = textproc.smart_join(self._pending_text)
                last_injected = None
            else:
                last_injected, self._last_injected = self._last_injected, ""
                remaining = ""
        if last_injected:
            if len(last_injected) <= 400:  # backspacing a novel would take ages
                send_backspaces(len(last_injected))
            return
        self.overlay.update_text(remaining or "Listening...")

    def _on_status(self, message: str) -> None:
        """Transcriber lifecycle messages (e.g. model finished loading)."""
        self.overlay.update_text(message)

    def _on_level(self, level: float) -> None:
        """Live mic level (0..1) for the overlay's audio indicator."""
        self.overlay.set_level(level)

    # -- Activation control ----------------------------------------------------

    def _duck_volume(self) -> None:
        """Silently lower system volume based on user setting."""
        multiplier = VOLUME_DUCK_OPTIONS.get(self.settings.volume_duck)
        if multiplier is None:  # "off"
            return
        kit = oskit.get()
        saved = kit.get_volume()
        if saved is None:  # volume control unavailable on this OS setup
            return
        with self._lock:
            self._saved_volume = saved
        if multiplier == 0.0:
            kit.set_mute(True)
        else:
            kit.set_volume(saved * multiplier)
        log.info("Volume ducked: %.0f%% → %s", saved * 100,
                 "muted" if multiplier == 0.0 else f"{saved * multiplier * 100:.0f}%")

    def _restore_volume(self) -> None:
        """Silently restore system volume to previous level."""
        with self._lock:
            saved = self._saved_volume
        if saved is None:
            return
        kit = oskit.get()
        kit.set_mute(False)
        if kit.set_volume(saved):
            with self._lock:
                self._saved_volume = None
            log.info("Volume restored to %.0f%%.", saved * 100)

    def activate(self) -> None:
        hwnd = get_foreground_window()
        with self._lock:
            self._target_hwnd = hwnd
            self._pending_text.clear()
        log.info("Dictation activated (target window: %s)", hwnd)
        self._dictation_start = time.monotonic()
        self.overlay.show(
            "Listening..." if self.transcriber.is_model_ready() else "Loading model…"
        )
        self.tray.set_active(True)
        # Mic first (recording starts instantly), THEN the chime — blocking,
        # so it finishes before ducking mutes the output (the old fire-and-
        # forget blip raced the mute and was sometimes silent). Ducking lands
        # ~0.3s into dictation, which is inaudible in practice.
        self.transcriber.start()
        if self.settings.sound_feedback:
            chime.play_start(blocking=True)
        self._duck_volume()
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
        if self.settings.sound_feedback:
            chime.play_stop()  # after restore, so it's audible
        self.tray.set_active(False)
        self.overlay.hide()

        with self._lock:
            full_text = textproc.smart_join(self._pending_text)
            self._pending_text.clear()
            target_hwnd = self._target_hwnd

        full_text = textproc.apply_replacements(full_text, self.settings.replacements)
        if self.settings.format_cleanup:
            full_text = textproc.clean_text(full_text, remove_fillers=self.settings.remove_fillers)

        if full_text:
            time.sleep(0.4)  # wait for modifier keys to fully release
            if target_hwnd:
                set_foreground_window(target_hwnd)
                time.sleep(0.15)  # let the window come to focus
            type_text(full_text, use_clipboard=self.settings.auto_paste)
            log.info("Injected text: %s", full_text)
            self._last_injected = full_text
            if self.settings.history_enabled:
                duration = time.monotonic() - self._dictation_start if self._dictation_start else 0.0
                self.history.append(
                    full_text, duration=duration,
                    model=self.settings.model_size, lang=self.settings.language,
                )

    def toggle(self) -> None:
        if self.transcriber.is_running():
            self.deactivate()
        else:
            self.activate()

    # -- Settings change -------------------------------------------------------

    def _on_settings_changed(self, reload_model: bool = False) -> None:
        """Apply settings changes on a background thread — callers include the
        pystray menu thread and dashboard callbacks, and a model reload (or
        download) must never block either."""
        def _apply() -> None:
            with self._settings_apply_lock:
                try:
                    log.info("Settings changed, re-registering hotkey%s",
                             " and reloading model" if reload_model else "")
                    self.hotkey.unregister()
                    self.hotkey.register()
                    self.tray.set_active(self._is_active())
                    if reload_model:
                        self.transcriber.reload_model()
                        # Eagerly load (and download, with tray notifications) the
                        # new model so the next dictation starts instantly.
                        self._preload_model()
                except Exception:
                    log.exception("Applying settings change failed")

        threading.Thread(target=_apply, daemon=True).start()

    def _is_active(self) -> bool:
        return self.transcriber.is_running()

    # -- Lifecycle -------------------------------------------------------------

    def run(self) -> None:
        from .watchdog import start as start_watchdog
        start_watchdog()
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
        oskit.get().set_mute(False)
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


_single_instance_socket = None


def _acquire_single_instance() -> bool:
    """One Eqho per machine. Two instances both hook the hotkey, fight over
    the mic, and stack models into VRAM until CUDA loads hang (the model-
    switch 'freeze'). A bound localhost port is the cross-platform lock."""
    global _single_instance_socket
    import socket
    try:
        s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        s.bind(("127.0.0.1", 48317))
        s.listen(1)
        _single_instance_socket = s  # held for the process lifetime
        return True
    except OSError:
        return False


def _log_unhandled_exceptions() -> None:
    """Route unhandled exceptions (main + threads) into the log file."""
    import sys

    def _hook(exc_type, exc, tb):
        log.critical("UNHANDLED EXCEPTION", exc_info=(exc_type, exc, tb))

    def _thread_hook(args):
        name = args.thread.name if args.thread else "?"
        log.critical("UNHANDLED THREAD EXCEPTION in %s", name,
                     exc_info=(args.exc_type, args.exc_value, args.exc_traceback))

    sys.excepthook = _hook
    threading.excepthook = _thread_hook


def main() -> None:
    if not _acquire_single_instance():
        log.error("Eqho is already running — this instance exits. "
                  "Check the system tray (and Task Manager for frozen instances).")
        try:
            import tkinter as tk
            from tkinter import messagebox
            root = tk.Tk()
            root.withdraw()
            messagebox.showinfo(
                "Eqho", "Eqho is already running — check the system tray.")
            root.destroy()
        except Exception:
            pass
        return

    import atexit
    atexit.register(_emergency_unmute)
    _log_unhandled_exceptions()
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
