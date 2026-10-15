#!/usr/bin/env python3
"""EQ-TESTASR — transcription accuracy + speed benchmark.

Benchmarks Eqho's REAL dictation path — the ModelHost child process with the
model and settings the app itself would use — against LibriSpeech test-clean
utterances whose reference text is known. The dataset lives OUTSIDE the repo.

Run with the owner's Windows venv (never pip-install into it):

    "venv/Scripts/python.exe" tools/bench_asr.py --limit 12 --work _bench/asr-check

Writes <work>/asr-results.csv (one row per utterance) and prints a summary.
Exit codes: 0 = pass, 1 = gate failed or failed utterances, 2 = setup error.
"""

import argparse
import csv
import logging
import os
import re
import sys
import time
from pathlib import Path

# The ModelHost child (spawn) re-imports this module as __mp_main__, so the
# repo root must land on sys.path at module level — before anything imports
# the src package. All heavy imports (av, numpy, src.*) stay inside main()
# so the child never pays for them.
REPO_ROOT = Path(__file__).resolve().parents[1]
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

SAMPLE_RATE = 16000

# LibriSpeech test-clean (CC BY 4.0), kept outside the repo. The Windows path
# applies when running under the owner's venv; the /mnt/d path under WSL.
DEFAULT_DATA_WIN = Path("D:/Vibe Coding/_vm/eqho/asr-data/LibriSpeech/test-clean")
DEFAULT_DATA_WSL = Path("/mnt/d/Vibe Coding/_vm/eqho/asr-data/LibriSpeech/test-clean")

CSV_COLUMNS = ["utt_id", "speaker", "audio_s", "ref_words", "sub", "del", "ins",
               "wer", "transcribe_s", "rtf", "status", "ref", "hyp"]

log = logging.getLogger("bench_asr")


# -- Text normalization + WER (own Levenshtein, no new packages) ---------------

def normalize_words(text: str) -> list:
    """Lowercase, drop apostrophes, strip punctuation, collapse spaces."""
    t = text.lower().replace("\u2019", "'").replace("'", "")
    t = re.sub(r"[^a-z0-9]+", " ", t)
    return t.split()


def wer_counts(ref: list, hyp: list) -> tuple:
    """Levenshtein on word lists -> (substitutions, deletions, insertions)."""
    m, n = len(ref), len(hyp)
    d = [[0] * (n + 1) for _ in range(m + 1)]
    for i in range(m + 1):
        d[i][0] = i
    for j in range(n + 1):
        d[0][j] = j
    for i in range(1, m + 1):
        row = d[i]
        prev = d[i - 1]
        for j in range(1, n + 1):
            cost = 0 if ref[i - 1] == hyp[j - 1] else 1
            row[j] = min(prev[j - 1] + cost, prev[j] + 1, row[j - 1] + 1)
    sub = dele = ins = 0
    i, j = m, n
    while i > 0 or j > 0:
        if i > 0 and j > 0 and ref[i - 1] == hyp[j - 1] and d[i][j] == d[i - 1][j - 1]:
            i, j = i - 1, j - 1
        elif i > 0 and j > 0 and d[i][j] == d[i - 1][j - 1] + 1:
            sub += 1
            i, j = i - 1, j - 1
        elif i > 0 and d[i][j] == d[i - 1][j] + 1:
            dele += 1
            i -= 1
        else:
            ins += 1
            j -= 1
    return sub, dele, ins


# -- Dataset --------------------------------------------------------------------

def collect_utterances(data_dir: Path) -> list:
    """All test-clean utterances: <speaker>/<chapter>/<utt>.flac + .trans.txt."""
    utts = []
    for spk_dir in sorted(p for p in data_dir.iterdir() if p.is_dir()):
        for ch_dir in sorted(p for p in spk_dir.iterdir() if p.is_dir()):
            trans = ch_dir / f"{spk_dir.name}-{ch_dir.name}.trans.txt"
            if not trans.exists():
                continue
            for line in trans.read_text(encoding="utf-8").splitlines():
                parts = line.strip().split(None, 1)
                if len(parts) != 2:
                    continue
                utt_id, text = parts
                flac = ch_dir / f"{utt_id}.flac"
                if flac.exists():
                    utts.append({"utt_id": utt_id, "speaker": spk_dir.name,
                                 "flac": flac, "text": text})
    return utts


