# Eqho in Rust — Plan (`0.9.x`, 2026-11-22)

Isolated build workspace: `D:\Vibe Coding\_vm\eqho\rust-09\` (branch `rust-0.9`, based on `dev` @ `434b7c6`, v0.8.11).
Parent Python workspace (`D:\Vibe Coding\_vm\eqho\{repo,coord,wt}`) and live user config (`%APPDATA%\Eqho`) remain **untouched** until parity switchover.

## 1. Principles
1. **Measure first (R0).** Prove `whisper.cpp 1.8.4` Vulkan + CPU speed/WER on real LibriSpeech clips (`asr-data/LibriSpeech`), idle RAM, startup latency, and per-pixel alpha pill animation on Windows before locking the full stack.
2. **Keep the out-of-process model host.** Inference always runs in a child process (`Eqho.exe --model-host`). Model/engine switches kill and respawn a fresh child so VRAM releases cleanly and native driver faults never crash the tray/UI.
3. **Isolated runtime config during `0.9.x` dev.** Default config dir during dev is `%APPDATA%\Eqho-Rust-0.9` (overridable via `EQHO_CONFIG_DIR`) with single-instance id `xyz.damt.eqho.rust09`. Never touches live `%APPDATA%\Eqho` or port `48317` until switchover.
4. **Fix the 7 audited `0.8.11` defects by design** (from `EQHO-RESEARCH-FABLE-20261116/lead-assessment.md`):
   - Surface mid-session/stop `ModelHostError` truthfully; preserve already-completed text.
   - Bounded pre-roll ring buffer (no leading silence accumulation) + bounded phrase cap + bounded audio queue.
   - Language-aware `clean_text(text, lang, remove_fillers)` (English `i` / fillers only for `"en"`; Unicode sentence capitalization).
   - Literal user replacements (safe for backslashes and punctuation terms like `C++`).
   - Verify `SetForegroundWindow` return & active HWND before pasting; keep an in-memory last-dictation recovery buffer independent of History.
   - Do not silently mutate Windows' global `UserDuckingPreference` registry key when `volume_duck == "off"`.
   - Real confidence gating on `whisper.cpp` using `whisper_full_get_segment_no_speech_prob` and `whisper_full_get_token_data`.
5. **Idle means idle.** Slint 1.19 `renderer-software` ONLY (never compile in `femtovg`). No dashboard window created until opened; pill overlay created lazily or hidden when idle.
6. **Versioning:** `0.9.0-dev` → `0.9.0` (parity switchover) → `0.9.x`. Never `1.0`.

## 2. Target Budgets & Measured R0 Results (Windows 11, RTX 3060 Laptop GPU, 2026-11-22)
| Metric | Python 0.8.11 Baseline | Rust `0.9.0` Budget | Measured R0 (`eqho-spike.exe` release, 8.6 MB) | Status |
| --- | --- | --- | --- | --- |
| Idle RAM (tray only, no model loaded) | ~150+ MB | ≤ 15 MB private | **3.42 MB private** (22.8 MB WS) | **PASS (4.4× under)** |
| Launch → tray icon ready | ~2–3 s | ≤ 250 ms | **38.1 ms** | **PASS (6.5× under)** |
| RTX 3060 `large-v3-turbo` (5 LibriSpeech clips) | 1.30–1.44 s, WER 1.05–1.86% | ≤ 2.0 s, WER ≤ 2.5% | **0.267–0.351 s** (mean 297 ms, **4.5× faster**), **1.39% WER** (71/72 words) | **PASS (4.5× faster)** |
| RTX 3060 `tiny` warm Vulkan | — | — | **90.3 ms** on 10.4 s audio (RTF 0.0087, 115× RT) | **PASS** |
| CPU fallback (`ggml-tiny.bin`, `threads=1`) | — | Clean fallback | **1.87 s** on 10.4 s audio (RTF 0.179) *(Note: prebuilt `ggml-cpu.dll` `OPENMP=1` faults at `threads>1`; clamped to 1 until R4 CI builds `-DGGML_OPENMP=OFF`)* | **PASS (safeguarded)** |
| Pill frame render (`280×51` layered BGRA) | ~5.5–6.6 ms (numpy + PPM) | ≤ 1.5 ms target / < 16 ms 60fps | **2.74 ms avg / 3.12 ms p95 / 3.60 ms max**, `focus_stolen: false` | **PASS (6× under 60fps)** |
| Installer / portable footprint | ~250+ MB (Python + CTranslate2 + Vulkan) | ≤ 30 MB zipped | **`Eqho-Setup-0.9.0-dev-win-x64.exe` 13 MB** (Inno Setup 6 LZMA2), portable zip **24 MB** (`Eqho.exe` 15.2 MB + 5 Whisper DLLs) | **PASS (~19× smaller)** |
| **Live `Eqho.exe --e2e-wav` (RTX 3060 Vulkan via `--model-host` IPC, 5 LibriSpeech clips)** | 1.30–1.44 s | ≤ 2.0 s, WER ≤ 2.5% | **271–387 ms** (mean **317.4 ms**, **4.2× faster** incl. IPC + textproc), **1.39% WER** (71/72 words exact), `no_speech_prob=3.5e-11`, `avg_logprob=-0.045..-0.132` | **PASS** |
| **Live `Eqho.exe --e2e-wav` Quantized `Q5_1` (`base-q5`, 60 MB)** | — | Clean Q5 CPU & GPU | Live WinHTTP download + GPU Vulkan **406 ms** (25.7× RT) / CPU 1T **7.06 s** on 10.44 s audio (exact text) | **PASS** |
| **Live `Eqho.exe --e2e-wav` CPU fallback (`tiny`, `--gpu false`)** | — | Zero crash | **3.05 s** on 10.44 s audio (RTF 0.292, Windows `OPENMP=1` thread clamp verified) | **PASS** |
| **Live `Eqho.exe --soak 30` (native Win32 layered pill + panel)** | — | Flat RAM | `ok: true`, **+0.16 MB** private RAM delta across 30 cycles | **PASS** |
| **Live WinHTTP GGML downloader** | — | Native HTTPS progress | Verified live HTTPS streaming download from HuggingFace with byte-level %, MB/s, ETA & atomic `.tmp.<pid>` staging | **PASS** |

## 3. Architecture (`rust/` Cargo Workspace)
```text
rust/
  Cargo.toml  rust-toolchain.toml  .cargo/config.toml
  vendor/whisper-1.8.4/whisper.h       exact header matching bundled whisper.dll 1.8.4 (MIT)
  crates/                              shared damt.xyz kit (reused from Ocy)
    damt-app          single instance, data paths, capped error log, panic hook
    damt-build        pure-Rust COFF .rsrc writer (manifest, icon, VERSIONINFO)
    damt-platform     base OS traits (clock, autostart, active screen, theme, notifier)
    damt-platform-win Windows impl of damt-platform
    damt-theme        tokens.toml -> theme.slint / tokens.css / tokens.rs
    damt-ui           app-neutral Slint 1.19 component kit
  apps/eqho/
    theme/            Eqho Toolcraft-inspired tokens.toml + generated outputs
    eqho-platform     FROZEN Eqho OS traits (audio, volume, hotkey, clipboard, injector, gpu, mem, mic privacy)
    eqho-platform-win Windows impl (WASAPI/cpal, COM IAudioEndpointVolume, WH_KEYBOARD_LL, EQ-MODS, EQ-CLIP, DXGI)
    eqho-core         pure logic: settings (0.8.11 JSON+BOM compat), history JSONL, textproc, adaptive VAD + bounded segmenter, pillfx, voice_features, chime, modelpick, modelstore
    eqho-host         out-of-process whisper.cpp 1.8.4 model host (IPC + GGML downloader + inactivity timeout + kill-on-switch)
    eqho-spike        R0 measurement binary (whisper.cpp benchmark + per-pixel alpha pill + Slint tray RAM)
    eqho-ui           R3 Slint 1.19 Dashboard (6 tabs + 6-step Onboarding + transcript text panel)
    eqho-app          R3/R4 final Eqho.exe binary (tray, coordinator, pill overlay, --model-host, --smoke, --soak)
