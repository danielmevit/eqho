# Smaller models on the processor (main PC, 2026-10-16)

Ryzen 7 5800H (8 cores / 16 threads), CUDA hidden (`CUDA_VISIBLE_DEVICES=-1`), faster-whisper forced (int8,
`cpu_threads` = 8 from `_cpu_threads()`), beam 5, language en. Same 60 + 60 LibriSpeech clips (test-clean /
test-other) as every other 2026-10-16 run. Raw per-clip CSVs + console log in `2026-10-16-cpu-models/`.

| Model | WER clean | WER other | × realtime clean / other | median s per clip | short clips (< 4 s) | 8–12 s clips | load |
|---|---|---|---|---|---|---|---|
| base | 4.95 % | 8.48 % | 8.4 / 6.7 | 0.94 | 0.77 | 1.08 | 2.6 s |
| distil-small.en | 3.82 % | 6.18 % | 3.3 / 2.8 | 2.56 | 2.42 | 2.72 | 5.5 s |
| small | 3.68 % | 5.99 % | 2.8 / 2.4 | 3.02 | 2.48 | 3.23 | 5.6 s* |
| distil-medium.en | 4.24 % | 7.56 % | 1.6 / 1.3 | 5.29 | 5.09 | 5.43 | 10.5 s |
| distil-large-v3 (earlier run, default 4 threads) | 2.48 % | 4.61 % | 0.9 / 0.7 | 9.55 | 9.26 | 10.20 | — |

\* first load 34 s included the download.

Notes:
- Each call has a fixed cost (the 30 s encoder window), so "seconds per clip" is what a user waits after stop —
  "× realtime" over 9 s clips hides it.
- distil-medium.en is slower than small AND less accurate → not worth recommending.
- distil-small.en vs small: same accuracy within noise, distil-small.en ~15 % faster; small is multilingual.
- The Intel i7-8665U laptop (4 cores) needed ~1.5–2× the time of this PC for distil-large-v3 (14–19 s vs ~10 s),
  so expect ~4–5 s per sentence with distil-small.en there — to be confirmed by laptop run 2 (TEST-PLAN-2).
- Two Claude workers ran short UI tests / smoke checks on this PC between 16:14 and 16:25 (small, distil-medium.en
  runs); their timings may be a few % high.

**Proposal for 0.8.10 (automatic model by hardware):** NVIDIA → distil-large-v3 (English) / large-v3-turbo (other);
graphics card with its own memory (whisper.cpp) → same; processor only → distil-small.en (English) / small (other).
