#!/usr/bin/env python3
"""EQ-TESTE2E — end-to-end dictation test: drives the WHOLE app like a user.

Starts Eqho (run.py) with EQHO_TEST_AUDIO (a recording instead of the
microphone) and EQHO_CONFIG_DIR (an isolated config dir), plus its own tiny
Tk target window (tools/e2e_target.py). Then it brings the target window to
the front, sends the hotkey (TEST_HOTKEY, see below), waits for the clip to play out, stops
dictation, and checks that the transcript landed in the target window.

WARNING: this test uses the REAL desktop. It sends real hotkey keypresses and
brings its own window to the foreground — do not touch the machine, and do
not run it while another Eqho instance is running (the single-instance lock
would make the test instance exit). Before every keypress it verifies that
its own target window is the foreground window and aborts otherwise, so it
never types into the owner's apps.

Run with the owner's Windows venv (never pip-install into it):

    "venv/Scripts/python.exe" tools/test_e2e.py --clips 3 --gate-wer 0.25

Clips come from LibriSpeech test-clean (see docs/ai/TESTING.md), or one
known file: --audio clip.wav --ref "the reference text".

Exit codes: 0 = pass, 1 = gate/clip failure or safety abort, 2 = setup error.
"""

import argparse
import csv
import json
import logging
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
TOOLS_DIR = Path(__file__).resolve().parent
for _p in (str(REPO_ROOT), str(TOOLS_DIR)):
    if _p not in sys.path:
        sys.path.insert(0, _p)

log = logging.getLogger("test_e2e")

SAMPLE_RATE = 16000
TARGET_TITLE = "Eqho E2E Target"
# Not Alt+Q: the `keyboard` library drops INJECTED key events that carry the
# Alt-down flag (_winkeyboard.py "fake_alt"), so a simulated Alt+Q never reaches
# Eqho. A real, physical Alt+Q is unaffected. The test uses an Alt-free hotkey.
TEST_HOTKEY = "ctrl+shift+f9"
READY_MARKER = "Model pre-loaded and ready."
LOAD_FAIL_MARKER = "Failed to pre-load model"
ALREADY_RUNNING_MARKER = "Eqho is already running"
STARTED_RE = re.compile(r"Transcription started \(session \d+")
INJECTED_RE = re.compile(r"Injected (\d+) chars")
ACTIVATED_RE = re.compile(r"Dictation activated")
ENDED_RE = re.compile(r"Dictation deactivated|Dictation session aborted")
STOPPED_RE = re.compile(r"Transcription stopped\.")

# How long to wait after the clip's audio before stopping dictation: the test
# source feeds silence after the file, so the VAD completes the phrase on its
# own (like a real user falling silent) before we press stop.
TAIL_SILENCE_WAIT = 2.5

CSV_COLUMNS = ["clip", "utt", "audio_s", "ref_words", "sub", "del", "ins", "wer",
               "start_latency_s", "stop_latency_s", "volume_ok", "ref", "hyp", "status"]


class SetupError(Exception):
    """Fatal setup problem -> exit 2."""


class SafetyAbort(Exception):
    """Target window could not be made foreground -> abort before any key."""


class ChildDied(Exception):
    """The Eqho process exited while we were waiting for it."""


# -- Eqho log tailing -----------------------------------------------------------

