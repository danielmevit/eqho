# Testing — Eqho

Beyond the `--smoke` gate (`run.py --smoke`, must pass before every commit),
Eqho has targeted benchmarks. This file explains how to run them and what the
numbers mean.

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
