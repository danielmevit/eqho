# Eqho on the Intel laptop — run 2 results (2026-10-16)

Dell Latitude 5400 · Intel Core i7-8665U (4 cores / 8 threads) · 16 GB · Intel UHD Graphics 620 (driver 31.0.101.2135)
· Windows 11 Pro 10.0.26200 · display 1920×1080 at 150 % Windows scaling.
Tested: public `danielmevit/eqho` **dev @ `9749bbc656bc01b36e30cb7c125dff183e64d64f`** (reports version 0.8.9).
Run by a Claude Code session on the laptop, following `TEST-PLAN-2.md`. Times are CEST. Paths are written as `KIT\...`.

## TL;DR

1. **The fixes work on this laptop.** Auto picks faster-whisper on the CPU (built-in graphics are skipped), Eqho
   recommends Distil Small EN (English) / Small (German), and **real dictation now types the text 3/3 with both the
   old and the new default model**. Nothing is dropped any more. The expected eqho.log lines are all there, including
   exactly one slow-model tip in the distil-large-v3 run.
2. **Distil Small EN is a good default here.** Stop → text **2.9 s** on average in real dictation (distil-large-v3:
   13.6 s). In the benchmark it needs **~2.6 s per clip** (distil-large-v3: 14–19 s), and a 2-second clip still costs
   ~2.5 s. WER 3.89 % / 5.99 % (clean / other), +1.4 pp vs distil-large-v3, and the same as the main PC.
