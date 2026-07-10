"""Local transcript history — JSONL file in the config dir, pruned to a cap.

One JSON object per line: {"ts", "text", "duration", "model", "lang"}.
Append-only in normal use; delete/clear rewrite the file atomically.
100% local, like everything else in Eqho.
"""

import json
import logging
import threading
import time
from datetime import datetime
from pathlib import Path

from .settings import CONFIG_DIR

log = logging.getLogger(__name__)

HISTORY_FILE = CONFIG_DIR / "history.jsonl"
MAX_ENTRIES = 1000


class TranscriptHistory:
    """Thread-safe accessor for the history file (appends come from the
    hotkey thread, reads from the dashboard thread)."""

    def __init__(self, path: Path = HISTORY_FILE, max_entries: int = MAX_ENTRIES):
        self._path = path
        self._max = max_entries
        self._lock = threading.Lock()

    def append(self, text: str, duration: float = 0.0, model: str = "", lang: str = "") -> None:
        if not text:
            return
        entry = {
            "ts": time.time(),
            "text": text,
            "duration": round(duration, 2),
            "model": model,
            "lang": lang,
        }
        with self._lock:
            try:
                self._path.parent.mkdir(parents=True, exist_ok=True)
                with self._path.open("a", encoding="utf-8") as f:
                    f.write(json.dumps(entry, ensure_ascii=False) + "\n")
                self._prune_locked()
            except Exception as e:
                log.debug("History append failed: %s", e)

    def read_all(self) -> list[dict]:
        """All entries, newest first."""
        with self._lock:
            return list(reversed(self._read_locked()))

    def search(self, query: str) -> list[dict]:
        """Case-insensitive substring search, newest first."""
        q = query.strip().lower()
        entries = self.read_all()
        if not q:
            return entries
        return [e for e in entries if q in e.get("text", "").lower()]

    def delete(self, ts: float) -> None:
        with self._lock:
            entries = [e for e in self._read_locked() if e.get("ts") != ts]
            self._write_locked(entries)

    def clear(self) -> None:
        with self._lock:
            try:
                self._path.unlink(missing_ok=True)
            except Exception as e:
                log.debug("History clear failed: %s", e)

    def export_txt(self, dest: Path) -> int:
        """Write a human-readable .txt (newest first). Returns entry count."""
        entries = self.read_all()
        lines = []
        for e in entries:
            stamp = datetime.fromtimestamp(e.get("ts", 0)).strftime("%Y-%m-%d %H:%M")
            lines.append(f"[{stamp}] {e.get('text', '')}")
        dest.write_text("\n".join(lines) + ("\n" if lines else ""), encoding="utf-8")
        return len(entries)

    # -- internals (call with self._lock held) --------------------------------

    def _read_locked(self) -> list[dict]:
        if not self._path.exists():
            return []
        out = []
        try:
            for line in self._path.read_text(encoding="utf-8").splitlines():
                line = line.strip()
                if not line:
                    continue
                try:
                    out.append(json.loads(line))
                except json.JSONDecodeError:
                    continue
        except Exception as e:
            log.debug("History read failed: %s", e)
        return out

    def _prune_locked(self) -> None:
        entries = self._read_locked()
        if len(entries) > self._max:
            self._write_locked(entries[-self._max:])

    def _write_locked(self, entries: list[dict]) -> None:
        try:
            if not entries:
                self._path.unlink(missing_ok=True)
                return
            tmp = self._path.with_suffix(".jsonl.tmp")
            tmp.write_text(
                "\n".join(json.dumps(e, ensure_ascii=False) for e in entries) + "\n",
                encoding="utf-8",
            )
            tmp.replace(self._path)
        except Exception as e:
            log.debug("History write failed: %s", e)
