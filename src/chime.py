"""Start/stop feedback blips — synthesized with numpy, played via sounddevice.

No audio assets to bundle, and it works on every OS sounddevice supports.
Note the ordering constraint in main.py: the start blip must play BEFORE
volume ducking mutes the output, and the stop blip AFTER it is restored.
"""

import logging

import numpy as np
import sounddevice as sd

log = logging.getLogger(__name__)

_SAMPLE_RATE = 44100
_VOLUME = 0.18


def _tone(freq: float, duration: float) -> np.ndarray:
    t = np.linspace(0, duration, int(_SAMPLE_RATE * duration), endpoint=False)
    wave = np.sin(2 * np.pi * freq * t)
    fade = max(1, int(_SAMPLE_RATE * 0.012))  # de-click envelope
    env = np.ones_like(wave)
    env[:fade] = np.linspace(0.0, 1.0, fade)
    env[-fade:] = np.linspace(1.0, 0.0, fade)
    return (wave * env * _VOLUME).astype(np.float32)


_START = np.concatenate([_tone(660, 0.07), _tone(880, 0.09)])  # rising: listening
_STOP = np.concatenate([_tone(880, 0.07), _tone(660, 0.09)])   # falling: done


def _play(data: np.ndarray) -> None:
    try:
        sd.play(data, _SAMPLE_RATE)  # non-blocking
    except Exception as e:
        log.debug("Chime playback failed: %s", e)


def play_start() -> None:
    _play(_START)


def play_stop() -> None:
    _play(_STOP)
