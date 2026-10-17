# Roadmap

Planned features and milestones for Eqho. Renumbered 2026-07-09 for the overhaul plan (see CHANGELOG 0.3.2).

## Phase 1 -- Core Dictation (v0.1.x) [done]
- [x] faster-whisper integration with energy-based VAD and streaming transcription
- [x] System tray with start/stop/quit
- [x] Global hotkey (toggle + hold-to-talk modes)
- [x] Floating overlay showing real-time partial text
- [x] Auto-paste into active window (clipboard + simulated typing)
- [x] Settings persistence to JSON
- [x] Multi-language support (13 languages)
- [x] Model selection (Tiny through Large v3, Distil models, Turbo)
- [x] PyInstaller packaging
- [x] CUDA GPU acceleration with automatic CPU fallback
- [x] Tray icon from project logo (active/inactive states)
- [x] Distil-Whisper as default (English-optimized, fastest high-quality model)

## Phase 2 -- Polish & Reliability (v0.2.x–v0.3.1) [done]
- [x] Audio device selector, volume ducking (pycaw), hotkey rewrite, focus capture/restore
- [x] Ekho→Eqho rename, hotkey customization UI, start-with-Windows, download notification
- [x] Overlay position preference, tray tooltip, mic error handling
- [x] **Dashboard** (customtkinter, sidebar nav, model cards, singleton)
- [x] Inter font bundled · theme system (dark/light/system) · responsive 1→2→3 col grid
- [x] New logo assets, overlay DWM rounded corners, light-theme contrast pass
- [x] ThemedDropdown widget, dashboard not-always-on-top, About tab responsive
- [x] Tray icon theme detection, window title bar icon (both shipped with known issues → Phase 4)

## Phase 3 -- Engine Hardening (v0.3.x) [done]
- [x] `platformdirs` for config + model cache (kills hardcoded `D:\EqhoModels`; migrates existing installs)
- [x] Model-load race fixes; model loading off the hotkey thread ("Loading model…" overlay state)
- [x] O(n²) audio buffer fix (chunk list); partials transcribe a bounded tail only
- [x] Hallucination gating (`no_speech_prob`/`avg_logprob`/peak-RMS + artifact blocklist)
- [x] Clipboard failure guards (fall back to simulated typing)
- [x] CPU warm-up transcribe (parity with the CUDA smoke test)
- [x] `run.py --smoke` headless verification gate (used by every later milestone + CI)

## Phase 4 -- UI Overhaul + Windows Packaging (v0.4.x)
Toolcraft-inspired restyle (original code only — see DECISIONS.md) + consumer-grade Windows installs.
- [x] Split `dashboard.py` into `src/ui/` (widgets / layout / tabs / orchestrator) with pub-sub context
- [x] New token system: near-black dark ramp, hairline 1px borders, compact radii (4/6/8/12), `font()` helper
- [x] Fix the 5 deferred v0.3.1 issues (dropdown polish, title-bar icon, tray theme refresh, responsive rebuild, unified padding)
- [x] Dark title bar (DWM), new logo assets adoption, overlay restyle (pulse + fade)
- [x] `packaging/windows/`: onedir PyInstaller spec + Inno Setup installer + portable zip
- [x] GitHub Actions release workflow (tag → installer + zip artifacts)

## Phase 5 -- Local Features (v0.5.x) [done]
All 100% local — no cloud, no server.
- [x] Transcript history log (JSONL) + functional History tab (search, copy, delete, export)
- [x] Whisper `initial_prompt` custom vocabulary setting
- [x] Text replacements (user-defined, applied before injection)
- [x] Voice commands ("new line", "period", "delete that") — opt-in
- [x] Sound feedback chime (synthesized, cross-platform via sounddevice)

## Phase 6 -- Cross-Platform (v0.6.x) [done]
Core dictation on Linux + macOS with graceful degradation.
- [x] `src/oskit/` platform abstraction (volume duck, hotkeys, focus, autostart, fonts, theme)
- [x] pynput hotkey backend (Linux/macOS; Windows keeps `keyboard` until validated)
- [x] Platform-marked requirements; pystray backends per OS
- [x] Linux tar.gz + AppImage · macOS .app + unsigned dmg
- [x] CI build matrix (windows/ubuntu/macos) with headless `--smoke`

## Next versions (owner plan 2026-10-16) — stay in 0.8.x while in beta
Order: fix what we have → first launch + speed on every PC → languages → interface languages → competitor research
and new features. The Microsoft Store comes after that, once the app is as good as we can make it (owner
2026-10-16: "lets first get it as optimized as we can and than launch"). Every change is checked with the test
system (docs/ai/TESTING.md: ASR benchmark, end-to-end dictation test, UI walker) before release.