class LogTail:
    """Incremental reader for the child's eqho.log (complete lines only)."""

    def __init__(self, path: Path) -> None:
        self.path = path
        self._offset = 0
        self._partial = ""
        self.lines: list[str] = []

    def _read_new(self) -> None:
        try:
            if not self.path.exists():
                return
            with self.path.open("rb") as f:
                f.seek(self._offset)
                data = f.read()
                self._offset = f.tell()
        except OSError:
            return
        if not data:
            return
        self._partial += data.decode("utf-8", errors="replace")
        *complete, self._partial = self._partial.split("\n")
        self.lines.extend(line for line in complete if line.strip())

    def count(self, rx: re.Pattern) -> int:
        self._read_new()
        return sum(1 for line in self.lines if rx.search(line))

    def mark(self) -> int:
        """Line index to pass as wait_for(since=...): take it BEFORE the action
        whose log line you wait for (the app can log within milliseconds)."""
        self._read_new()
        return len(self.lines)

    def wait_for(self, rx: re.Pattern, timeout: float,
                 proc: subprocess.Popen = None, since: int = None):
        """Poll until a NEW line matching rx appears (lines before `since`, or
        already seen before this call when `since` is None, never match).
        Returns (detect_time, match) or (None, None) on timeout. Raises
        ChildDied if proc exits first."""
        self._read_new()
        start = len(self.lines) if since is None else since
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            self._read_new()
            for line in self.lines[start:]:
                m = rx.search(line)
                if m:
                    return time.monotonic(), m
            if proc is not None and proc.poll() is not None:
                raise ChildDied(f"Eqho exited with code {proc.returncode}")
            time.sleep(0.05)
        return None, None


# -- System volume (pycaw, same API the app's oskit uses) ------------------------

def volume_state():
    """(master volume scalar 0..1, muted bool) or None if unavailable."""
    try:
        from comtypes import CoInitialize
        from pycaw.pycaw import AudioUtilities
        CoInitialize()
        ctl = AudioUtilities.GetSpeakers().EndpointVolume
        return (ctl.GetMasterVolumeLevelScalar(), bool(ctl.GetMute()))
    except Exception as e:
        log.warning("Volume probe unavailable: %s", e)
        return None


def restore_volume(state) -> None:
    """Safety net: put the system volume back to the given state."""
    try:
        from comtypes import CoInitialize
        from pycaw.pycaw import AudioUtilities
        from comtypes import GUID
        CoInitialize()
        ctl = AudioUtilities.GetSpeakers().EndpointVolume
        ctl.SetMute(False, GUID())
        ctl.SetMasterVolumeLevelScalar(max(0.0, min(1.0, state[0])), GUID())
    except Exception as e:
        log.error("Could not restore system volume: %s", e)


def volume_matches(before, after) -> bool:
    if before is None or after is None:
        return True  # cannot check on this machine — don't fail the run
    # Volume scalar must be back at the starting level. Ending muted is only
    # acceptable if it started muted (the app always unmutes on restore).
    return abs(after[0] - before[0]) <= 0.01 and (not after[1] or before[1])


# -- Window focus (the test never types unless ITS window is foreground) --------

def _user32():
    import ctypes
    return ctypes.windll.user32


def find_window(title: str, expected_pid: int = None, timeout: float = 15.0):
    """hwnd of a top-level window by exact title (optionally owned by
    expected_pid, so a stale window from a crashed earlier run can't be
    mistaken for the fresh target), or None."""
    import ctypes
    user32 = _user32()
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        hwnd = user32.FindWindowW(None, title)
        if hwnd:
            if expected_pid is None:
                return hwnd
            pid = ctypes.c_ulong()
            user32.GetWindowThreadProcessId(hwnd, ctypes.byref(pid))
            if pid.value == expected_pid:
                return hwnd
        time.sleep(0.1)
    return None


def bring_to_front(hwnd, timeout: float = 8.0) -> bool:
    """Make hwnd the foreground window; False if the OS refuses for good."""
    user32 = _user32()
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if user32.IsIconic(hwnd):
            user32.ShowWindow(hwnd, 9)  # SW_RESTORE
        user32.SetForegroundWindow(hwnd)
        if user32.GetForegroundWindow() == hwnd:
            return True
        # Foreground-lock workaround: a synthetic ALT press grants this
        # process the right to set the foreground window.
        user32.keybd_event(0x12, 0, 0, 0)  # VK_MENU down
        user32.keybd_event(0x12, 0, 2, 0)  # VK_MENU up
        time.sleep(0.1)
    return user32.GetForegroundWindow() == hwnd