def pick_subset(utts: list, limit: int) -> list:
    """Deterministic subset: round-robin over speakers, sorted utterance ids."""
    by_speaker = {}
    for u in sorted(utts, key=lambda x: x["utt_id"]):
        by_speaker.setdefault(u["speaker"], []).append(u)
    speakers = sorted(by_speaker)
    picked = []
    idx = 0
    while len(picked) < limit:
        progressed = False
        for spk in speakers:
            if len(picked) >= limit:
                break
            queue = by_speaker[spk]
            if idx < len(queue):
                picked.append(queue[idx])
                progressed = True
        if not progressed:
            break
        idx += 1
    return picked


# -- Audio decode (PyAV, no new dependencies) -----------------------------------

def decode_flac(path: Path) -> "object":
    """Decode a FLAC file to float32 mono 16 kHz (LibriSpeech is already
    16 kHz mono, but resample anyway so odd files can't skew results)."""
    import av
    import numpy as np
    container = av.open(str(path))
    try:
        stream = container.streams.audio[0]
        resampler = av.AudioResampler(
            format=av.AudioFormat("s16"), layout="mono", rate=SAMPLE_RATE)
        parts = []
        for frame in container.decode(stream):
            outs = resampler.resample(frame)
            if not isinstance(outs, list):  # older PyAV returns a single frame
                outs = [outs]
            for out in outs:
                arr = out.to_ndarray()
                if arr.ndim == 2:  # packed mono -> (1, n)
                    arr = arr.reshape(-1)
                parts.append(arr.astype(np.float32) / 32768.0)
        if not parts:
            return np.zeros(0, dtype=np.float32)
        return np.concatenate(parts)
    finally:
        container.close()


# -- App-path post-processing (mirror transcriber._do_complete) -----------------

def apply_app_gates(segments: list, duration_s: float) -> str:
    """Join segments the way the app's dictation path does: drop empty and
    low-confidence segments, discard short known hallucinations. The
    near-silent RMS gate is mic-specific and does not apply to file audio."""
    from src.transcriber import (AVG_LOGPROB_MIN, HALLUCINATION_BLOCKLIST,
                                 NO_SPEECH_PROB_MAX)
    kept = []
    for s in segments:
        t = s["text"].strip()
        if not t:
            continue
        if s["no_speech_prob"] > NO_SPEECH_PROB_MAX and s["avg_logprob"] < AVG_LOGPROB_MIN:
            continue
        kept.append(t)
    text = " ".join(kept)
    if text and duration_s < 2.0 and text.lower().strip(' .!?"') in HALLUCINATION_BLOCKLIST:
        return ""
    return text


# -- Totals / baseline ----------------------------------------------------------

def totals_from_rows(rows: list) -> dict:
    """Aggregate per-utterance rows (from this run or a previous CSV)."""
    ref = err = 0
    audio = transcribe = 0.0
    ok = empty = failed = 0
    for r in rows:
        status = r["status"]
        if status == "error":
            failed += 1
            continue
        empty += 1 if status == "empty" else 0
        ok += 0 if status == "empty" else 1
        ref += int(r["ref_words"])
        err += int(r["sub"]) + int(r["del"]) + int(r["ins"])
        audio += float(r["audio_s"])
        transcribe += float(r["transcribe_s"])
    return {
        "n": len(rows), "ok": ok, "empty": empty, "failed": failed,
        "ref_words": ref, "errors": err,
        "wer": (err / ref) if ref else 0.0,
        "audio_s": audio, "transcribe_s": transcribe,
        "rtf": (transcribe / audio) if audio else 0.0,
    }


def read_baseline(csv_path: str) -> dict:
    path = Path(csv_path)
    if not path.exists():
        log.error("Baseline CSV not found: %s", path)
        sys.exit(2)
    with path.open(newline="", encoding="utf-8") as f:
        rows = list(csv.DictReader(f))
    missing = [c for c in CSV_COLUMNS[:11] if rows and c not in rows[0]]
    if not rows or missing:
        log.error("Baseline CSV %s is not a bench_asr results file (missing %s).",
                  path, missing or "rows")
        sys.exit(2)
    return totals_from_rows(rows)