### 0.8.9 — reliability + test system + AMD/Intel engine (finishing, on dev)
- [x] Model host: no stale replies after a timeout; first-run download can't time out (EQ-HOST)
- [x] Model change no longer crashes (EQ-DASH)
- [x] Sessions always clean up: audio never stays muted, no text leaks into the next dictation, transcripts kept out
      of the log (EQ-SESSION)
- [x] Settings saved safely, survive a locked or corrupt file (EQ-SETTINGS)
- [x] Test system: ASR accuracy/speed benchmark, end-to-end dictation test, UI walker (EQ-TESTASR/E2E/UI)
- [x] End-to-end + UI tests on the desktop (10/10, WER 1.05%; 28 controls, 0 errors)
- [x] Bigger accuracy run (GPU 200+200 clips, CPU-only 60+60) — numbers in CHANGELOG/LOG, no public claims yet
- [x] Pinned release dependencies, CI Python 3.13, macOS Cmd+V paste, safer clipboard restore (EQ-CI, EQ-INJECT)
- [x] whisper.cpp/Vulkan engine in the Windows downloads (EQ-AMD)
- [x] Second launch opens the running Eqho; plain microphone messages (EQ-OPEN); wrapped help texts, all CPU
      cores, timing in the log (EQ-POLISH)
- [ ] **Slow PCs (Intel laptop test, 2026-10-16):** never lose the last sentence, no live preview when slow
      (EQ-FINISH); Auto uses the processor on built-in graphics, whisper.cpp models in Eqho's folder, Custom
      Vocabulary on whisper.cpp, settings with a BOM (EQ-SLOWPC)
- [ ] Desktop test with a slow engine on the main PC, then a short re-run on the Latitude (fix + small/base models)

### 0.8.10 — first launch + speed on every PC
- [x] **First-launch onboarding** (EQ-ONBOARD, on `next`; screenshots review pending) (must, same as Vecto/Laydown): welcome → **which language do you speak?** (picks the
      model) → microphone test with a level meter → model download with consent, size and progress → audio lowering
      on/off (consent) → try it ("press Alt+Q and say something") → done. Skippable, reopen from the dashboard.
- [x] **Automatic model choice by hardware** (EQ-PICK + EQ-SLOWHINT tip; confirmed on the Intel laptop 2026-10-16: distil-small.en types ~2.9 s after stop vs 13.6 s) (moved up from 0.8.11): NVIDIA → distil-large-v3 on CUDA; graphics
      card → whisper.cpp; processor only → a smaller fast model. Shown and changeable in settings. Measured
      2026-10-16: distil-large-v3 on the processor takes 11–14 s per 10 s sentence (Ryzen 7 5800H) and 14–19 s per
      sentence on an Intel i7-8665U laptop — too slow. Benchmark small/base/distil-small (speed + accuracy) on both
      with tools/bench_asr.py first, then pick.
- [x] Keep the model ready after start — already true: the model preloads at startup (ready ~17 s after launch on NVIDIA) and is never unloaded
- [x] Low free memory (EQ-LOWMEM): warn in plain words before loading the model on the processor (needs about 1.5–2 GB free;
      seen 2026-10-16: on a full PC the model stopped partway)
- [ ] (low priority) whisper.cpp: shorter audio window for short sentences (`audio_ctx`). Since Auto now uses whisper.cpp only on real graphics cards, which are already fast, this matters little; faster-whisper has no such option
- [x] Version number in Eqho.exe's file properties (EQ-EXEVER; CI checks it)
- [x] Probes write into a temporary folder, never the real eqho.log (EQ-PROBEENV + tools/check_probe_isolation.py)

### 0.8.11 — the dictation pill (owner 2026-10-17)
- [ ] Text box above the pill stays see-through by default; options to customise it (see-through amount, text size,
      maybe width)
- [ ] Pill size option; default about 20 % smaller than today
- [ ] Waves shaped by the voice itself: the line(s) inside the pill follow the live microphone waveform, not just the
      loudness

### 0.8.12 — languages (speech)
- [ ] Non-English: multilingual model when a non-English language is chosen; offer every language Whisper supports
      (~99), not only 13