def ensure_foreground(hwnd) -> None:
    """Abort the whole run (before pressing anything) if hwnd isn't front."""
    user32 = _user32()
    if user32.GetForegroundWindow() == hwnd:
        return
    if not bring_to_front(hwnd):
        raise SafetyAbort(
            f"target window {hwnd} could not be made foreground "
            f"(current foreground: {_user32().GetForegroundWindow()}) — "
            "aborting before pressing any key")


# -- Clips ------------------------------------------------------------------------

def build_clips(args) -> list:
    if args.audio:
        if not args.ref:
            raise SetupError("--audio requires --ref \"<reference text>\"")
        path = Path(args.audio)
        if not path.exists():
            raise SetupError(f"--audio file not found: {path}")
        return [{"utt": path.stem, "path": path, "ref": args.ref}]
    from bench_asr import (DEFAULT_DATA_WIN, DEFAULT_DATA_WSL,
                           collect_utterances, pick_subset)
    data_dir = (Path(args.data) if args.data else
                (DEFAULT_DATA_WIN if DEFAULT_DATA_WIN.exists() else DEFAULT_DATA_WSL))
    if not data_dir.exists():
        raise SetupError(f"LibriSpeech test-clean not found at {data_dir} (use --data)")
    utts = collect_utterances(data_dir)
    if not utts:
        raise SetupError(f"No utterances found under {data_dir}")
    subset = pick_subset(utts, args.clips)
    log.info("Dataset: %d utterances, picked %d (round-robin over speakers).",
             len(utts), len(subset))
    return [{"utt": u["utt_id"], "path": u["flac"], "ref": u["text"]}
            for u in subset]


def clip_duration(path: Path, cache: dict) -> float:
    if path not in cache:
        from bench_asr import decode_flac
        audio = decode_flac(path)
        cache[path] = len(audio) / SAMPLE_RATE
    return cache[path]


# -- One clip ---------------------------------------------------------------------

