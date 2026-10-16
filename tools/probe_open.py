"""Probe: second-launch 'show' handshake + plain mic-blocked messages (EQ-OPEN).

1. Single-instance lock socket on a free test port (never the real 48317):
   'show' from a second launch fires the open-dashboard callback exactly
   once; junk bytes are ignored and don't kill the listener; with no
   listener the client reports failure in under 3 s.
2. Mic failure messages with a fake OsKit (no GUI, no real registry, never
   opens Settings): blocked -> privacy message + settings page opened once
   per run; unknown/not blocked -> plain 'No microphone found' message;
   the technical error stays in the log, never in the tray. The single
   teardown path still unmutes, hides the overlay and resets the tray.

Exit 0 on success, 1 on any failure. Run with the project venv:

    venv/Scripts/python.exe tools/probe_open.py
"""

import logging
import socket
import sys
import threading
import time
from pathlib import Path
from types import SimpleNamespace

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import _probe_env  # noqa: F401,E402  isolated EQHO_CONFIG_DIR before any src import

import src.model_host as model_host_mod
import src.oskit as oskit_mod
import src.transcriber as transcriber_mod
import src.main as main_mod

PRIVACY_MESSAGE = ("Windows is blocking the microphone. Turn on microphone "
                   "access for desktop apps — the settings page is opening now.")
NO_MIC_MESSAGE = ("No microphone found. Plug one in, or choose another one in "
                  "Eqho → General → Microphone.")
TECHNICAL_ERROR = ("Error opening InputStream: Unanticipated host error "
                   "[PaErrorCode -9999]")


# -- Fakes --------------------------------------------------------------------


class FakeKit:
    """Stands in for the oskit: volume ducking + mic privacy, all recorded.
    open_mic_privacy_settings() only counts — Settings is never opened."""

    def __init__(self, blocked=None):
        self.blocked = blocked  # what mic_access_blocked() reports
        self.opens = 0
        self.volume = 0.8
        self.muted = False
        self.calls = []

    def get_volume(self):
        return self.volume

    def set_volume(self, v):
        self.calls.append(("set_volume", v))
        self.volume = v
        return True

    def set_mute(self, m):
        self.calls.append(("set_mute", m))
        self.muted = m
        return True

    def disable_os_mic_ducking(self):
        pass

    def get_window_process_name(self, hwnd):
        return None

    def mic_access_blocked(self):
        return self.blocked

    def open_mic_privacy_settings(self):
        self.opens += 1
        return True


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
    """No child process, no real model — the worker never starts anyway
    (the mic fails to open before it is spawned)."""

    def __init__(self, backend="fake"):
        self.backend = backend
        self.ready = False

    def is_ready(self, model_size):
        return self.ready

    def load_model(self, model_size, model_dir):
        return True

    def transcribe(self, audio, language, beam_size, initial_prompt=""):
        return []

    def set_backend(self, backend):
        self.backend = backend

    def shutdown(self):
        pass


class FailingInputStream:
    """A mic stream that can't open — the Windows privacy-switch failure."""

    def __init__(self, **kwargs):
        raise Exception(TECHNICAL_ERROR)


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


def _wait_for(cond, timeout):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if cond():
            return True
        time.sleep(0.02)
    return cond()


def _send_raw(port, payload):
    with socket.create_connection(("127.0.0.1", port), timeout=3.0) as s:
        s.sendall(payload)


def _build_app(kit):
    """Patch in all fakes, then build the real App (mic always fails)."""
    oskit_mod.get = lambda: kit
    model_host_mod.ModelHost = FakeModelHost
    model_host_mod.resolve_backend = lambda setting: "fake"
    transcriber_mod.sd = SimpleNamespace(InputStream=FailingInputStream)

    main_mod.TranscriptionOverlay = FakeOverlay
    main_mod.TrayApp = FakeTray
    main_mod.HotkeyManager = FakeHotkey
    main_mod.get_foreground_window = lambda: 12345

    app = main_mod.App()
    # Deterministic settings for the scenarios (in-memory only, never saved).
    app.settings.sound_feedback = False
    app.settings.volume_duck = "mute"
    app.settings.voice_commands = False
    app.settings.history_enabled = False
    app.settings.format_cleanup = False
    app.settings.replacements = {}
    return app


# -- Scenarios -----------------------------------------------------------------


