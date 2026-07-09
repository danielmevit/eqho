"""Global hotkey listener supporting hold-to-talk and toggle modes."""

import logging
import threading
import time
from typing import Callable, Optional

import keyboard

from .settings import Settings

log = logging.getLogger(__name__)


class HotkeyManager:
    """Registers a global hotkey and calls back on press/release."""

    def __init__(
        self,
        settings: Settings,
        on_activate: Callable[[], None],
        on_deactivate: Callable[[], None],
        on_toggle: Optional[Callable[[], None]] = None,
    ):
        self._settings = settings
        self._on_activate = on_activate
        self._on_deactivate = on_deactivate
        self._on_toggle_cb = on_toggle
        self._active = False
        self._registered = False
        self._lock = threading.Lock()
        self._hold_hook = None  # single hook for hold mode (handles both press/release)
        self._hotkey_handler = None  # from add_hotkey
        self._last_toggle_time: float = 0

    def register(self) -> None:
        if self._registered:
            self.unregister()

        combo = self._settings.hotkey
        mode = self._settings.hotkey_mode

        if mode == "hold":
            # Use a single global hook that filters for our key + modifiers.
            # This avoids the keyboard library's hook_key bug where two
            # hooks on the same key name corrupt internal _hooks state.
            target_key = self._last_key(combo)
            target_scans = set(keyboard.key_to_scan_codes(target_key))

            def _hold_handler(event):
                if event.scan_code not in target_scans:
                    return
                if event.event_type == keyboard.KEY_DOWN:
                    self._on_hold_press(event)
                elif event.event_type == keyboard.KEY_UP:
                    self._on_hold_release(event)

            self._hold_hook = keyboard.hook(_hold_handler, suppress=False)
        else:
            self._hotkey_handler = keyboard.add_hotkey(combo, self._on_toggle, suppress=False)

        self._registered = True
        log.info("Hotkey registered: %s (%s mode)", combo, mode)

    def unregister(self) -> None:
        if self._hold_hook is not None:
            try:
                keyboard.unhook(self._hold_hook)
            except Exception as e:
                log.debug("Failed to unhook hold hook: %s", e)
            self._hold_hook = None

        if self._hotkey_handler is not None:
            try:
                keyboard.remove_hotkey(self._hotkey_handler)
            except Exception as e:
                log.debug("Failed to remove hotkey: %s", e)
            self._hotkey_handler = None

        self._registered = False
        self._active = False
        log.info("Hotkey unregistered.")

    def _last_key(self, combo: str) -> str:
        return combo.split("+")[-1].strip()

    def _modifiers_held(self) -> bool:
        combo = self._settings.hotkey
        parts = [p.strip().lower() for p in combo.split("+")]
        mods = parts[:-1]
        for m in mods:
            if not keyboard.is_pressed(m):
                return False
        return True

    # -- Toggle mode ----------------------------------------------------------

    def _on_toggle(self) -> None:
        now = time.monotonic()
        with self._lock:
            if now - self._last_toggle_time < 0.4:
                return  # debounce rapid double-fires
            self._last_toggle_time = now
        # Toggle state authority lives with the app (it knows whether the
        # transcriber actually started); we only debounce here. The local
        # _active flag is a fallback when no on_toggle callback was given.
        if self._on_toggle_cb is not None:
            self._on_toggle_cb()
            return
        with self._lock:
            if self._active:
                self._active = False
                self._on_deactivate()
            else:
                self._active = True
                self._on_activate()

    # -- Hold mode ------------------------------------------------------------

    def _on_hold_press(self, _event) -> None:
        if not self._modifiers_held():
            return
        with self._lock:
            if not self._active:
                self._active = True
                self._on_activate()

    def _on_hold_release(self, _event) -> None:
        with self._lock:
            if self._active:
                self._active = False
                self._on_deactivate()

    @property
    def is_active(self) -> bool:
        return self._active
