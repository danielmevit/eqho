"""User preferences persisted to a JSON config file."""

import json
import logging
import os
from dataclasses import dataclass, field, asdict
from pathlib import Path
from typing import Optional

import platformdirs

log = logging.getLogger(__name__)

# Config lives in the platform-native config dir. On Windows (roaming=True)
# this resolves to %APPDATA%\Eqho — the same directory Eqho has always used,
# so existing installs keep their settings without migration.
CONFIG_DIR = Path(platformdirs.user_config_dir("Eqho", appauthor=False, roaming=True))
CONFIG_FILE = CONFIG_DIR / "settings.json"

# Pre-0.3.3 installs cached models to a hardcoded D:\EqhoModels. If that
# directory exists, Settings.load() pins it into `model_dir` so the cache
# keeps working; fresh installs use the platform cache dir instead.
LEGACY_MODEL_DIR = Path("D:/EqhoModels")
DEFAULT_MODEL_DIR = Path(platformdirs.user_cache_dir("Eqho", appauthor=False)) / "models"

WHISPER_MODELS = {
    "distil-large-v3": "Distil Large v3 (~1.5 GB, English-optimized, recommended)",
    "distil-medium.en": "Distil Medium EN (~750 MB, fast English)",
    "distil-small.en": "Distil Small EN (~330 MB, fastest English)",
    "large-v3-turbo": "Large v3 Turbo (~1.6 GB, multilingual, near-large accuracy)",
    "medium": "Medium (~1.5 GB, multilingual)",
    "small": "Small (~950 MB, multilingual)",
    "base": "Base (~300 MB, multilingual)",
    "tiny": "Tiny (~150 MB, fastest, least accurate)",
    "large-v3": "Large v3 (~3.1 GB, highest accuracy)",
}

SUPPORTED_LANGUAGES = {
    "en": "English",
    "es": "Spanish",
    "zh": "Mandarin",
    "ja": "Japanese",
    "ko": "Korean",
    "vi": "Vietnamese",
    "ar": "Arabic",
    "uk": "Ukrainian",
    "fr": "French",
    "de": "German",
    "pt": "Portuguese",
    "ru": "Russian",
    "it": "Italian",
}

HOTKEY_MODES = ("toggle", "hold")

VOLUME_DUCK_OPTIONS = {
    "off": None,
    "50%": 0.5,
    "25%": 0.25,
    "10%": 0.10,
    "mute": 0.0,
}

OVERLAY_POSITIONS = [
    "bottom-center",
    "top-center",
    "top-left",
    "top-right",
    "bottom-left",
    "bottom-right",
]


@dataclass
class Settings:
    language: str = "en"
    hotkey: str = "alt+q"
    hotkey_mode: str = "toggle"  # "toggle" or "hold"
    model_size: str = "distil-large-v3"
    model_dir: str = ""  # empty = resolve_model_dir() picks legacy dir or platform cache
    audio_device: Optional[int] = None  # None = system default input device
    auto_paste: bool = True  # paste via clipboard vs simulated keystrokes
    overlay_enabled: bool = True
    overlay_opacity: float = 0.85
    overlay_font_size: int = 14
    overlay_position: str = "bottom-center"
    volume_duck: str = "mute"  # "off", "50%", "25%", "10%", "mute"
    start_with_windows: bool = False
    theme: str = "dark"  # "dark", "light", "system"
    # Local features (v0.5.0)
    history_enabled: bool = True     # save dictations to history.jsonl
    initial_prompt: str = ""         # custom vocabulary bias for Whisper
    replacements: dict = field(default_factory=dict)  # text substitutions
    voice_commands: bool = False     # "new line", "period", "delete that", …
    sound_feedback: bool = True      # start/stop chime

    # runtime-only (not persisted)
    _listeners: list = field(default_factory=list, repr=False)

    def resolve_model_dir(self) -> Path:
        """Directory Whisper models are cached in."""
        if self.model_dir:
            return Path(self.model_dir)
        if LEGACY_MODEL_DIR.exists():
            return LEGACY_MODEL_DIR
        return DEFAULT_MODEL_DIR

    def save(self) -> None:
        CONFIG_DIR.mkdir(parents=True, exist_ok=True)
        data = asdict(self)
        data.pop("_listeners", None)
        CONFIG_FILE.write_text(json.dumps(data, indent=2), encoding="utf-8")

    @classmethod
    def load(cls) -> "Settings":
        settings = None
        if CONFIG_FILE.exists():
            try:
                data = json.loads(CONFIG_FILE.read_text(encoding="utf-8"))
                data.pop("_listeners", None)
                settings = cls(**{k: v for k, v in data.items() if k in cls.__dataclass_fields__})
            except (json.JSONDecodeError, TypeError):
                pass
        if settings is None:
            settings = cls()

        # One-time migration: pin a pre-0.3.3 legacy model cache explicitly so
        # it survives even if the drive layout changes later.
        if not settings.model_dir and LEGACY_MODEL_DIR.exists():
            settings.model_dir = str(LEGACY_MODEL_DIR)
            try:
                settings.save()
                log.info("Pinned legacy model cache %s into settings.", LEGACY_MODEL_DIR)
            except Exception as e:
                log.debug("Could not persist model_dir migration: %s", e)
        return settings

    def add_listener(self, callback) -> None:
        self._listeners.append(callback)

    def notify(self) -> None:
        for cb in self._listeners:
            cb(self)


def is_model_cached(settings: "Settings", model_key: str) -> bool:
    """True if the given Whisper model is already downloaded to the cache dir."""
    cache_dir = settings.resolve_model_dir()
    for candidate in (
        cache_dir / model_key,
        cache_dir / f"models--Systran--faster-whisper-{model_key}",
        cache_dir / f"models--ctranslate2-4you--distil-whisper-{model_key}",
        cache_dir / "huggingface" / f"models--Systran--faster-whisper-{model_key}",
        cache_dir / "huggingface" / f"models--ctranslate2-4you--distil-whisper-{model_key}",
    ):
        if candidate.exists():
            return True
    return False