def run_clip(index: int, clip: dict, *, work: Path, swap: Path, tail: LogTail,
             proc: subprocess.Popen, vol0, durations: dict) -> dict:
    from bench_asr import normalize_words, wer_counts
    import keyboard

    out_txt = work / f"clip{index:02d}.txt"
    stopfile = work / f"clip{index:02d}.stop"
    for p in (out_txt, stopfile):
        p.unlink(missing_ok=True)

    # Swap the test audio BEFORE the hotkey: the source reads the file when
    # dictation starts (Eqho is idle right now, so this is race-free).
    shutil.copyfile(clip["path"], swap)
    duration = clip_duration(clip["path"], durations)
    log.info("[clip %d] %s: %.1fs audio", index, clip["utt"], duration)

    # A Windows venv's python.exe is a launcher that runs the base interpreter
    # as a CHILD process, so the window's pid would never equal target.pid.
    # The target needs only the stdlib (tkinter), so start the base interpreter.
    target = subprocess.Popen(
        [getattr(sys, "_base_executable", sys.executable), str(TOOLS_DIR / "e2e_target.py"),
         "--out", str(out_txt), "--stopfile", str(stopfile),
         "--title", TARGET_TITLE])
    row = {"clip": index, "utt": clip["utt"], "audio_s": round(duration, 3),
           "ref_words": 0, "sub": 0, "del": 0, "ins": 0, "wer": "",
           "start_latency_s": "", "stop_latency_s": "", "volume_ok": "",
           "ref": clip["ref"], "hyp": ""}
    try:
        hwnd = find_window(TARGET_TITLE, expected_pid=target.pid)
        if not hwnd:
            raise SafetyAbort("target window never appeared — aborting")
        mark = tail.mark()  # BEFORE the foreground check: nothing between check and keypress
        ensure_foreground(hwnd)

        # the hotkey (start) -> "listening"
        t_alt1 = time.monotonic()
        keyboard.send(TEST_HOTKEY)
        t_started, _ = tail.wait_for(STARTED_RE, 15.0, proc, since=mark)
        if t_started is None:
            raise RuntimeError("dictation did not start (no 'Transcription started' in the log)")
        row["start_latency_s"] = round(t_started - t_alt1, 3)
        log.info("[clip %d] start latency: %.2fs", index, t_started - t_alt1)

        # Let the clip play out in real time (the VAD completes the phrase
        # during the trailing silence), then the hotkey (stop) -> text appears.
        time.sleep(duration + TAIL_SILENCE_WAIT)
        mark = tail.mark()  # BEFORE the foreground check: nothing between check and keypress
        ensure_foreground(hwnd)
        t_alt2 = time.monotonic()
        keyboard.send(TEST_HOTKEY)
        t_injected, m = tail.wait_for(INJECTED_RE, 45.0, proc, since=mark)
        if t_injected is None:
            raise RuntimeError("no text was injected (no 'Injected N chars' in the log)")
        row["stop_latency_s"] = round(t_injected - t_alt2, 3)
        log.info("[clip %d] stop latency: %.2fs (%s chars injected)",
                 index, t_injected - t_alt2, m.group(1))

        # Tell the target to write its content, then read it back.
        stopfile.write_text("", encoding="utf-8")
        deadline = time.monotonic() + 10.0
        while not out_txt.exists() and time.monotonic() < deadline:
            time.sleep(0.1)
        if not out_txt.exists():
            raise RuntimeError("target window did not write its content")
        text = out_txt.read_text(encoding="utf-8")

        # System volume must be back at its starting level by now (the app
        # restores it during stop, before injecting the text).
        vol1 = volume_state()
        row["volume_ok"] = "yes" if volume_matches(vol0, vol1) else "NO"
        if row["volume_ok"] == "NO":
            log.error("[clip %d] volume NOT restored: started %s, now %s",
                      index, vol0, vol1)

        ref_words = normalize_words(clip["ref"])
        hyp_words = normalize_words(text)
        sub, dele, ins = wer_counts(ref_words, hyp_words)
        row.update(ref_words=len(ref_words), sub=sub, ins=ins,
                   wer=round((sub + dele + ins) / len(ref_words), 4)
                   if ref_words else 0.0,
                   hyp=text)
        row["del"] = dele
        log.info("[clip %d] WER %.1f%% (%d ref words)%s", index,
                 row["wer"] * 100, len(ref_words), "" if text else " (EMPTY)")
    finally:
        # Never leave a dictation session live, whatever happened above.
        _ensure_stopped(tail, proc, hwnd)
        if target.poll() is None:
            stopfile.write_text("", encoding="utf-8")
            try:
                target.wait(timeout=5)
            except subprocess.TimeoutExpired:
                target.kill()
    return row


def _ensure_stopped(tail: LogTail, proc: subprocess.Popen, hwnd) -> None:
    """If a dictation is still active, press the hotkey once to end it.
    Only ever presses with our own target window in the foreground — if the
    app already stopped on its own (race), the press would START a new
    dictation into whatever is front, so a window we cannot foreground means
    no press at all (the process-tree kill ends the session without typing)."""
    if tail.count(ACTIVATED_RE) <= tail.count(ENDED_RE):
        return
    if hwnd is None:
        log.error("Dictation still active but the target window is gone — "
                  "NOT sending the hotkey (shutdown will kill the session).")
        return
    import keyboard
    # Everything slow happens BEFORE the foreground check, so nothing sits
    # between the check and the keypress; re-check "still active" last (the
    # app may have stopped on its own meanwhile — a press would then START one).
    mark = tail.mark()
    if tail.count(ACTIVATED_RE) <= tail.count(ENDED_RE):
        return
    log.warning("Dictation still active — sending the hotkey to stop it.")
    try:
        ensure_foreground(hwnd)
    except SafetyAbort as e:
        log.error("Dictation still active but %s — NOT sending the hotkey "
                  "(shutdown will kill the session).", e)
        return
    try:
        keyboard.send(TEST_HOTKEY)
        tail.wait_for(STOPPED_RE, 15.0, proc, since=mark)
    except Exception as e:
        log.error("Could not stop the leftover dictation: %s", e)