```

## 4. Phases
- **R0 (Spike + Baseline):** `eqho-spike` on Windows — verify `whisper.cpp 1.8.4` FFI + Vulkan speed/WER on `asr-data/LibriSpeech`, per-pixel alpha pill frame rate, Slint software tray idle RAM.
- **R1 (Core):** `eqho-core` full pure-Rust port + `proptest` + Python `0.8.11` fixture parity tests.
- **R2 (Platform + Host):** `eqho-platform-win` + `eqho-host` (`--model-host` IPC, GGML HuggingFace downloader with real byte progress, DXGI/AVX2/RAM checks) + live Windows E2E dictation test (`EQHO_TEST_AUDIO`).
- **R3 (UI + Overlay + App):** Native per-pixel alpha pill + Slint text panel + Dashboard (General, Models, Settings, Overlay, History, About) + 6-step Onboarding + tray icon.
- **R4 (Parity + Packaging + Merge Decision):** `--smoke`, 30-cycle soak test, installer/portable zip + VC++ runtime/static-CRT resolution + third-party notices, merge `rust-0.9` into `dev` on owner go.

## 5. Worker Rules (Unio)
- Workers run `agy` with exact `--model gemini-4-hydrogen-medium`.
- Every build uses `CARGO_INCREMENTAL=0`, `CARGO_BUILD_JOBS=4`, and `CARGO_TARGET_DIR="$HOME/.cache/eqho-target/<worker>"` (never write targets onto `/mnt/d`).
- Never disable git hooks (`--no-verify`, `core.hooksPath`).
- Never claim Windows runtime checks passed unless actually run on Windows.
