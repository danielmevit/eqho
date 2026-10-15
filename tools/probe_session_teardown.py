"""Probe: dictation-session teardown with FAKES (no real model/audio/hotkeys).

Covers the 2026-10-12 review findings F1, F2, F10 in src/main.py and
src/transcriber.py:

(a) model-load failure in toggle mode -> the app's single teardown path
    restores system volume (unducks/unmutes), hides the overlay, resets the
    tray and notifies the user — audio never stays muted.
(b) a final transcription slower than stop()'s flush window -> its text is
    dropped (the session is closed) and can NOT land in the next dictation;
    the next session's own text IS delivered and injected.
(c) no transcript text appears in captured log output — only lengths.

The real App and VoiceTranscriber run unmodified; the model host, mic
stream, overlay, tray, hotkeys, injector and OS volume kit are fakes.

Exit 0 on success, 1 on any failure. Run with the project venv:

    venv/Scripts/python.exe tools/probe_session_teardown.py
"""

import logging
import sys
import threading
import time
from pathlib import Path
from types import SimpleNamespace

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

import numpy as np

import src.model_host as model_host_mod
import src.oskit as oskit_mod
import src.transcriber as transcriber_mod

SENTINEL_LATE = "LATE TEXT FROM SESSION ONE"
SENTINEL_FRESH = "SECOND SESSION TEXT"


# -- Fakes --------------------------------------------------------------------


class FakeVolumeKit:
    """Stands in for the oskit volume controller; records every call."""

    def __init__(self):
        self.volume = 0.8
        self.muted = False
        self.calls = []

    def get_volume(self):
        self.calls.append(("get_volume", None))
        return self.volume

    def set_volume(self, v):
        self.calls.append(("set_volume", v))
        self.volume = v
        return True

    def set_mute(self, m):
        self.calls.append(("set_mute", m))
        self.muted = m

    def disable_os_mic_ducking(self):
        pass

    def get_window_process_name(self, hwnd):
        return None


class FakeOverlay:
    def __init__(self, settings):
        self.visible = False
        self.text = ""

    def start(self):
        pass

    def show(self, text=""):
        self.visible = True
        self.text = text

    def update_text(self, text):
        self.text = text

    def set_level(self, level):
        pass

    def hide(self):
        self.visible = False

    def shutdown(self):
        pass


class FakeTray:
    def __init__(self, settings, on_toggle=None, on_quit=None,
                 on_settings_changed=None, on_restart=None):
        self.active = None
        self.notifications = []

    def set_active(self, active):
        self.active = active

    def notify(self, message):
        self.notifications.append(message)

    def run(self):
        pass


class FakeHotkey:
    def __init__(self, settings, on_activate=None, on_deactivate=None,
                 on_toggle=None):
        pass

    def register(self):
        pass

    def unregister(self):
        pass


class FakeModelHost:
    """Stands in for ModelHost: no child process, no real model. Transcribe
    responses come from a queue; a callable response is invoked (used for a
    transcription that outlasts stop()'s flush window)."""

    def __init__(self, backend="fake"):
        self.backend = backend
        self.info = {"backend": "fake"}
        self.ready = False
        self.load_ok = True
        self._responses = []
        self._lock = threading.Lock()

    def set_responses(self, responses):
        with self._lock:
            self._responses = list(responses)

    def is_ready(self, model_size):
        return self.ready

    def load_model(self, model_size, model_dir):
        return self.load_ok

    def transcribe(self, audio, language, beam_size, initial_prompt=""):
        with self._lock:
            response = self._responses.pop(0) if self._responses else None
        if callable(response):
            response = response()
        if not response:
            return []
        return [{"text": response, "no_speech_prob": 0.0, "avg_logprob": 0.0}]

    def set_backend(self, backend):
        self.backend = backend

    def shutdown(self):
        pass


class FakeInputStream:
    """A mic stream that produces no audio; the probe pushes chunks into the
    transcriber's queue directly."""

    def __init__(self, **kwargs):
        self.callback = kwargs.get("callback")

    def start(self):
        pass

    def stop(self):
        pass

    def close(self):
        pass


class _Capture(logging.Handler):
    def __init__(self):
        super().__init__()
        self.messages = []

    def emit(self, record):
        self.messages.append(record.getMessage())


# -- Helpers -------------------------------------------------------------------


def _check(failures, ok, label):
    print(("PASS" if ok else "FAIL"), "-", label)
    if not ok:
        failures.append(label)


def _push_audio(transcriber, seconds=0.5, amplitude=0.05):
    """One loud chunk — enough to pass the near-silence gate and the
    MIN_PHRASE_DURATION flush threshold."""
    n = int(transcriber_mod.SAMPLE_RATE * seconds)
    transcriber._audio_q.put(np.full(n, amplitude, dtype=np.float32))