# -- Eqho child lifecycle -----------------------------------------------------------

def download_model(settings, model: str) -> None:
    """Fetch `model` the way the app's Models tab does (src/modelstore.py):
    same engine and model folder as `settings`. Blocks until done."""
    from src import modelstore
    from src.settings import is_model_cached
    expected = modelstore.MODEL_SIZES_MB.get(model)
    log.info("Downloading model %r (~%s MB) into %s ...", model,
             expected if expected else "?", settings.resolve_model_dir())
    if not modelstore.start_download(settings, model):
        raise SetupError(f"A download of {model!r} is already running.")
    t0 = time.monotonic()
    last = -1
    while True:
        st = modelstore.status(model)
        if st.get("done"):
            break
        pct = int(st.get("progress", 0.0) * 100)
        if pct // 10 > last // 10:
            log.info("  ... %d%%", pct)
            last = pct
        time.sleep(1.0)
    if st.get("error"):
        raise SetupError(f"Model download failed ({model}): {st['error']}")
    if not is_model_cached(settings, model):
        raise SetupError(f"Model {model!r} still isn't cached after the download.")
    log.info("Model %r downloaded: %.0f MB in %.0f s.", model,
             modelstore.model_size_mb(settings, model), time.monotonic() - t0)


def write_test_settings(cfg_dir: Path, model: str, language: str) -> None:
    """settings.json for the test run: the owner's real settings (so the
    model cache resolves), but toggle mode, TEST_HOTKEY, start-with-Windows off, history off."""
    from src.settings import Settings
    owner = Settings.load()
    data = {k: getattr(owner, k) for k in owner.__dataclass_fields__
            if k != "_listeners"}
    data.update(hotkey=TEST_HOTKEY, hotkey_mode="toggle", history_enabled=False,
                start_with_windows=False,
                model_size=model, language=language)
    cfg_dir.mkdir(parents=True, exist_ok=True)
    (cfg_dir / "settings.json").write_text(
        json.dumps(data, indent=2), encoding="utf-8")
    log.info("Test settings: model=%s language=%s toggle mode, history off "
             "(config dir: %s)", model, language, cfg_dir)


def start_eqho(cfg_dir: Path, swap: Path, work: Path) -> subprocess.Popen:
    env = dict(os.environ)
    env["EQHO_TEST_AUDIO"] = str(swap)
    env["EQHO_CONFIG_DIR"] = str(cfg_dir)
    proc = subprocess.Popen(
        [sys.executable, "run.py"], cwd=str(REPO_ROOT), env=env,
        stdout=(work / "eqho-stdout.log").open("w", encoding="utf-8"),
        stderr=subprocess.STDOUT)
    return proc


def wait_until_ready(tail: LogTail, proc: subprocess.Popen, timeout: float) -> None:
    log.info("Waiting for Eqho to load the model (up to %.0fs)...", timeout)
    t, _ = tail.wait_for(re.compile(re.escape(READY_MARKER)), timeout, proc)
    if t is not None:
        log.info("Eqho ready (model pre-loaded).")
        return
    if tail.count(re.compile(re.escape(LOAD_FAIL_MARKER))):
        raise SetupError("Eqho failed to pre-load the model (see the log)")
    if tail.count(re.compile(re.escape(ALREADY_RUNNING_MARKER))):
        raise SetupError("Eqho is already running — quit the running instance "
                         "(tray icon) before the e2e test")
    raise SetupError(f"Eqho did not become ready within {timeout:.0f}s")


