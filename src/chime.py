"""Start/stop feedback blips — synthesized with numpy, played via sounddevice.

Deliberately soft and unhurried (Daniel's feedback: nothing urgent-sounding):
low register (G4/B4 major third), quiet (-24 dB-ish), and a full Hann envelope
so each note swells in and out with no attack spike.

No audio assets to bundle, and it works on every OS sounddevice supports.
Ordering constraint in main.py: the start blip must play BEFORE volume ducking
mutes the output, and the stop blip AFTER it is restored.
"""

import logging

import numpy as np
import sounddevice as sd

log = logging.getLogger(__name__)

_SAMPLE_RATE = 44100
_VOLUME = 0.06


def _tone(freq: float, duration: float) -> np.ndarray:
    t = np.linspace(0, duration, int(_SAMPLE_RATE * duration), endpoint=False)
    wave = np.sin(2 * np.pi * freq * t)
    env = np.hanning(len(wave))  # swell in/out — soft, no click, no urgency
    return (wave * env * _VOLUME).astype(np.float32)


_START = np.concatenate([_tone(392.0, 0.12), _tone(494.0, 0.16)])  # G4 → B4, gentle rise
_STOP = np.concatenate([_tone(494.0, 0.12), _tone(392.0, 0.16)])   # B4 → G4, gentle fall


def _play(data: np.ndarray, blocking: bool) -> None:
    try:
        sd.play(data, _SAMPLE_RATE)
        if blocking:
            sd.wait()
    except Exception as e:
        log.debug("Chime playback failed: %s", e)


def play_start(blocking: bool = False) -> None:
    """Start blip. blocking=True guarantees it finishes BEFORE volume ducking
    mutes the output (a non-blocking start blip raced the mute and was
    sometimes inaudible)."""
    _play(_START, blocking)


def play_stop(blocking: bool = False) -> None:
    _play(_STOP, blocking)