### 0.8.13 — the app in many languages (interface)
- [ ] Move all UI text (dashboard, onboarding, tray, overlay, messages) into translation files
- [ ] Translate the interface, then grow the list; follow the Windows display language, with a picker in
      onboarding and settings. Wave 1 languages: **main** — English, German, Spanish, French, Italian, Portuguese, Dutch, Polish, Russian, Ukrainian, Turkish, Japanese, Chinese (Simplified), Korean; **Balkan** — Macedonian, Serbian (Latin only), Croatian, Slovenian, Bulgarian, Albanian, Romanian, Greek (no separate Bosnian: Serbian and Croatian cover it); **Central Europe** — Czech, Slovak. Later waves: as many more as quality allows (owner: "as huge as it can be").
- [ ] Speech: make sure every wave-1 language is offered for transcription with the multilingual model (Whisper
      covers all of them; accuracy is lower for smaller languages such as Macedonian and Albanian — measure with the
      ASR benchmark on public recordings where available)

### Later 0.8.x — competitor research, then new features
- [ ] **Competitor scan:** features and approach of other dictation apps (open-source ones by reading their code for
      ideas only — never copy code), what users praise/complain about; turn it into a feature plan
- [ ] Voice commands and clean-up: "new line", "full stop", "comma"; remove "um/uh"; capitalisation
- [ ] Friendly "words to know" list (names, products, jargon) + fixed replacements (today: hidden initial_prompt)
- [ ] Demo GIF + README/Store screenshots

### Microsoft Store — after the app is as good as we can make it
- [ ] Store package (MSIX, microphone capability) + listing text (no competitor names, no unverifiable claims)
- [ ] Privacy page on the site; AI disclosure in the listing
- [ ] Pricing decision (one-time, like Vecto/Laydown)
- [ ] Code signing

## Phase 7 -- Public Launch (→ v1.0)
- [x] Relocate internal docs (now root AGENTS/SOUL/TODO + docs/ai/)
- [x] Add AGPL-3.0 license
- [x] Clean public README with screenshot, feature highlights, install instructions
- [x] GitHub Releases page with pre-built installers (v0.6.3+, all 3 OSes)
- [x] winget manifest submitted (microsoft/winget-pkgs PR #400796) · [ ] MSIX/Store (needs signing) 
- [x] CONTRIBUTING.md
- [x] Landing page (danielmevit.github.io/eqho, Astro in site/) · [ ] demo GIF/video (Daniel — see TODO.md)
- [ ] Final QA pass on a clean Windows install (Daniel — see TODO.md)
- [x] Repo public; since 2026-10-16 one repo — `dev` is public too (Eqho-private archived)

## Phase 8 -- Native Engine (future)
Migrate off the Python runtime for a lean commercial-grade product.
- [ ] Replace faster-whisper with **whisper.cpp** (zero Python dependency; Metal on macOS)
- [ ] Evaluate Tauri (Rust) or C#/WPF for the app shell (replacing tkinter + pystray)
- [ ] Signed binaries, auto-update mechanism
- [ ] Plugin system for custom post-processing (punctuation cleanup, formatting)

### Why whisper.cpp for distribution
- Pure C/C++, no Python runtime, no 200MB+ PyInstaller bundle
- Runs fast on CPU out of the box; supports CUDA, DirectCompute, and Vulkan for GPU
- Native bindings for C#, Rust, Go, Node.js (Whisper.net for C#, whisper-rs for Rust)
- End-user experience: install and run, no venv, no pip, no CUDA Toolkit headaches
- Current faster-whisper stack is ideal for development velocity; whisper.cpp is for shipping

## Stretch Goals / Backlog
- [x] Text cleanup — Tier 1 deterministic auto-format (v0.7.0)
- [x] Model-switch reliability — subprocess model host, seamless (Option B, v0.8.0). Pluggable backend.
- [ ] AMD/Intel GPU support — whisper.cpp/Vulkan backend (scaffolded in model_host; needs Vulkan build + packaging + AMD-hardware verification). Shared engine with Eqho Mobile / desktop Phase 8.
- [ ] Text cleanup — Tier 3 optional local-LLM "polish" mode (opt-in, may rephrase; parked)
- [ ] Eqho Mobile — private voice keyboard (KMP + Compose, whisper.cpp engine; Android-first). See feasibility notes.
- [ ] silero-VAD streaming segmenter (replace fixed RMS threshold)
- [ ] Streaming partials (LocalAgreement) with live injection
- [x] Per-application paste mode rules (v0.7.4)
- [ ] Wayland-native hotkeys/injection
- [ ] Speaker identification (who's talking)
- [ ] Real-time translation (transcribe in one language, output in another)
- [ ] System audio capture (transcribe meetings/calls, not just mic)
- [ ] REST API mode for integration with other tools
