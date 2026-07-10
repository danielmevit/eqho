"""Global hotkey listener supporting hold-to-talk and toggle modes.

Two backends, chosen by settings.hotkey_backend ("auto" | "keyboard" | "pynput"):
- "keyboard": the keyboard library — reliable Win32 hooks; needs root on Linux
  and doesn't work on macOS. Default on Windows.
- "pynput": works without root on X11 and on macOS (after the user grants
  Accessibility + Input Monitoring). Default on Linux/macOS.
"""

import logging
import sys
import threading
import time
from typing import Callable, Optional

from .settings import Settings

log = logging.getLogger(__name__)

_MODIFIER_ALIASES = {
    "alt": "alt", "alt_l": "alt", "alt_r": "alt", "alt_gr": "alt",
    "ctrl": "ctrl", "ctrl_l": "ctrl", "ctrl_r": "ctrl", "control": "ctrl",
    "shift": "shift", "shift_l": "shift", "shift_r": "shift",
    "cmd": "cmd", "cmd_l": "cmd", "cmd_r": "cmd",
    "win": "cmd", "windows": "cmd", "super": "cmd",
}


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
        self._backend = ""
        self._lock = threading.Lock()
        self._hold_hook = None  # keyboard-lib hold-mode hook
        self._hotkey_handler = None  # keyboard-lib add_hotkey handle
        self._pynput_hotkeys = None  # pynput GlobalHotKeys (toggle mode)
        self._pynput_listener = None  # pynput Listener (hold mode)
        self._held_mods: set = set()
        self._last_toggle_time: float = 0

    # -- Registration ------------------------------------------------------------

    def _pick_backend(self) -> str:
        backend = getattr(self._settings, "hotkey_backend", "auto") or "auto"
        if backend == "auto":
            return "keyboard" if sys.platform == "win32" else "pynput"
        return backend

    def register(self) -> None:
        if self._registered:
            self.unregister()

        combo = self._settings.hotkey
        mode = self._settings.hotkey_mode
        self._backend = self._pick_backend()

        if self._backend == "keyboard":
            self._register_keyboard(combo, mode)
        else:
            self._register_pynput(combo, mode)

        self._registered = True
        log.info("Hotkey registered: %s (%s mode, %s backend)", combo, mode, self._backend)

    def _register_keyboard(self, combo: str, mode: str) -> None:
        import keyboard

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

    def _register_pynput(self, combo: str, mode: str) -> None:
        from pynput import keyboard as pk

        if mode == "toggle":
            self._pynput_hotkeys = pk.GlobalHotKeys({
                self._pynput_combo(combo): self._on_toggle,
            })
            self._pynput_hotkeys.daemon = True
            self._pynput_hotkeys.start()
            return

        # Hold mode: track modifiers ourselves and match the trigger key.
        target = self._last_key(combo).lower()
        needed_mods = {
            _MODIFIER_ALIASES.get(p.strip().lower(), p.strip().lower())
            for p in combo.split("+")[:-1]
        }
        self._held_mods = set()

        def _name_of(key) -> Optional[str]:
            if isinstance(key, pk.KeyCode):
                return (key.char or "").lower() or None
            return getattr(key, "name", None)

        def _on_press(key):
            name = _name_of(key)
            if name in _MODIFIER_ALIASES:
                self._held_mods.add(_MODIFIER_ALIASES[name])
                return
            if name == target and needed_mods <= self._held_mods:
                self._on_hold_press(None)

        def _on_release(key):
            name = _name_of(key)
            if name in _MODIFIER_ALIASES:
                self._held_mods.discard(_MODIFIER_ALIASES[name])
                return
            if name == target:
                self._on_hold_release(None)

        self._pynput_listener = pk.Listener(on_press=_on_press, on_release=_on_release)
        self._pynput_listener.daemon = True
        self._pynput_listener.start()

    @staticmethod
    def _pynput_combo(combo: str) -> str:
        """'alt+q' → '<alt>+q' (pynput GlobalHotKeys syntax)."""
        parts = [p.strip().lower() for p in combo.split("+")]
        out = [f"<{_MODIFIER_ALIASES.get(p, p)}>" for p in parts[:-1]]
        last = parts[-1]
        out.append(f"<{last}>" if len(last) > 1 else last)
        return "+".join(out)

    def unregister(self) -> None:
        if self._hold_hook is not None:
            try:
                import keyboard
                keyboard.unhook(self._hold_hook)
            except Exception as e:
                log.debug("Failed to unhook hold hook: %s", e)
            self._hold_hook = None

        if self._hotkey_handler is not None:
            try:
                import keyboard
                keyboard.remove_hotkey(self._hotkey_handler)
            except Exception as e:
                log.debug("Failed to remove hotkey: %s", e)
            self._hotkey_handler = None

        for attr in ("_pynput_hotkeys", "_pynput_listener"):
            listener = getattr(self, attr)
            if listener is not None:
                try:
                    listener.stop()
                except Exception as e:
                    log.debug("Failed to stop pynput listener: %s", e)
                setattr(self, attr, None)

        self._registered = False
        self._active = False
        log.info("Hotkey unregistered.")

    def _last_key(self, combo: str) -> str:
        return combo.split("+")[-1].strip()

    def _modifiers_held(self) -> bool:
        combo = self._settings.hotkey
        parts = [p.strip().lower() for p in combo.split("+")]
        mods = parts[:-1]
        if self._backend == "pynput":
            aliased = {_MODIFIER_ALIASES.get(m, m) for m in mods}
            return aliased <= self._held_mods
        import keyboard
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
