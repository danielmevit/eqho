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
- [ ] Split `dashboard.py` into `src/ui/` (widgets / layout / tabs / orchestrator) with pub-sub context
- [ ] New token system: near-black dark ramp, hairline 1px borders, compact radii (4/6/8/12), `font()` helper
- [ ] Fix the 5 deferred v0.3.1 issues (dropdown polish, title-bar icon, tray theme refresh, responsive rebuild, unified padding)
- [ ] Dark title bar (DWM), new logo assets adoption, overlay restyle (pulse + fade)
- [ ] `packaging/windows/`: onedir PyInstaller spec + Inno Setup installer + portable zip
- [ ] GitHub Actions release workflow (tag → installer + zip artifacts)

## Phase 5 -- Local Features (v0.5.x)
All 100% local — no cloud, no server.
- [ ] Transcript history log (JSONL) + functional History tab (search, copy, delete, export)
- [ ] Whisper `initial_prompt` custom vocabulary setting
- [ ] Text replacements (user-defined, applied before injection)
- [ ] Voice commands ("new line", "period", "delete that") — opt-in
- [ ] Sound feedback chime (synthesized, cross-platform via sounddevice)

## Phase 6 -- Cross-Platform (v0.6.x)
Core dictation on Linux + macOS with graceful degradation.
- [ ] `src/oskit/` platform abstraction (volume duck, hotkeys, focus, autostart, fonts, theme)
- [ ] pynput hotkey backend (Linux/macOS; Windows keeps `keyboard` until validated)
- [ ] Platform-marked requirements; pystray backends per OS
- [ ] Linux tar.gz + AppImage · macOS .app + unsigned dmg
- [ ] CI build matrix (windows/ubuntu/macos) with headless `--smoke`

## Phase 7 -- Public Launch (→ v1.0)
- [x] Relocate internal docs (now root AGENTS/SOUL/TODO + docs/ai/)
- [x] Add AGPL-3.0 license
- [ ] Clean public README with screenshots, feature highlights, install instructions
- [ ] GitHub Releases page with pre-built installer
- [ ] winget manifest (needs public release) · MSIX/Store (needs signing) 
- [ ] CONTRIBUTING.md if accepting community contributions
- [ ] Optional: landing page, demo GIF/video
- [ ] Final QA pass on a clean Windows install
- [ ] Flip repo from private to public

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
- [ ] silero-VAD streaming segmenter (replace fixed RMS threshold)
- [ ] Streaming partials (LocalAgreement) with live injection
- [ ] Per-application paste mode rules (some apps need typing, not clipboard)
- [ ] Wayland-native hotkeys/injection
- [ ] Speaker identification (who's talking)
- [ ] Real-time translation (transcribe in one language, output in another)
- [ ] System audio capture (transcribe meetings/calls, not just mic)
- [ ] REST API mode for integration with other tools