def _build_app(kit, injected):
    """Patch in all fakes, then build the real App."""
    oskit_mod.get = lambda: kit
    model_host_mod.ModelHost = FakeModelHost
    model_host_mod.resolve_backend = lambda setting: "fake"
    transcriber_mod.sd = SimpleNamespace(InputStream=FakeInputStream)
    transcriber_mod.STOP_JOIN_TIMEOUT = 0.5  # shrink the flush window

    import src.main as main_mod
    main_mod.TranscriptionOverlay = FakeOverlay
    main_mod.TrayApp = FakeTray
    main_mod.HotkeyManager = FakeHotkey
    main_mod.get_foreground_window = lambda: 12345
    main_mod.set_foreground_window = lambda hwnd: None
    main_mod.send_backspaces = lambda n: None
    main_mod.type_text = lambda text, use_clipboard=True: injected.append(text)
    main_mod.chime = SimpleNamespace(play_start=lambda **kw: None,
                                     play_stop=lambda: None)

    app = main_mod.App()
    # Deterministic settings for the scenarios (in-memory only, never saved).
    app.settings.volume_duck = "mute"
    app.settings.sound_feedback = False
    app.settings.voice_commands = False
    app.settings.history_enabled = False
    app.settings.format_cleanup = False
    app.settings.replacements = {}
    return app


# -- Scenarios -----------------------------------------------------------------


def scenario_model_load_failure(app, kit, failures):
    """(a) toggle mode + duck=mute + model-load failure -> volume restored,
    overlay hidden, tray reset, user notified."""
    host = app.transcriber._host
    host.ready = False
    host.load_ok = False

    app.activate()  # ducks (mutes) system audio after start
    worker = app.transcriber._worker
    worker.join(timeout=10)
    time.sleep(0.1)

    mutes = [c for c in kit.calls if c[0] == "set_mute"]
    _check(failures, ("set_mute", True) in mutes,
           "(a) activation muted system audio")
    _check(failures, ("set_mute", False) in mutes,
           "(a) teardown unmuted system audio")
    _check(failures, ("set_volume", 0.8) in kit.calls and kit.volume == 0.8,
           "(a) volume restored to the pre-duck level")
    _check(failures, not kit.muted, "(a) audio is not left muted")
    _check(failures, not app.overlay.visible, "(a) overlay hidden after failure")
    _check(failures, app.tray.active is False, "(a) tray reset after failure")
    _check(failures, any("Model failed to load" in n for n in app.tray.notifications),
           "(a) load failure surfaced to the user via tray")
    _check(failures, not app.transcriber.is_running(),
           "(a) transcriber no longer running")


def scenario_slow_final_flush(app, kit, failures, capture):
    """(b) a final transcription slower than stop()'s flush window is dropped
    and never lands in the next dictation; the next session's own text is
    delivered and injected."""
    host = app.transcriber._host
    host.ready = True
    host.load_ok = True

    # Session 1: the final flush blocks past the (shrunk) join window.
    late_gate = threading.Event()

    def _slow_transcribe():
        late_gate.wait(timeout=30)
        return SENTINEL_LATE

    host.set_responses([_slow_transcribe])
    app.activate()
    _push_audio(app.transcriber)
    time.sleep(0.3)  # let the worker consume the chunk
    worker = app.transcriber._worker
    app.deactivate()  # flush window times out -> session closed
    _check(failures, app._pending_text == [],
           "(b) nothing pending right after the timed-out stop")

    late_gate.set()  # the slow transcription finally completes...
    worker.join(timeout=10)
    time.sleep(0.1)
    _check(failures, app._pending_text == [],
           "(b) late session-1 text NOT delivered to the app")
    _check(failures, any("Dropped result from closed session" in m
                         for m in capture.messages),
           "(b) late result dropped by the session check")

    # Session 2: its own text must arrive normally.
    host.set_responses([SENTINEL_FRESH])
    app.activate()
    _push_audio(app.transcriber)
    time.sleep(0.3)
    app.deactivate()  # flush completes within the window -> delivered
    _check(failures, app._pending_text == [],
           "(b) session-2 pending list drained after injection")


def scenario_no_transcripts_in_logs(failures, capture, injected):
    """(c) captured log output contains lengths only, never transcript text."""
    joined = "\n".join(capture.messages)
    for sentinel, label in ((SENTINEL_LATE, "session-1 late text"),
                            (SENTINEL_FRESH, "session-2 text")):
        _check(failures, sentinel not in joined,
               f"(c) transcript text ({label}) absent from captured logs")
    _check(failures, any("Complete transcript:" in m and "chars" in m
                         for m in capture.messages),
           "(c) transcript length IS logged")
    _check(failures, any("Injected" in m and "chars" in m
                         for m in capture.messages),
           "(c) injected length IS logged")


def main() -> int:
    kit = FakeVolumeKit()
    injected = []

    app = _build_app(kit, injected)  # imports src.main -> logging configured
    # Attach the capture handler AFTER src.main's basicConfig, and pin the
    # root level (basicConfig is a no-op once a handler exists).
    logging.getLogger().setLevel(logging.INFO)
    capture = _Capture()
    logging.getLogger().addHandler(capture)

    failures = []

    scenario_model_load_failure(app, kit, failures)
    scenario_slow_final_flush(app, kit, failures, capture)

    # (b) continued: the next session received ONLY its own text.
    _check(failures, injected == [SENTINEL_FRESH],
           f"(b) next session injected only its own text (got {injected!r})")
    _check(failures, SENTINEL_LATE not in injected,
           "(b) late session-1 text never injected")

    scenario_no_transcripts_in_logs(failures, capture, injected)

    print()
    if failures:
        print(f"probe_session_teardown: {len(failures)} FAILURE(S)")
        return 1
    print("probe_session_teardown: all checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
