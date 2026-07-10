"""System tray icon with menu for Eqho."""

import logging
import os
import sys
import threading
from pathlib import Path
from typing import Callable, Optional

from PIL import Image, ImageDraw
import pystray

from . import oskit
from .oskit.base import autostart_command
from .settings import Settings, SUPPORTED_LANGUAGES, WHISPER_MODELS, VOLUME_DUCK_OPTIONS, OVERLAY_POSITIONS
from .audio import list_input_devices
from .ui import open_dashboard

log = logging.getLogger(__name__)

_ASSETS = Path(__file__).resolve().parent.parent / "assets"


def _get_taskbar_theme() -> str:
    """Taskbar light/dark mode (can differ from the apps theme on Windows)."""
    return oskit.get().taskbar_theme()


def _load_icon(active: bool = False) -> Image.Image:
    """Load the 'e' logo for the tray icon.

    Picks white icon on dark taskbar, blue icon on light taskbar.
    Dims to 40% brightness for inactive state.
    Falls back to legacy icon_64 PNGs, then programmatic generation.
    """
    # Pick variant based on taskbar theme. Asset names indicate the theme
    # they serve: *_dark.png (white art) for a dark taskbar, *_white.png
    # (blue art) for a light one.
    taskbar = _get_taskbar_theme()
    if taskbar == "dark":
        new_logo = _ASSETS / "logo_62_dark.png"
    else:
        new_logo = _ASSETS / "logo_62_white.png"

    if new_logo.exists():
        img = Image.open(new_logo).convert("RGBA")
        if not active:
            from PIL import ImageEnhance
            img = ImageEnhance.Brightness(img).enhance(0.4)
        return img

    # Fallback to the other variant
    fallback = _ASSETS / "logo_62_white.png"
    if fallback.exists():
        img = Image.open(fallback).convert("RGBA")
        if not active:
            from PIL import ImageEnhance
            img = ImageEnhance.Brightness(img).enhance(0.4)
        return img

    # Legacy fallback
    if active:
        path = _ASSETS / "icon_64_active.png"
    else:
        path = _ASSETS / "icon_64_inactive.png"
    if path.exists():
        return Image.open(path).convert("RGBA")
    path_default = _ASSETS / "icon_64.png"
    if path_default.exists():
        return Image.open(path_default).convert("RGBA")
    return _create_icon_fallback(active)


