# Testing — Eqho

Beyond the `--smoke` gate (`run.py --smoke`, must pass before every commit),
Eqho has targeted benchmarks. This file explains how to run them and what the
numbers mean.

The `tools/probe_*.py` scripts run in a temporary config folder (`tools/_probe_env.py` sets `EQHO_CONFIG_DIR`), so they never touch the real eqho.log, settings or history; `tools/check_probe_isolation.py` verifies this.

## ASR benchmark — `tools/bench_asr.py` (EQ-TESTASR)

Measures transcription **accuracy and speed on real speech with known text**:
LibriSpeech test-clean (CC BY 4.0; 2620 utterances, 40 speakers). The dataset
is kept **outside the repo** at `D:\Vibe Coding\_vm\eqho\asr-data\LibriSpeech\test-clean`.

Every utterance is decoded to float32 mono 16 kHz (PyAV) and transcribed
through Eqho's **real path**: the `ModelHost` child process, the model from
the user's settings, the app's dictation beam size (5) and its
post-processing gates (low-confidence segment drop, hallucination blocklist).
No model is ever loaded in-process, and an uncached model is **refused** unless
`--allow-download` is passed.

### Run

```bash
# owner's Windows venv — never pip-install into it
"/mnt/d/Vibe Coding/Eqho/venv/Scripts/python.exe" tools/bench_asr.py --limit 12 --work _bench/asr-check
```

### Flags

| Flag | Meaning |
|------|---------|
| `--limit N` | N utterances, deterministic: round-robin over speakers, sorted ids (default 60, ~8-10 min audio) |
| `--model NAME` | override the model from the user's settings |
| `--allow-download` | permit downloading an uncached model (default: refuse and exit 2) |
| `--work DIR` | output dir; writes `DIR/asr-results.csv` (default `_bench/asr`) |
| `--data DIR` | LibriSpeech test-clean root (default: the known path above) |
| `--language CODE` | override the language from the user's settings |
| `--beam-size N` | beam size (default 5 = the app's dictation path) |
| `--baseline CSV` | print a WER/RTF comparison to an earlier run's CSV |
| `--gate-wer X` | exit 1 if total WER > X (e.g. `0.15` = 15%) |
| `--gate-vs CSV` | exit 1 if total WER worsens by more than 1 percentage point vs that run |

### What the numbers mean

- **WER (word error rate)** — `(substitutions + deletions + insertions) / reference
  words`, computed on normalized words (lowercase, punctuation stripped,
  spaces collapsed) with an in-repo Levenshtein. The **total** WER pools all
  errors over all reference words (an empty hypothesis counts as all
  deletions); per-utterance WER is in the CSV.
- **RTF (real-time factor)** — transcribe time / audio length. Lower is
  faster; RTF 0.2 means 5x realtime. Includes only the `ModelHost.transcribe`
  call, not decode or load.
- **Model load** — wall time of `ModelHost.load_model` (child spawn + model
  load + warmup), i.e. what the app experiences on a cold start.
- **Empty / failed** — utterances that produced no text (counted as WER 1.0)
  vs utterances whose transcribe call errored (excluded from WER; any failure
  fails the run with exit 1).

### Exit codes

`0` pass · `1` gate failed or failed utterances · `2` setup error (missing
data, uncached model, failed load, bad baseline CSV).

### Comparing runs

Keep a results CSV per meaningful run (e.g. `_bench/asr-<date>/asr-results.csv`)
and use it as the reference:

```bash
"...python.exe" tools/bench_asr.py --limit 60 --work _bench/asr-new \
    --baseline _bench/asr-2026-10/asr-results.csv \
    --gate-wer 0.15 --gate-vs _bench/asr-2026-10/asr-results.csv
```

This prints the WER/RTF deltas and fails the run if WER exceeds 15% or
regresses more than 1 pp versus the reference — the guard for engine, model
or post-processing changes.

## End-to-end dictation test — `tools/test_e2e.py` (EQ-TESTE2E)

Drives the **whole app like a user**: starts Eqho (`run.py`) with a recording
instead of the microphone, presses the hotkey (Ctrl+Shift+F9 in its test settings — a simulated Ctrl+Shift+F9 never reaches the `keyboard` library; a real Ctrl+Shift+F9 works), and checks that
the transcript lands in a target window. Covers everything the ASR benchmark
cannot: hotkey, VAD, overlay, volume duck/restore, focus handling, text
injection, teardown.

**WARNING: this test uses the real desktop.** It sends real Ctrl+Shift+F9 keypresses
and brings its own target window to the foreground. Do not touch the machine
while it runs. Before every keypress it verifies that its own target window
is the foreground window and aborts without pressing anything if it is not —
but focus can change at any moment, so the machine must be left alone for
the duration of the run. Quit any running Eqho first (the single-instance
lock would make the test instance exit immediately).

### How it works

- `EQHO_TEST_AUDIO=<wav/flac>` — the recorder feeds that file (decoded to
  float32 mono 16 kHz, real-time pacing, then silence) instead of opening the
  microphone. Zero effect when unset. The file is re-read on every
  dictation start, so the test swaps its content between clips.
- `EQHO_CONFIG_DIR=<dir>` — settings, history and the log live in that
  folder instead of `%APPDATA%\Eqho`, so the run never touches the owner's
  real settings or history. Zero effect when unset.
- The test writes its own `settings.json` (the owner's model and language,
  but toggle mode, Ctrl+Shift+F9, history off), starts `tools/e2e_target.py` (a Tk
  text box that writes its content to a file when told to close), waits for
  Eqho's log to say the model is pre-loaded, then per clip: foreground the
  target, Ctrl+Shift+F9 (start), wait out the clip, Ctrl+Shift+F9 (stop), read the target's
  text, compute WER (same normalizer/Levenshtein as the ASR benchmark), and
  measure latency from Eqho's log (Ctrl+Shift+F9 → "Transcription started",
  stop → "Injected N chars"). System volume is checked after every clip
  (pycaw) and restored by the test if Eqho left it ducked.