def scenario_listener(failures):
    """The lock socket: 'show' opens the dashboard, junk is ignored, a
    missing listener fails the client fast."""
    print("Second-launch handshake on a free test port...")
    ok = main_mod._acquire_single_instance(port=0)  # OS picks a free port
    _check(failures, ok, "lock acquired on a free test port")
    sock = main_mod._single_instance_socket
    port = sock.getsockname()[1]
    _check(failures, port != main_mod.SINGLE_INSTANCE_PORT,
           "test port is not the real lock port")
    _check(failures, not main_mod._acquire_single_instance(port=port),
           "a second bind on the same port is refused (lock works)")

    fired = []

    def on_show():
        fired.append(1)

    main_mod._start_instance_listener(sock, on_show)

    _check(failures, main_mod._request_show(port), "client delivers 'show'")
    _wait_for(lambda: len(fired) == 1, 5.0)
    _check(failures, len(fired) == 1, "'show' fired the open-dashboard callback once")

    _send_raw(port, b"junk-bytes-not-show")
    time.sleep(0.3)
    _check(failures, len(fired) == 1, "junk bytes ignored (callback not fired)")

    _send_raw(port, b"show" + b"X" * 60)  # 'show' plus trailing bytes
    time.sleep(0.3)
    _check(failures, len(fired) == 1, "'show' with trailing bytes ignored")

    _check(failures, main_mod._request_show(port), "client delivers 'show' again")
    _wait_for(lambda: len(fired) == 2, 5.0)
    _check(failures, len(fired) == 2, "listener still serving after junk bytes")

    # No listener on the port: the client must report failure fast.
    probe = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    probe.bind(("127.0.0.1", 0))
    dead_port = probe.getsockname()[1]
    probe.close()
    t0 = time.monotonic()
    failed = not main_mod._request_show(dead_port)
    elapsed = time.monotonic() - t0
    _check(failures, failed, "no listener -> client reports failure")
    _check(failures, elapsed < 3.0, f"failure came fast ({elapsed:.2f}s < 3s)")

    sock.close()  # stops the listener thread
    main_mod._single_instance_socket = None


def scenario_mic_messages(failures, capture):
    """Mic can't open: plain words in the tray, technical error in the log,
    privacy settings opened at most once, teardown always resets."""
    print("Mic failure messages with a fake OsKit...")
    kit = FakeKit(blocked=True)
    app = _build_app(kit)

    app.activate()  # mic fails; the Windows privacy switch says "Deny"
    notes = app.tray.notifications
    _check(failures, notes and notes[-1] == PRIVACY_MESSAGE,
           "blocked mic -> privacy message in the tray")
    _check(failures, kit.opens == 1, "privacy settings page opened once")
    _check(failures, not app.overlay.visible, "overlay hidden after mic failure")
    _check(failures, app.tray.active is False, "tray reset after mic failure")
    _check(failures, not kit.muted, "system audio unmuted after mic failure")
    _check(failures, ("set_volume", 0.8) in kit.calls,
           "volume restored after mic failure")

    app.activate()  # second hotkey press: same message, settings NOT reopened
    _check(failures, kit.opens == 1, "settings page never opened twice")
    _check(failures, app.tray.notifications[-1] == PRIVACY_MESSAGE,
           "blocked mic -> privacy message again on re-press")

    kit.blocked = None  # privacy state unknown
    app.activate()
    _check(failures, app.tray.notifications[-1] == NO_MIC_MESSAGE,
           "unknown privacy state -> 'No microphone found' message")
    _check(failures, kit.opens == 1, "settings page not opened when unknown")

    kit.blocked = False  # clearly allowed — the mic itself is the problem
    app.activate()
    _check(failures, app.tray.notifications[-1] == NO_MIC_MESSAGE,
           "not blocked -> 'No microphone found' message")

    joined = "\n".join(capture.messages)
    _check(failures, TECHNICAL_ERROR in joined, "technical error kept in the log")
    _check(failures, all(TECHNICAL_ERROR not in n for n in app.tray.notifications),
           "technical error never shown to the user")


def main() -> int:
    print("Running open/mic probe (EQ-OPEN)...")
    # Attach the capture handler AFTER src.main's basicConfig ran at import.
    logging.getLogger().setLevel(logging.INFO)
    capture = _Capture()
    logging.getLogger().addHandler(capture)

    failures = []
    scenario_listener(failures)
    scenario_mic_messages(failures, capture)

    print()
    if failures:
        print(f"probe_open: {len(failures)} FAILURE(S)")
        return 1
    print("probe_open: ALL OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())