def _create_icon_fallback(active: bool = False) -> Image.Image:
    """Programmatic fallback using Eqho's gradient palette."""
    size = 64
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(img)
    draw.rounded_rectangle([0, 0, size - 1, size - 1], radius=10, fill="#000000")

    bars = [0.3, 0.5, 0.8, 1.0, 0.7, 0.9, 0.6, 0.4, 0.7, 1.0, 0.8, 0.5, 0.3]
    bar_w, gap = 3, 1
    total_w = len(bars) * (bar_w + gap) - gap
    start_x = (size - total_w) // 2
    cx, cy = size // 2, size // 2
    max_h = size * 0.6

    for i, h_frac in enumerate(bars):
        x = start_x + i * (bar_w + gap)
        h = int(max_h * h_frac)
        t = i / (len(bars) - 1)
        if active:
            r, g, b = int(0), int(255 * (1 - t) + 191 * t), int(171 * (1 - t) + 255 * t)
        else:
            r, g, b = 0, int((255 * (1 - t) + 191 * t) / 3), int((171 * (1 - t) + 255 * t) / 3)
        draw.rounded_rectangle(
            [x, cy - h // 2, x + bar_w, cy + h // 2],
            radius=bar_w // 2,
            fill=(r, g, b),
        )
    return img


class TrayApp:
    """System tray application controller."""

    def __init__(
        self,
        settings: Settings,
        on_toggle: Callable[[], None],
        on_quit: Callable[[], None],
        on_settings_changed: Callable,
    ):
        self._settings = settings
        self._on_toggle = on_toggle
        self._on_quit = on_quit
        self._on_settings_changed = on_settings_changed
        self._icon: Optional[pystray.Icon] = None
        self._is_active = False
        self._dashboard_open = False
        self._theme_stop = threading.Event()

    def _tooltip(self, active: bool = False) -> str:
        lang = SUPPORTED_LANGUAGES.get(self._settings.language, self._settings.language)
        hotkey = self._settings.hotkey.title()
        base = f"Eqho — {hotkey} | {lang}"
        return f"{base} — Listening..." if active else base

    def run(self) -> None:
        self._icon = pystray.Icon(
            "Eqho",
            icon=_load_icon(False),
            title=self._tooltip(),
            menu=self._build_menu(),
        )
        threading.Thread(target=self._watch_taskbar_theme, daemon=True).start()
        self._icon.run()

    def _watch_taskbar_theme(self) -> None:
        """Reload the tray icon when the Windows taskbar theme flips.

        The registry key has no change notification we can use from here, so
        poll it — cheap (one registry read) and 15 s is fast enough for a
        theme switch."""
        last = _get_taskbar_theme()
        while not self._theme_stop.wait(15):
            current = _get_taskbar_theme()
            if current != last and self._icon:
                last = current
                try:
                    self._icon.icon = _load_icon(self._is_active)
                    log.info("Taskbar theme changed to %s — tray icon reloaded.", current)
                except Exception as e:
                    log.debug("Tray icon reload failed: %s", e)

    def set_active(self, active: bool) -> None:
        self._is_active = active
        if self._icon:
            self._icon.icon = _load_icon(active)
            self._icon.title = self._tooltip(active)

    def notify(self, message: str) -> None:
        if self._icon:
            try:
                self._icon.notify(message, "Eqho")
            except Exception:
                pass

    def _build_menu(self) -> pystray.Menu:
        return pystray.Menu(
            pystray.MenuItem("Dashboard", self._open_dashboard_click, default=True),
            pystray.Menu.SEPARATOR,
            pystray.MenuItem(
                lambda _: "Stop Listening" if self._is_active else "Start Listening",
                self._toggle_click,
            ),
            pystray.MenuItem("Microphone", self._mic_submenu()),
            pystray.MenuItem("Model", self._model_submenu()),
            pystray.MenuItem("Hotkey Mode", pystray.Menu(
                pystray.MenuItem(
                    "Toggle (press once)",
                    self._set_mode_toggle,
                    checked=lambda _: self._settings.hotkey_mode == "toggle",
                    radio=True,
                ),
                pystray.MenuItem(
                    "Hold to talk",
                    self._set_mode_hold,
                    checked=lambda _: self._settings.hotkey_mode == "hold",
                    radio=True,
                ),
            )),
            pystray.MenuItem("Paste Mode", pystray.Menu(
                pystray.MenuItem(
                    "Clipboard paste (fast)",
                    self._set_paste_clipboard,
                    checked=lambda _: self._settings.auto_paste,
                    radio=True,
                ),
                pystray.MenuItem(
                    "Simulated typing",
                    self._set_paste_typing,
                    checked=lambda _: not self._settings.auto_paste,
                    radio=True,
                ),
            )),
            pystray.MenuItem("Language", self._language_submenu()),
            pystray.MenuItem("Volume While Speaking", self._volume_duck_submenu()),
            pystray.MenuItem("Show Overlay", self._toggle_overlay,
                             checked=lambda _: self._settings.overlay_enabled),
            pystray.MenuItem("Overlay Position", self._overlay_position_submenu()),
            pystray.Menu.SEPARATOR,
            pystray.MenuItem("Start with Windows", self._toggle_startup,
                             checked=lambda _: self._settings.start_with_windows),
            pystray.Menu.SEPARATOR,
            pystray.MenuItem("Quit", self._quit_click),
        )

    def _mic_submenu(self) -> pystray.Menu:
        items = [pystray.MenuItem(
            "System Default",
            self._make_mic_setter(None),
            checked=lambda _, k=None: self._settings.audio_device is k,
            radio=True,
        )]
        for idx, name in list_input_devices():
            # Skip Bluetooth headset mics to avoid HFP switching
            items.append(pystray.MenuItem(
                name,
                self._make_mic_setter(idx),
                checked=lambda _, k=idx: self._settings.audio_device == k,
                radio=True,
            ))
        return pystray.Menu(*items)

    def _make_mic_setter(self, idx):
        def _set(icon, item):
            self._settings.audio_device = idx
            self._settings.save()
            self._on_settings_changed(reload_model=True)
        return _set

    def _model_submenu(self) -> pystray.Menu:
        items = []
        for key, label in WHISPER_MODELS.items():
            items.append(pystray.MenuItem(
                label,
                self._make_model_setter(key),
                checked=lambda _, k=key: self._settings.model_size == k,
                radio=True,
            ))
        return pystray.Menu(*items)

    def _make_model_setter(self, key: str):
        def _set(icon, item):
            if self._settings.model_size != key:
                self._settings.model_size = key
                self._settings.save()
                self._on_settings_changed(reload_model=True)
        return _set

    def _language_submenu(self) -> pystray.Menu:
        items = []
        for code, name in SUPPORTED_LANGUAGES.items():
            items.append(pystray.MenuItem(
                f"{name} ({code})",
                self._make_lang_setter(code),
                checked=lambda _, c=code: self._settings.language == c,
                radio=True,
            ))
        return pystray.Menu(*items)

    def _make_lang_setter(self, code: str):
        def _set(icon, item):
            self._settings.language = code
            self._settings.save()
            self._on_settings_changed(reload_model=True)
        return _set

    def _volume_duck_submenu(self) -> pystray.Menu:
        labels = {"off": "Off (no change)", "50%": "50%", "25%": "25%", "10%": "10%", "mute": "Mute"}
        items = []
        for key in VOLUME_DUCK_OPTIONS:
            items.append(pystray.MenuItem(
                labels[key],
                self._make_volume_duck_setter(key),
                checked=lambda _, k=key: self._settings.volume_duck == k,
                radio=True,
            ))
        return pystray.Menu(*items)

    def _make_volume_duck_setter(self, key: str):
        def _set(icon, item):
            self._settings.volume_duck = key
            self._settings.save()
        return _set

    def _overlay_position_submenu(self) -> pystray.Menu:
        labels = {
            "bottom-center": "Bottom Center",
            "top-center": "Top Center",
            "top-left": "Top Left",
            "top-right": "Top Right",
            "bottom-left": "Bottom Left",
            "bottom-right": "Bottom Right",
        }
        items = []
        for pos in OVERLAY_POSITIONS:
            items.append(pystray.MenuItem(
                labels.get(pos, pos),
                self._make_overlay_pos_setter(pos),
                checked=lambda _, p=pos: self._settings.overlay_position == p,
                radio=True,
            ))
        return pystray.Menu(*items)

    def _toggle_startup(self, icon, item) -> None:
        self._settings.start_with_windows = not self._settings.start_with_windows
        self._settings.save()
        oskit.get().set_autostart(self._settings.start_with_windows, autostart_command())

    def _make_overlay_pos_setter(self, pos: str):
        def _set(icon, item):
            self._settings.overlay_position = pos
            self._settings.save()
        return _set

    def _open_dashboard_click(self, icon, item) -> None:
        open_dashboard(self._settings, self._on_settings_changed)

    def _toggle_click(self, icon, item) -> None:
        self._on_toggle()

    def _quit_click(self, icon, item) -> None:
        self._on_quit()
        icon.stop()

    def _set_mode_toggle(self, icon, item) -> None:
        self._settings.hotkey_mode = "toggle"
        self._settings.save()
        self._on_settings_changed()

    def _set_mode_hold(self, icon, item) -> None:
        self._settings.hotkey_mode = "hold"
        self._settings.save()
        self._on_settings_changed()

    def _set_paste_clipboard(self, icon, item) -> None:
        self._settings.auto_paste = True
        self._settings.save()

    def _set_paste_typing(self, icon, item) -> None:
        self._settings.auto_paste = False
        self._settings.save()

    def _toggle_overlay(self, icon, item) -> None:
        self._settings.overlay_enabled = not self._settings.overlay_enabled
        self._settings.save()

    def stop(self) -> None:
        self._theme_stop.set()
        if self._icon:
            self._icon.stop()