- Shutdown kills the Eqho process tree (`taskkill /T /F`) — the app has no
  remote-quit IPC; volume is verified/restored separately, which is what
  matters.

### Run

```bash
# owner's Windows venv — never pip-install into it; Windows only
"/mnt/d/Vibe Coding/Eqho/venv/Scripts/python.exe" tools/test_e2e.py --clips 3 --gate-wer 0.25
```

| Flag | Meaning |
|------|---------|
| `--clips N` | N LibriSpeech test-clean clips, round-robin over speakers (default 3) |
| `--audio F --ref "text"` | single clip from any wav/flac with known text |
| `--gate-wer X` | exit 1 if total WER > X (e.g. `0.25` = 25%) |
| `--work DIR` | output dir; writes `DIR/e2e-results.csv` (default `_bench/e2e`) |
| `--data DIR` | LibriSpeech test-clean root (default: the known path from part 1) |
| `--model NAME` / `--language CODE` | override the owner's settings (model must be cached unless `--allow-download`) |
| `--allow-download` | download an uncached model first (same engine, model folder and code path as the app's Models tab) instead of refusing |
| `--ready-timeout S` | seconds to wait for the model to load (default 300) |

### What the numbers mean

- **WER (total)** — pooled over all clips, same normalization as the ASR
  benchmark. A clip with no text in the target window counts as all
  deletions AND fails the run on its own.
- **Latency** — start: Alt+Q → "Transcription started" in the log (the mic
  path is live); stop: Alt+Q → "Injected N chars" (text is in the target).
  Wall-clock from the keypress to the log line, so it includes the app's
  deliberate stop-path sleeps.
- **Volume** — the system master volume must be back at its starting level
  (and unmuted, unless it started muted) after every clip.

### Exit codes

`0` pass · `1` clip/gate failure or safety abort · `2` setup error (missing
data, uncached model, Eqho already running, model failed to load).

Everything the run produced (per-clip text, results CSV, Eqho's own log and
config dir) is kept under `--work` for inspection.

## UI interactive test — `tools/test_ui.py` (EQ-TESTUI)

Automated end-to-end verification of every screen, tab, and control in the
CustomTkinter dashboard (`src/ui/`):
1. **In-process construction**: Builds the real `Dashboard` window the way the app
   does (with minimal fake app context; no model inference, no child processes,
   no global hotkey hooks, no system tray).
2. **Full coverage**: Visits every tab (`general`, `models`, `history`, `settings`,
   including embedded Overlay and About panels).
3. **Control traversal**: Finds every button, switch/checkbox, dropdown, segmented
   button, slider, entry, and textbox in the widget tree, exercises each (invokes,
   toggles, cycles choices, sets values), and pumps the Tk event loop.
4. **Exception detection**: Catches and fails on any unhandled Tkinter callback
   exceptions via `report_callback_exception`.
5. **Safety SKIP list**: Never triggers quit/exit, history deletion/clearing,
   file/folder/URL dialogs, Windows autostart, model downloads, or interactive
   hotkey capture.
6. **Screenshots**: Captures a PIL ImageGrab screenshot of the dashboard window for
   each visited tab to `<work>/ui-<tab>.png`.
7. **Conditional states** (EQ-STATES): with fakes only (no real download, mic,
   keyboard hook), each rarely seen state gets the cut-off check and a
   `<work>/ui-state-<name>.png`: Models card downloading (40 %) and failed;
   General hotkey capture mode; History empty / search with no results / one
   very long entry; Setup model page with the model present and after a failed
   download, mic page "Windows is blocking" and "No microphone found"; the
   dictation overlay (own Tk thread — only show/update/hide/after cross threads,
   geometry read on the overlay thread) with "Writing it down…", "Still
   writing…" and a long partial.
8. **Light theme**: default tabs, setup pages and all states again in the light
   theme (`*-light.png`, cut-off check included), then the theme is restored.

### Run

```bash
# Owner's Windows venv (opens interactive desktop window for ~1 minute)
"/mnt/d/Vibe Coding/Eqho/venv/Scripts/python.exe" tools/test_ui.py --work _test/ui
```

### Flags

| Flag | Meaning |
|------|---------|
| `--work DIR` | Output directory for tab screenshots (default `_test/ui`) |
| `--delay SEC` | Delay in seconds between control operations (default `0.03`) |

### Exit codes

`0` pass · `1` any exception or Tk callback failure.
