"""Probe: one friendly tip when the chosen model is slow on this PC (EQ-SLOWHINT).

Uses FAKES (no real model): a fake model host whose transcribe() "takes" a
chosen number of seconds (fake clock), fake settings with a counting save().
Checks:
- a 7 s call with a non-recommended model -> one notice, right labels,
  setting saved;
- a second slow call -> no second notice;
- a fast call -> nothing;
- slow but already on the recommended model -> nothing;
- after switching to another non-recommended model -> one new notice.

One line per check; exit 1 on any failure. Run with the project venv:

    venv/Scripts/python.exe tools/probe_slowhint.py
"""

import logging
import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import _probe_env  # noqa: F401,E402  isolated EQHO_CONFIG_DIR before any src import

import src.model_host as model_host_mod
import src.oskit as oskit_mod
import src.transcriber as transcriber_mod
from src.modelpick import recommend_model
from src.settings import Settings

logging.basicConfig(level=logging.INFO, handlers=[logging.NullHandler()])
sys.stdout.reconfigure(errors="backslashreplace")

failures = 0


def check(name: str, ok: bool, detail: str = "") -> None:
    global failures
    if not ok:
        failures += 1
    print(f"[{'PASS' if ok else 'FAIL'}] {name}" + (f" - {detail}" if detail else ""))


class FakeKit:
    def disable_os_mic_ducking(self):
        pass


class Clock:
    now = 0.0


class FakeHost:
    call_s = 0.0

    def __init__(self, backend="faster-whisper"):
        self.backend = backend

    def transcribe(self, audio, language, beam, prompt):
        Clock.now += FakeHost.call_s
        return [{"text": "hello there", "no_speech_prob": 0.0, "avg_logprob": -0.1}]


class FakeSettings(Settings):
    def save(self):
        self.saves = getattr(self, "saves", 0) + 1


oskit_mod.get = lambda: FakeKit()
model_host_mod.resolve_backend = lambda setting: "faster-whisper"
model_host_mod.ModelHost = FakeHost
transcriber_mod.time.perf_counter = lambda: Clock.now

settings = FakeSettings(language="en", model_dir=".")
rec = recommend_model(settings.language)
slow_model = next(k for k in ("distil-large-v3", "large-v3", "medium") if k != rec)
other_model = next(k for k in ("large-v3", "medium", "small") if k not in (rec, slow_model))
settings.model_size = slow_model

tr = transcriber_mod.VoiceTranscriber(settings)
notices = []
tr.set_callbacks(on_partial=lambda t: None, on_complete=lambda t: None,
                 on_notice=notices.append)
audio = np.full(16000 * 3, 0.1, dtype=np.float32)


def run(seconds):
    FakeHost.call_s = seconds
    before = len(notices)
    tr._do_complete(audio, tr.current_session())
    return notices[before:]


def label(key):
    from src.settings import WHISPER_MODELS
    return WHISPER_MODELS[key].split(" (")[0]


new = run(7.0)
want = (f"Eqho is slow on this PC with {label(slow_model)}. {label(rec)} is much "
        "faster — you can switch in Eqho → Models.")
check("slow call on non-recommended model -> one notice", len(new) == 1, f"{len(new)} notices")
check("notice has the right labels", new[:1] == [want], repr(new[:1]))
check("setting remembered and saved",
      settings.slow_hint_shown_for == slow_model and getattr(settings, "saves", 0) == 1,
      f"{settings.slow_hint_shown_for!r}, saves={getattr(settings, 'saves', 0)}")
check("model never changed", settings.model_size == slow_model)

check("second slow call -> no second notice", run(7.0) == [])
check("fast call -> nothing", run(1.0) == [])

settings.model_size = rec
check("slow on the recommended model -> nothing", run(9.0) == [])

settings.model_size = other_model
new = run(7.0)
check("switched to another non-recommended model -> one new notice",
      len(new) == 1 and label(other_model) in new[0], repr(new))
check("again slow -> nothing more", run(7.0) == [])

print("ALL PASS" if not failures else f"{failures} FAILURE(S)")
sys.exit(1 if failures else 0)