# -- Main -----------------------------------------------------------------------

def main() -> None:
    p = argparse.ArgumentParser(
        description="Eqho ASR benchmark: WER + speed on LibriSpeech test-clean "
                    "through the real ModelHost path.")
    p.add_argument("--limit", type=int, default=60,
                   help="utterances, spread round-robin over speakers (default 60)")
    p.add_argument("--model", help="override the model from the user's settings")
    p.add_argument("--allow-download", action="store_true",
                   help="permit downloading the model if not cached (default: refuse)")
    p.add_argument("--work", default="_bench/asr", help="output dir (default _bench/asr)")
    p.add_argument("--data", help="LibriSpeech test-clean root (default: known path)")
    p.add_argument("--language", help="override the language from the user's settings")
    p.add_argument("--beam-size", type=int, default=5,
                   help="beam size (default 5 = the app's dictation path)")
    p.add_argument("--baseline", help="print a comparison to an earlier results CSV")
    p.add_argument("--gate-wer", type=float, help="exit 1 if total WER > X (e.g. 0.15)")
    p.add_argument("--gate-vs", help="exit 1 if WER worsens by >1 pp vs this results CSV")
    args = p.parse_args()

    logging.basicConfig(level=logging.INFO,
                        format="%(levelname)s %(name)s: %(message)s")

    # Dataset
    if args.data:
        data_dir = Path(args.data)
    else:
        data_dir = DEFAULT_DATA_WIN if DEFAULT_DATA_WIN.exists() else DEFAULT_DATA_WSL
    if not data_dir.exists():
        log.error("LibriSpeech test-clean not found at %s (use --data).", data_dir)
        sys.exit(2)
    utts = collect_utterances(data_dir)
    if not utts:
        log.error("No utterances found under %s.", data_dir)
        sys.exit(2)
    subset = pick_subset(utts, args.limit)
    log.info("Dataset: %d utterances, picked %d (round-robin over speakers).",
             len(utts), len(subset))

    # Settings + model — exactly what the app would use
    from src.settings import Settings, is_model_cached
    settings = Settings.load()
    model = args.model or settings.model_size
    language = args.language or settings.language
    model_dir = settings.resolve_model_dir()
    os.environ["HF_HUB_CACHE"] = str(Path(model_dir) / "huggingface")
    if not is_model_cached(settings, model) and not args.allow_download:
        log.error("Model %r is not cached in %s — refusing to download "
                  "(pass --allow-download to allow it).", model, model_dir)
        sys.exit(2)

    # Real inference path: ModelHost child process, app-resolved backend
    from src.model_host import ModelHost, ModelHostError, resolve_backend
    backend = resolve_backend(getattr(settings, "engine_backend", "auto"))
    host = ModelHost(backend=backend)
    log.info("Loading model %s (backend=%s, dir=%s)...", model, backend, model_dir)
    t0 = time.perf_counter()
    if not host.load_model(model, model_dir):
        log.error("Model host failed to load %s.", model)
        sys.exit(2)
    load_s = time.perf_counter() - t0

    # Transcribe every utterance
    rows = []
    host_down = False
    for i, u in enumerate(subset, 1):
        utt_id, ref_text = u["utt_id"], u["text"]
        if host_down:
            rows.append({"utt_id": utt_id, "speaker": u["speaker"],
                         "audio_s": 0.0, "ref_words": 0, "sub": 0, "del": 0,
                         "ins": 0, "wer": "", "transcribe_s": 0.0, "rtf": "",
                         "status": "error: model host down", "ref": ref_text,
                         "hyp": ""})
            log.error("[%d/%d] %s skipped (model host down).", i, len(subset), utt_id)
            continue
        audio = decode_flac(u["flac"])
        duration = len(audio) / SAMPLE_RATE
        t0 = time.perf_counter()
        try:
            segments = host.transcribe(audio, language, args.beam_size,
                                       settings.initial_prompt or "")
            transcribe_s = time.perf_counter() - t0
            text = apply_app_gates(segments, duration)
            status = "ok" if text else "empty"
        except ModelHostError as e:
            transcribe_s = time.perf_counter() - t0
            log.error("[%d/%d] %s failed: %s", i, len(subset), utt_id, e)
            rows.append({"utt_id": utt_id, "speaker": u["speaker"],
                         "audio_s": duration, "ref_words": 0, "sub": 0, "del": 0,
                         "ins": 0, "wer": "", "transcribe_s": transcribe_s,
                         "rtf": "", "status": "error", "ref": ref_text, "hyp": ""})
            # The child is dead after a failure — respawn like the app would
            # on its next session; if that fails too, skip the rest.
            host_down = not host.load_model(model, model_dir)
            continue
        ref_words = normalize_words(ref_text)
        hyp_words = normalize_words(text)
        sub, dele, ins = wer_counts(ref_words, hyp_words)
        wer = (sub + dele + ins) / len(ref_words) if ref_words else 0.0
        rtf = transcribe_s / duration if duration else 0.0
        rows.append({"utt_id": utt_id, "speaker": u["speaker"],
                     "audio_s": round(duration, 3), "ref_words": len(ref_words),
                     "sub": sub, "del": dele, "ins": ins,
                     "wer": round(wer, 4), "transcribe_s": round(transcribe_s, 3),
                     "rtf": round(rtf, 4), "status": status,
                     "ref": ref_text, "hyp": text})
        log.info("[%d/%d] %s %.1fs audio, WER %.1f%%, RTF %.3f%s",
                 i, len(subset), utt_id, duration, wer * 100, rtf,
                 "" if text else " (EMPTY)")
    host.shutdown()

    # Results CSV
    work = Path(args.work)
    work.mkdir(parents=True, exist_ok=True)
    out_csv = work / "asr-results.csv"
    with out_csv.open("w", newline="", encoding="utf-8") as f:
        w = csv.DictWriter(f, fieldnames=CSV_COLUMNS)
        w.writeheader()
        w.writerows(rows)

    # Summary
    t = totals_from_rows(rows)
    info = host.info
    print()
    print("-" * 64)
    print(" Eqho ASR benchmark")
    print(" utterances   : %d  (ok %d, empty %d, failed %d)"
          % (t["n"], t["ok"], t["empty"], t["failed"]))
    print(" audio        : %.1f s   transcribe: %.1f s" % (t["audio_s"], t["transcribe_s"]))
    print(" RTF          : %.3f  (%.1fx realtime)" % (t["rtf"], 1 / t["rtf"] if t["rtf"] else 0))
    print(" WER (total)  : %.2f%%  (%d errors / %d ref words)"
          % (t["wer"] * 100, t["errors"], t["ref_words"]))
    print(" model load   : %.1f s" % load_s)
    print(" model        : %s  backend=%s  %s" % (
        model, info.get("backend", backend),
        " ".join(f"{k}={v}" for k, v in info.items() if k != "backend")))
    print(" results      : %s" % out_csv)
    print("-" * 64)

    # Baseline comparison / gates
    gate_failures = []
    if t["failed"]:
        gate_failures.append("%d failed utterances" % t["failed"])
    if args.baseline:
        b = read_baseline(args.baseline)
        print(" baseline     : WER %.2f%% -> %+.2f pp, RTF %.3f -> %+.3f (%s)"
              % (b["wer"] * 100, (t["wer"] - b["wer"]) * 100,
                 b["rtf"], t["rtf"] - b["rtf"], args.baseline))
    if args.gate_vs:
        b = read_baseline(args.gate_vs)
        delta_pp = (t["wer"] - b["wer"]) * 100
        print(" gate-vs      : WER %+.2f pp vs %s (limit +1.00 pp)"
              % (delta_pp, args.gate_vs))
        if delta_pp > 1.0:
            gate_failures.append("WER worsened by %.2f pp vs %s" % (delta_pp, args.gate_vs))
    if args.gate_wer is not None:
        if t["wer"] > args.gate_wer:
            gate_failures.append("total WER %.2f%% > gate %.2f%%"
                                 % (t["wer"] * 100, args.gate_wer * 100))
    if gate_failures:
        log.error("GATE FAILED: %s", "; ".join(gate_failures))
        sys.exit(1)
    log.info("All gates passed.")


if __name__ == "__main__":
    main()
