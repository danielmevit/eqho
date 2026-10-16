"""Probe: model-host timeout behaviour with a FAKE child (no real model).

Covers the 2026-10-12 review findings F3/F8 in src/model_host.py:

(a) after a forced transcribe timeout the child is killed, so its late
    reply can never be read; a dead child surfaces an error (not a silent
    empty result); after the respawn-on-next-load the next transcribe gets
    its OWN answer.
(b) a load that keeps reporting progress does NOT time out, even when the
    total wait far exceeds the inactivity window (the scaled-down stand-in
    for a first-run download outlasting the old fixed 600 s limit).
(c) a silent load (no progress messages) DOES time out after the
    inactivity window, and the child is killed.

Exit 0 on success, 1 on any failure. Run with the project venv:

    venv/Scripts/python.exe tools/probe_host_timeout.py
"""

import multiprocessing
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import _probe_env  # noqa: F401,E402  isolated EQHO_CONFIG_DIR before any src import

import src.model_host as mh
from src.model_host import ModelHost, ModelHostError

# Scaled-down timings — the real constants are minutes, the logic is identical.
TRANSCRIBE_TIMEOUT = 2.0
LOAD_INACTIVITY = 1.5


def _fake_worker(to_child, from_child, backend):
    """Fake model-host child. The load message's `model_dir` field selects the
    scenario: "stale" | "normal" | "slow-progress" | "silent"."""
    mode = None
    while True:
        try:
            msg = to_child.get()
        except Exception:
            break
        kind = msg[0]
        if kind == "shutdown":
            break
        if kind == "load":
            mode = msg[1]["model_dir"]
            if mode == "silent":
                time.sleep(60)  # never ready, never reports progress
                continue
            if mode == "slow-progress":
                # Report progress for far longer than the inactivity window;
                # the parent must keep waiting as long as progress flows.
                for i in range(6):
                    time.sleep(LOAD_INACTIVITY * 0.8)
                    from_child.put(("progress", f"downloading {i + 1}/6"))
            from_child.put(("ready", {"backend": "fake", "mode": mode}))
        elif kind == "transcribe":
            if mode == "stale":
                # Outlast the parent's transcribe timeout, then answer late —
                # the reply the OLD code would have handed to the NEXT request.
                time.sleep(TRANSCRIBE_TIMEOUT * 5)
                from_child.put(("segments", [{"text": "STALE REPLY"}]))
            else:
                from_child.put(("segments", [{"text": "FRESH"}]))


class FakeHost(ModelHost):
    """ModelHost wired to the fake child — no real model, no real backend."""

    def _spawn(self) -> None:
        self._to = self._ctx.Queue()
        self._from = self._ctx.Queue()
        self._proc = self._ctx.Process(
            target=_fake_worker, args=(self._to, self._from, self._backend),
            daemon=True)
        self._proc.start()


def _check(failures, ok, label):
    print(("PASS" if ok else "FAIL"), "-", label)
    if not ok:
        failures.append(label)


def main() -> int:
    # Shrink the real timeouts so the probe runs in seconds, not minutes.
    mh._TRANSCRIBE_TIMEOUT = TRANSCRIBE_TIMEOUT
    mh._LOAD_INACTIVITY_TIMEOUT = LOAD_INACTIVITY
    failures = []
    host = FakeHost()

    # -- (a) forced transcribe timeout -> kill, no stale reply, own answer --
    if not host.load_model("fake", "stale"):
        _check(failures, False, "(a) initial load failed")
    t0 = time.monotonic()
    try:
        host.transcribe(b"", "en", 1)
        _check(failures, False, "(a) timed-out transcribe returned instead of raising")
    except ModelHostError as e:
        elapsed = time.monotonic() - t0
        _check(failures, "timeout" in str(e), f"(a) transcribe raised on timeout ({e})")
        _check(failures, TRANSCRIBE_TIMEOUT <= elapsed < TRANSCRIBE_TIMEOUT * 3,
               f"(a) transcribe timed out promptly ({elapsed:.1f}s)")
    _check(failures, not host.is_ready("fake"), "(a) child killed after timeout")
    # A dead child must surface an error, not a silent empty result.
    try:
        host.transcribe(b"", "en", 1)
        _check(failures, False, "(a) transcribe on dead child returned silently")
    except ModelHostError:
        _check(failures, True, "(a) dead child surfaces an error")
    # Respawn on next load; the next transcribe must get its OWN answer.
    if host.load_model("fake", "normal"):
        _check(failures, True, "(a) respawn on next load")
    else:
        _check(failures, False, "(a) respawn on next load")
    segs = host.transcribe(b"", "en", 1)
    text = " ".join(s["text"] for s in segs)
    _check(failures, "FRESH" in text and "STALE" not in text,
           f"(a) next transcribe got its OWN answer ({text!r})")
    host.shutdown()

    # -- (b) load with continuous progress past the limit -> no timeout ----
    host = FakeHost()
    t0 = time.monotonic()
    ok = host.load_model("fake", "slow-progress")
    elapsed = time.monotonic() - t0
    _check(failures, ok, "(b) load with progress did not time out")
    _check(failures, elapsed > LOAD_INACTIVITY * 3,
           f"(b) load waited past the inactivity window ({elapsed:.1f}s > "
           f"{LOAD_INACTIVITY}s window)")
    host.shutdown()

    # -- (c) silent load -> inactivity timeout, child killed ---------------
    host = FakeHost()
    t0 = time.monotonic()
    ok = host.load_model("fake", "silent")
    elapsed = time.monotonic() - t0
    _check(failures, not ok, "(c) silent load timed out")
    _check(failures, LOAD_INACTIVITY <= elapsed < LOAD_INACTIVITY * 4,
           f"(c) silent load failed after the inactivity window ({elapsed:.1f}s)")
    _check(failures, host._proc is None, "(c) child killed after silent timeout")
    host.shutdown()

    print()
    if failures:
        print(f"probe_host_timeout: {len(failures)} FAILURE(S)")
        return 1
    print("probe_host_timeout: all checks passed")
    return 0


if __name__ == "__main__":
    multiprocessing.freeze_support()
    sys.exit(main())