def shutdown_eqho(proc: subprocess.Popen) -> None:
    """Stop Eqho and its model-host child. The app has no remote-quit IPC, so
    this kills the process tree; volume is verified/restored separately."""
    if proc.poll() is not None:
        return
    log.info("Shutting Eqho down (process tree)...")
    subprocess.run(["taskkill", "/PID", str(proc.pid), "/T", "/F"],
                   capture_output=True)
    try:
        proc.wait(timeout=10)
    except subprocess.TimeoutExpired:
        proc.kill()


# -- Main ---------------------------------------------------------------------------

def main() -> None:
    p = argparse.ArgumentParser(
        description="Eqho end-to-end dictation test: real app, real hotkey, "
                    "file audio, own target window. Uses the REAL desktop.")
    p.add_argument("--clips", type=int, default=3,
                   help="number of LibriSpeech clips (default 3)")
    p.add_argument("--audio", help="single-clip mode: path to a wav/flac file")
    p.add_argument("--ref", help="reference text for --audio")
    p.add_argument("--gate-wer", type=float,
                   help="exit 1 if total WER > X (e.g. 0.25 = 25%%)")
    p.add_argument("--work", default="_bench/e2e", help="output dir")
    p.add_argument("--data", help="LibriSpeech test-clean root (default: known path)")
    p.add_argument("--model", help="override the model from the owner's settings")
    p.add_argument("--language", help="override the language from the owner's settings")
    p.add_argument("--ready-timeout", type=float, default=300.0,
                   help="seconds to wait for the model to load (default 300)")
    p.add_argument("--allow-download", action="store_true",
                   help="download the model first if it isn't cached "
                        "(default: refuse)")
    args = p.parse_args()

    logging.basicConfig(level=logging.INFO,
                        format="%(levelname)s %(name)s: %(message)s")

    if sys.platform != "win32":
        log.error("The e2e test drives the Windows desktop (the hotkey, foreground "
                  "windows, pycaw) — run it with the owner's Windows venv.")
        sys.exit(2)

    try:
        clips = build_clips(args)
    except SetupError as e:
        log.error("%s", e)
        sys.exit(2)

    # The model must already be cached — the e2e test only downloads when
    # explicitly asked (--allow-download).
    from src.settings import Settings, is_model_cached
    owner = Settings.load()
    model = args.model or owner.model_size
    language = args.language or owner.language
    if not is_model_cached(owner, model):
        if not args.allow_download:
            log.error("Model %r is not cached — refusing to download it for an "
                      "e2e run (pass --allow-download to allow it).", model)
            sys.exit(2)
        try:
            download_model(owner, model)
        except SetupError as e:
            log.error("%s", e)
            sys.exit(2)

    work = (REPO_ROOT / args.work).resolve()
    work.mkdir(parents=True, exist_ok=True)
    cfg_dir = work / "eqho-config"
    if cfg_dir.exists():
        shutil.rmtree(cfg_dir)
    write_test_settings(cfg_dir, model, language)
    swap = work / ("test-audio" + Path(clips[0]["path"]).suffix)

    vol0 = volume_state()
    if vol0 is None:
        log.warning("Cannot read system volume — the volume-restore check "
                    "will be skipped.")
    else:
        log.info("System volume at start: %.0f%% (muted=%s)",
                 vol0[0] * 100, vol0[1])

    proc = start_eqho(cfg_dir, swap, work)
    tail = LogTail(cfg_dir / "eqho.log")
    rows: list[dict] = []
    csv_rows: list[dict] = []   # every clip, failed ones too (status column)
    failure: list[str] = []

    def failed_row(i: int, clip: dict, reason: str) -> None:
        csv_rows.append({"clip": i, "utt": clip["utt"], "ref": clip["ref"],
                         "status": reason[:80]})
    exit_code = 0
    try:
        wait_until_ready(tail, proc, args.ready_timeout)
        durations: dict = {}
        for i, clip in enumerate(clips, 1):
            try:
                row = run_clip(i, clip, work=work, swap=swap, tail=tail,
                               proc=proc, vol0=vol0, durations=durations)
                rows.append(row)
                csv_rows.append(dict(row, status="ok" if row["hyp"] else "empty text"))
            except SafetyAbort as e:
                failed_row(i, clip, f"safety abort: {e}")
                log.error("SAFETY ABORT: %s", e)
                failure.append(f"safety abort on clip {i}: {e}")
                exit_code = 1
                break
            except ChildDied as e:
                failed_row(i, clip, f"app died: {e}")
                log.error("[clip %d] FAILED: %s", i, e)
                failure.append(f"clip {i} ({clip['utt']}): {e}")
                exit_code = 1
                break
            except Exception as e:
                failed_row(i, clip, f"{type(e).__name__}: {e}")
                log.error("[clip %d] FAILED: %s", i, e)
                failure.append(f"clip {i} ({clip['utt']}): {type(e).__name__}: {e}")
    except (SetupError, ChildDied) as e:
        log.error("%s", e)
        failure.append(str(e))
        exit_code = 2
    finally:
        shutdown_eqho(proc)
        vol1 = volume_state()
        if vol0 is not None and not volume_matches(vol0, vol1):
            log.error("System volume left at %s (started at %s) — restoring.",
                      vol1, vol0)
            restore_volume(vol0)
            failure.append(f"volume not restored (was {vol0}, ended {vol1})")

    # Clips never reached (setup failure or abort) still get a row
    for i, clip in enumerate(clips, 1):
        if i > len(csv_rows):
            failed_row(i, clip, "not run")

    # Results CSV
    out_csv = work / "e2e-results.csv"
    with out_csv.open("w", newline="", encoding="utf-8") as f:
        w = csv.DictWriter(f, fieldnames=CSV_COLUMNS)
        w.writeheader()
        w.writerows(csv_rows)

    # Summary (pooled WER over all clips, like the ASR benchmark)
    ref_total = sum(r["ref_words"] for r in rows)
    err_total = sum(int(r["sub"]) + int(r["del"]) + int(r["ins"]) for r in rows)
    wer_total = (err_total / ref_total) if ref_total else 0.0
    empty = sum(1 for r in rows if not r["hyp"])
    lat_start = [r["start_latency_s"] for r in rows if r["start_latency_s"] != ""]
    lat_stop = [r["stop_latency_s"] for r in rows if r["stop_latency_s"] != ""]
    print()
    print("-" * 64)
    print(" Eqho end-to-end dictation test")
    print(" clips        : %d  (ok %d, empty %d, failed %d)"
          % (len(clips), len(rows) - empty, empty, len(failure)))
    print(" WER (total)  : %.2f%%  (%d errors / %d ref words)"
          % (wer_total * 100, err_total, ref_total))
    if lat_start:
        print(" latency      : start %.2fs avg, stop %.2fs avg (hotkey -> "
              "listening / stop -> text appears)"
              % (sum(lat_start) / len(lat_start),
                 sum(lat_stop) / len(lat_stop) if lat_stop else 0.0))
    if vol0 is None:
        print(" volume       : check skipped (volume probe unavailable)")
    else:
        print(" volume       : %s"
              % ("restored OK" if not any("volume" in f for f in failure)
                 else "NOT restored"))
    print(" config dir   : %s" % cfg_dir)
    print(" results      : %s" % out_csv)
    print(" eqho log     : %s" % (cfg_dir / "eqho.log"))
    print("-" * 64)

    # Gates
    if empty:
        failure.append("%d clip(s) produced no text in the target window" % empty)
    if args.gate_wer is not None and ref_total:
        if wer_total > args.gate_wer:
            failure.append("total WER %.2f%% > gate %.2f%%"
                           % (wer_total * 100, args.gate_wer * 100))
    if failure:
        log.error("FAILED: %s", "; ".join(failure))
        sys.exit(exit_code or 1)
    log.info("All checks passed.")
    sys.exit(0)


if __name__ == "__main__":
    main()
