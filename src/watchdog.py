"""UI-freeze watchdog.

Components call beat(name) periodically (the dashboard does, once a second,
from inside its Tk loop). A monitor thread notices when a heartbeat goes
stale and dumps EVERY thread's current stack into the log — so a frozen UI
tells us exactly which line of which thread is stuck instead of being a
mystery.
"""

import logging
import sys
import threading
import time
import traceback

log = logging.getLogger(__name__)

_beats: dict = {}
_lock = threading.Lock()
_started = False
_dumped: set = set()
_STALL_AFTER = 5.0  # seconds without a beat = stalled


def beat(name: str) -> None:
    with _lock:
        _beats[name] = time.monotonic()


def clear(name: str) -> None:
    """Stop watching a heartbeat (component shut down cleanly)."""
    with _lock:
        _beats.pop(name, None)
        _dumped.discard(name)


def dump_stacks(reason: str) -> None:
    """Log the stack of every live thread."""
    names = {t.ident: t.name for t in threading.enumerate()}
    lines = [f"=== THREAD DUMP ({reason}) ==="]
    for tid, frame in sys._current_frames().items():
        lines.append(f"--- thread: {names.get(tid, tid)} ---")
        lines.append("".join(traceback.format_stack(frame)).rstrip())
    log.critical("\n".join(lines))


def start() -> None:
    global _started
    if _started:
        return
    _started = True

    def _monitor() -> None:
        while True:
            time.sleep(2)
            now = time.monotonic()
            with _lock:
                snapshot = dict(_beats)
            for name, last in snapshot.items():
                stale = now - last
                if stale > _STALL_AFTER:
                    if name not in _dumped:
                        _dumped.add(name)
                        dump_stacks(f"'{name}' heartbeat stale for {stale:.1f}s")
                else:
                    _dumped.discard(name)

    threading.Thread(target=_monitor, daemon=True, name="watchdog").start()