3. **On battery, speed halves in phases.** All first measurements in this run happened on battery (the charger was
   not connected): CPU-bound work switched between normal and ~2× slower phases (distil-large-v3 dictation took
   26 s instead of 13.6 s). Everything was re-run on AC; **the tables below are the AC numbers**, and the battery
   numbers are kept in `runs2\battery\`. Transcripts were identical on battery and AC, so only speed is affected.
4. **The UI test failed (23 cut-off texts) and the screenshots show two real problems at 150 % scaling:**
   - The window (940×560 × 125 % zoom × 150 % Windows scaling = **1762×1050 px**) is **taller than the space above
     the taskbar (~1008 px)**. The taskbar covers the bottom of every screenshot, and **no first-time-setup page shows
     its Skip / Back / Next buttons**. They are packed at the bottom (`src/ui/onboarding.py:77`) and end up behind
     the taskbar.
   - At the minimum window width, setup text is clipped instead of wrapped (fixed `wraplength=620`,
     `src/ui/onboarding.py:163`). The welcome page then reads "Everything stays on this computer — you say is sent
     anywhere": the word "nothing" is lost.

---

## Step 1: new source (dev branch)

| Check | Result |
|---|---|
| Commit (zip comment) | `9749bbc656bc01b36e30cb7c125dff183e64d64f` = `dev` HEAD on GitHub at test time ("Intel laptop test plan: never record computer name…") |
| `requirements-lock.txt` | identical to run 1 → `pip install -r`: 51 × "Requirement already satisfied", **nothing new installed** |
| Engine wheel | `pywhispercpp-0.0.0-cp313-cp313-win_amd64.whl`, SHA-256 `85C3123D…08B5`, identical to run 1. Force-reinstalled as the plan says. `pip check`: no broken requirements. |
| `run.py --smoke` | **`"ok": true`**, 15.2 s, version 0.8.9, 12 audio devices, model_load ok, silence → `""`. It downloaded the 75 MB faster-whisper tiny model into `KIT\models` (run 1's smoke used whisper.cpp). |

Notes on step 1:
- The plan's zip-comment one-liner prints an **empty line** in Windows PowerShell 5.1: .NET Framework's `ZipArchive`
  has no `Comment` property. I read the comment with Python's `zipfile` instead, and cross-checked it against the
  GitHub API.
- As in run 1, I ran the smoke check with `EQHO_CONFIG_DIR=KIT\cfg-smoke` (model_dir = `KIT\models`) so that
  nothing is written outside KIT. `%APPDATA%\Eqho` and `%LOCALAPPDATA%\Eqho` still do not exist.
- The version is still 0.8.9 on dev (the plan mentions 0.8.10; presumably it is bumped at release).

## Step 2: what Eqho picks on this laptop

```
INFO:src.model_host:Engine auto: graphics adapters [Intel(R) UHD Graphics 620 128 MB] -> faster-whisper
engine: faster-whisper | hardware: cpu | model en: distil-small.en | model de: small
```
**Exactly as expected.** DXGI reports 128 MB of dedicated memory for the UHD 620, below the 2 GiB threshold.

## Step 3: real dictation (`tools/test_e2e.py`, 3 clips, engine_backend = auto)

Final runs on **AC power** (22:27–22:30):

| Model | Clip | Audio | Typed? | Stop → text | WER |
|---|---|---|---|---|---|
| distil-large-v3 | 1089-134686-0000 | 10.4 s | yes (162 chars) | 13.86 s | 0.0 % |
| distil-large-v3 | 1188-133604-0000 | 10.7 s | yes (109 chars) | 13.55 s | 0.0 % |
| distil-large-v3 | 121-121726-0000 | 8.5 s | yes (104 chars) | 13.30 s | 0.0 % |
| **distil-large-v3 total** | | | **3/3** | **13.57 s avg** | **0.00 %** (0/64) |
| distil-small.en | 1089-134686-0000 | 10.4 s | yes (161 chars) | 3.30 s | 3.6 % ("flower" for "flour") |
| distil-small.en | 1188-133604-0000 | 10.7 s | yes (115 chars) | 2.92 s | 10.5 % (2 subs, e.g. "Tishin" for "Titian") |
| distil-small.en | 121-121726-0000 | 8.5 s | yes (107 chars) | 2.45 s | 0.0 % |
| **distil-small.en total** | | | **3/3** | **2.89 s avg** | **4.69 %** (3/64) |

Start latency (hotkey → listening): 0.03–0.20 s. Volume restored OK in both runs. "All checks passed" in both runs.

`eqho-config\eqho.log`, distil-large-v3 run (every line the plan asks about):
```
22:27:23 src.model_host: Engine auto: graphics adapters [Intel(R) UHD Graphics 620 128 MB] -> faster-whisper
22:27:39 src.model_host: Model host ready: distil-large-v3 {'backend': 'faster-whisper', 'device': 'cpu', 'compute': 'int8'}
22:27:41 src.transcriber: Live preview off: engine not on CUDA and not measured yet
22:28:05 src.transcriber: Complete transcript: 162 chars (12.4s audio, transcribed in 13.47s)
22:28:05 src.transcriber: Slow-model tip shown: distil-large-v3 took 13.47s (suggested distil-small.en)
22:28:06 eqho: Injected 162 chars
22:28:08 src.transcriber: Live preview off: last transcription took 13.5s
22:28:32 src.transcriber: Complete transcript: 109 chars (12.2s audio, transcribed in 13.44s)
22:28:33 eqho: Injected 109 chars
22:28:57 src.transcriber: Complete transcript: 104 chars (10.0s audio, transcribed in 13.49s)
22:28:58 eqho: Injected 104 chars
```
distil-small.en run:
```
22:29:00 src.model_host: Engine auto: graphics adapters [Intel(R) UHD Graphics 620 128 MB] -> faster-whisper
22:29:06 src.model_host: Model host ready: distil-small.en {'backend': 'faster-whisper', 'device': 'cpu', 'compute': 'int8'}
22:29:08 src.transcriber: Live preview off: engine not on CUDA and not measured yet
22:29:22 src.transcriber: Complete transcript: 161 chars (12.4s audio, transcribed in 2.92s)
22:29:23 eqho: Injected 161 chars
22:29:25 src.transcriber: Live preview off: last transcription took 2.9s
22:29:39 src.transcriber: Complete transcript: 115 chars (12.2s audio, transcribed in 2.78s)
22:29:40 eqho: Injected 115 chars
22:29:53 src.transcriber: Complete transcript: 107 chars (10.0s audio, transcribed in 2.60s)
22:29:54 eqho: Injected 107 chars
```
- The slow-model tip appears **once** per run (first dictation only) and not with distil-small.en (2.6–2.9 s is
  below the 6 s threshold). No "Low free memory" warning, no "Dropped result", no errors.
- **Live preview stays off with distil-small.en too:** one call takes 2.6–2.9 s, above `PARTIAL_MAX_INFER_S = 1.5`.
  On this laptop only `base` (about 0.9–1.2 s per call) would get a live preview.
- "12.4s audio" for a 10.4 s clip is the clip plus the test's ~2 s of trailing silence.

Problems and how I handled them:
- **First attempt (21:58, on battery):** distil-large-v3 typed 3/3 with 0 % WER, but stop → text was
  **25.6–26.1 s** ("transcribed in 25.32s / 25.48s / 26.24s"). Logs are in `runs2\battery\e2e-distil-large-v3*`.
- **First attempt, distil-small.en: refused in 1 s, exit code 2:**
  `ERROR test_e2e: Model 'distil-small.en' is not cached — refusing to download it for an e2e run.`
  The plan says Eqho downloads the model itself and the test waits up to 5 minutes, but `test_e2e.py` (lines
  493–498) refuses uncached models. Fix: step 4 downloaded it into `KIT\models` (`bench_asr.py --allow-download`),
  and then the run above passed.

## Step 4: small models on the processor (faster-whisper, CPU int8, 4 threads; 60 + 60 clips)

Final runs on **AC power** (22:30–22:45). A CPU sampler (`runs2\cpu-samples.csv`, every 2 s) shows steady speed:
CPU performance 116–185 % of the 1.9 GHz base clock, about 55 % total CPU time (4 of 8 threads), with no slow phases.

| Model | WER clean | WER other | × realtime (clean / other) | Median s per clip (clean / other) | Short clips < 4 s, avg s (clean n=11 / other n=16) | Load s |
|---|---|---|---|---|---|---|
| **distil-small.en** | **3.89 %** (55/1414) | **5.99 %** (65/1085) | 3.43× / 2.53× | **2.64 / 2.66** | 2.45 / 2.55 | 5.6 / 6.3 |
| small | 3.61 % (51/1414) | 5.44 % (59/1085) | 2.58× / 2.11× | 3.40 / 3.19 | 2.91 / 3.04 | 4.9 / 4.9 |
| base | 4.60 % (65/1414) | 8.57 % (93/1085) | 6.93× / 6.03× | 1.15 / 1.03 | 0.96 / 0.94 | 2.2 / 2.2 |
| *run 1: distil-large-v3 (faster-whisper, CPU, AC)* | *2.48 %* | *4.52 %* | *0.48× / 0.48×* | *19.0 / 14.4* | *19.9 / 14.3* | *62.1 / 21.1* |

Main PC (for comparison, from the plan): distil-small.en 3.82 % / 6.18 % at ~2.5 s · small 3.68 % / 5.99 % at ~3 s ·
base 4.95 % / 8.48 % at ~1 s. **This laptop is about as fast as the main PC's CPU numbers for these small models**,
and WER agrees within 0.4 pp.

- All 360 clip transcriptions OK: 0 failed, 0 empty.
- The fixed cost per call dominates: the shortest clips (2.0–2.6 s of audio) take ~2.5 s with distil-small.en,
  ~2.9 s with small and ~0.9 s with base. Slowest single clip on AC: 3.7 s (distil-small.en) / 7.7 s (small) /
  3.6 s (base). 90th percentile: 2.9–3.1 s / 3.9–4.1 s / 1.3–2.1 s.
- Load times above are with the model already cached. The first load, including the download (battery runs), took
  21.3 s (distil-small.en, 320 MB), 41.2 s (small, 464 MB) and 14.5 s (base, 141 MB).

**On battery (first attempt, 22:02–22:25, kept in `runs2\battery\`).** Same WER, and every transcript was
identical to the AC run (60/60 per run), but speed was unstable: within single runs, per-clip time jumped between a
normal and a ~2× slower phase. For example, distil-small.en other went from 2.5 s to 5.5 s per clip from clip 7 on,
and base dropped from ~2 s to ~1 s near the end.

| Model (battery) | × realtime clean / other | Median s clean / other |
|---|---|---|
| distil-small.en | 3.73× / 1.31× | 2.38 / 5.48 |
| small | 2.20× / 1.55× | 2.95 / 3.43 |
| base | 3.48× / 3.22× | 2.31 / 2.05 |

So on battery, expect roughly 2.5–5.5 s per short dictation with distil-small.en (vs 26 s with distil-large-v3).
That is still usable.

## Step 5: UI test on this display (`tools/test_ui.py`)

Result: **FAILED (exit 1)**. 4 tabs visited, 34 controls exercised, 22 skipped, 13 screenshots, **23 errors, all
"Cut-off text"**, all from the "narrower window" re-check. At the default size (1762×1050 px) all 143 labels
passed (0 cut off).

Why the narrow check hits harder here: the test asks for 1100 physical px, which at 1.875 total scaling is 587 logical
px. The window's `minsize(600, …)` clamps that, so the check runs at the **minimum window width** (600 logical =
1125 px). On a 100 % display the same check would test about 880 logical px. The cut-offs are still real at the
minimum width:
- General: "Higher picks up quieter speec…", "Press to activate/deactivate di…", "Keep dictations in the History t…"
  (clipped, not wrapped).
- Models: card titles collide with their status ("Distil Small ENot downloade", "Large v3 Turbot downloac",
  "Distil Medium EN ● Reac"), and "✓ Ready" / "Not downloaded" are cut. The test also reports the model-folder path
  label as cut.
- Settings → About: value labels squeezed to 18 px ("0.8.9", "Distil Large v3", "Daniel Mevit", …) and the licence
  lines are cut at 243 px.

What the screenshots show (I looked at `ui-models.png`, `ui-models-narrow.png`, `ui-general-narrow.png`,
`ui-settings.png` and all `ui-setup-*.png`):
1. **The window does not fit above the taskbar.** At 1920×1080 with 150 % scaling, about 1008 px are free above the
   72 px taskbar, but the window is 1050 px tall. In every screenshot the taskbar covers the bottom ~95 px of the
   window. On the dashboard, that hides the bottom of the scrollable content (acceptable). In first-time setup,
   **none of the six pages shows the Skip setup / Back / Next / Finish row**: it is `pack(side="bottom")` in
   `onboarding.py:77` and lies behind the taskbar. A new user on a laptop like this may not find "Next".
   (150 % is Windows' recommended scaling for a 14" 1080p panel, so this is a common setup.)
2. **Setup text loses words at line ends.** `_text()` uses a fixed `wraplength=620` (`onboarding.py:163`), which at
   1.875 scaling is ~1160 px, wider than the setup body (~1005 px at this width). The setup pages were captured at
   the minimum width, because the test walks setup after its narrow re-check:
   - Welcome: "Everything stays on this computer — you say is sent anywhere." ("nothing" missing, so the privacy
     promise reads as its opposite)
   - Speech model: "…Distil Small EN, about 330 stays on this computer." ("MB. It" missing)
   - Try it: "Your words appear wher cursor is." ("e your" missing)

   The cut-off checker does not inspect setup pages, so it does not report these.
3. Otherwise the pages look right: recommended model "Distil Small EN — Recommended for this PC", the header "On this
   PC, speech models run on the processor — the smaller models are much faster here.", the download → "Done. The
   speech model is ready." flow, the sound page, and dark theme. ("Distil Medium EN ✓ Ready" comes from the test's
   own fake model folder, `test_ui.py:127`.)
4. The screenshots are screen captures, so the originals include the Windows taskbar (pinned app icons, clock). In the
   published copies, rows 953–1050 (taskbar plus off-screen strip) are covered by a labelled grey bar. The image size
   is unchanged, so the overlap stays visible. User-profile paths in all published logs are replaced with `KIT\…` or
   `C:\Users\<user>\…`.

## Errors, odd things, and notes

- **Battery:** the laptop ran on battery from the start (status "discharging", 78 % → 59 %) until 22:26. All
  results in the tables are from re-runs on AC. The keep-awake lock (SetThreadExecutionState) was held for the
  whole run.
- `test_e2e.py` refuses to download models (see step 3). The plan's statement that it downloads them itself is wrong.
- The plan's zip-comment command returns nothing in Windows PowerShell 5.1 (see step 1).
- **Run-1 files already published on the public repo:** the logs under
  `docs/ai/benchmarks/2026-10-16-intel-uhd620/` (`*.log`, `e2e/*.log`, `app-smoke.txt`; 10 files) contain the
  laptop's **Windows user name** inside `C:\Users\<name>\…` paths. The computer name does not appear. These are now
  scrubbed in branch `bench/intel-uhd620-run2` (separate commit), but **the user name is still in the git history of
  `dev`** (and in `Eqho-private`, branch `bench/intel-uhd620-2026-10-16`). Removing it from history needs a history
  rewrite and force-push, which is the owner's decision.
- Nothing new was installed. Disk use: `KIT\models` is now 2.39 GB (distil-large-v3 1446 MB, small 464 MB,
  distil-small.en 320 MB, base 141 MB, tiny 75 MB). Run 1's `%LOCALAPPDATA%\pywhispercpp` (1.59 GB) is still there.
  No `eqho_ui_test_*` folders were left in %TEMP%.

## My view: is Distil Small EN a good default for a laptop like this?

**Yes.** On this 2019 4-core laptop it turns a short sentence into typed text in **about 2.5–3.3 s on AC** (≈5.5 s
in the worst battery phase). distil-large-v3 needs 13.6 s on AC and 26 s on battery. That is the difference between
usable and not. Accuracy costs about 1.4 pp WER (3.89 % vs 2.48 % clean, 5.99 % vs 4.52 % other), with the typical
errors being homophones and rare names ("flower"/"flour", "Tishin"/"Titian").
- `small` is no better a choice for English: ~0.3–0.5 pp better WER, but ~25 % slower and 1.4× the download size.
  It is the right pick for non-English, as the code already does.
- `base` is 2.5× faster (~1 s, and fast enough for live preview), but WER on harder audio rises to 8.6 %.
  It is a reasonable "fastest" option, not the default.
- Suggestions:
  1. Fix the setup window height at 150 % scaling. Fit the window to the work area, or make the setup body
     scrollable with the nav row always visible.
  2. Make setup text wrap to the real width, e.g. with the existing `auto_wrap_label()` helper (`src/ui/widgets.py`,
     around line 387), instead of the fixed `wraplength=620`. Also let the cut-off check cover the setup pages, and
     make its narrow check use a logical width so it tests the same layout on every display scaling.
  3. Maybe allow live preview when one call is under ~3 s for short phrases, or just accept no preview on such PCs.
  4. Update `test_e2e.py` or the plan regarding downloads.

## Files

Published (branch `bench/intel-uhd620-run2` of `danielmevit/eqho`): this file and `runs2\` (logs, CSVs and
screenshots only, with no audio, models or settings files), in `docs/ai/benchmarks/2026-10-16-intel-uhd620/`.

In KIT on the laptop:

- `TEST-PLAN-2.md`, `eqho-dev.zip`, `src2\eqho-dev\` (source), `cfg-cpu\`, `cfg-e2e-distil-large-v3\`,
  `cfg-e2e-distil-small.en\` (BOM-less settings).
- `runs2\`
  - `pip-lock.log`, `pip-wheel.log`, `smoke.log`, `step2.log`
  - `e2e-<model>.log` and `e2e-<model>\` (`e2e-results.csv`, `eqho-config\eqho.log`, …): final AC runs
  - `cpu-<model>-<set>.log` and `cpu-<model>-<set>\asr-results.csv`: final AC runs
  - `timeline-cpu.csv`, `cpu-samples.csv` (CPU speed during the AC re-runs)
  - `ui.log` and `ui\*.png` (13 screenshots)
  - `battery\`: the first attempts on battery (step 3 and step 4), kept for reference
