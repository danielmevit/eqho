# Changelog

All notable changes to Eqho are tracked here.

Date format: `YYYY-MM-DD`.

## [0.3.2] - 2026-07-09

### Added
- **CodeGraph index** — repo is now CodeGraph-indexed for structural navigation (`codegraph init`; `.codegraph/` gitignored). Markdown no longer hand-maintains file maps.
- **`src/version.py`** — single source of truth for the version; the About tab reads it (fixes the displayed "0.3.0" lagging the documented 0.3.1).

### Changed
- **Docs restructured per the `_refs` AI-setup playbook** — new root `AGENTS.md` (short router) + `docs/ai/` intent files (`START_HERE`, `DESIGN_SYSTEM`, `DECISIONS`, `GOTCHAS`). `SOUL.md` and `TODO.md` moved to repo root; `agent-instructions/` removed (its rulebook content absorbed into the router + GOTCHAS + DECISIONS). TODO de-staled (license + docs-relocation items were long done).
- **ROADMAP renumbered** for the 2026-07 overhaul: Phase 3 = engine hardening (v0.3.x), Phase 4 = UI overhaul + Windows packaging (v0.4.x), Phase 5 = local features (v0.5.x), Phase 6 = cross-platform (v0.6.x), Phase 7 = public launch (→1.0), Phase 8 = whisper.cpp native engine (future).
- README structure tree replaced with a short pointer (CodeGraph owns structure); build.ps1 header no longer hardcodes a version.

## [0.3.1] - 2026-04-01

### Added
- **Custom ThemedDropdown widget** — replaces native `tkinter.Menu` popups with a fully themed dropdown. Rounded corners via DWM API, themed colors, scroll support for long lists, click-outside-to-close.
- **Tray icon theme detection** — reads `SystemUsesLightTheme` from Windows registry to pick white icon on dark taskbar, blue icon on light taskbar.
- **Window title bar icon** — `iconphoto()` from `logo_32_dark.png` at 16/32/48px sizes, replacing legacy `.ico`.

### Changed
- **Dashboard always-on-top disabled** — dashboard no longer stays above all windows. `_focus_existing()` temporarily sets topmost to steal focus, then disables after 100ms. Overlay remains always-on-top.
- **About tab responsive** — split into `_build_about_details()` and `_build_about_credits()` with 2-column grid layout on wider windows.

### Known Issues (deferred)
- ThemedDropdown closed state renders without rounded corners and small chevron indicator.
- Title bar icon may still show old icon due to OS/customtkinter cache — may need settings cleanup.
- Tray icon theme detection not yet taking effect — needs runtime debugging.
- Responsive tab rebuild requires resize + re-click on some tabs (General, About).
- Column spacing inconsistent across tabs — needs unified padding audit.

## [0.3.0] - 2026-03-31

### Added
- **Dashboard** — full settings window with sidebar navigation (General, Overlay, Models, History, About). Built on customtkinter for modern rounded widgets.
- **Theme system** — dark, light, and system (auto-detects Windows theme) modes. Horizontal pill switcher in the dashboard sidebar ("Light / Dark / System"). Theme applies to dashboard and overlay.
- **Eqho Design System** — consistent color palette, spacing, and radius tokens. Accent: Eqho blue (#58a6ff).
- **Model cards** in Models tab — detailed info per model with download status, language scope, size, device, recommendation text. Select directly from cards.
- **Overlay tab** — dedicated settings page with enable/disable switch, position dropdown, opacity slider, font size slider.
- **History tab** (placeholder) — coming in Phase 3, shows planned features.
- **About tab** — version info, tech stack credits, author link to GitHub profile.
- **Dashboard singleton** — clicking the tray icon when the dashboard is already open focuses the existing window instead of opening a duplicate. Thread-safe with `_opening` guard.
- **Responsive dashboard layout** — resizable window with 2→3 column grid based on window width (breakpoints at 560px, 900px). Tabs rebuild on resize.
- **Tab icons** — sidebar tabs and section headers display icons for visual clarity.
- **Status info bar** — tab headers show current model, hotkey, language, and mode at a glance.
- **New logo assets** — horizontal wordmark (140x60, dark/light variants), 32px "e" mark, 62px tray icon (blue on transparent).
- **Overlay rounded corners** — Windows 11 DWM API (`DWMWA_WINDOW_CORNER_PREFERENCE`) applied to the floating overlay bar.
- **Shutdown cleanup** — `shutdown_dashboard()` called before other teardown; `tk.Variable.__del__` patched to suppress RuntimeError spam on exit.

### Changed
- Tray left-click now opens Dashboard (previously toggled listening). Dashboard is the primary settings surface.
- Tray menu restructured: "Dashboard" is the default action, all settings accessible from the dashboard.
- Overlay bar is now theme-aware — colors update based on dark/light/system setting.
- Model selection no longer freezes the dashboard — settings callbacks run in a background thread.
- Theme switching now rebuilds the entire dashboard UI (sidebar + content) to apply colors consistently.
- Light theme contrast improved — gray canvas background (`#f0f2f5`) with white cards, darker secondary/muted text, more visible borders.
- Tray icon updated to use native 62px logo (no scaling). Inactive state dims to 40% brightness.
- Dropdown menus replaced with custom ThemedDropdown widget.
- Dashboard window is now resizable with minimize/maximize buttons.
- Bottom padding added to all dashboard tabs.
- `customtkinter>=5.2.0` added as a dependency.

## [0.2.1] - 2026-03-31

### Added
- **Hotkey customization UI** — press-to-capture key binding in the dashboard.
- **Start with Windows** toggle — adds/removes Eqho from the Windows registry Run key.
- **Model download progress notification** — tray notification when a model is being downloaded for the first time, and again when ready.
- **Graceful microphone error handling** — if the selected mic is unavailable, falls back to the system default and notifies the user.
- **Overlay position preference** — choose where the transcription overlay appears: bottom-center (default), top-center, or any corner.
- **Tray tooltip** — hovering the tray icon now shows the current hotkey and language.
- **Inter font** — bundled as the standard app typeface, loaded at runtime via Windows API.

## [0.2.0] - 2026-03-31

### Changed
- **Renamed project from Ekho to Eqho.** Updated all code, docs, config paths, and build scripts.
- Default hotkey changed to **`Alt+Q`** (Q for Eqho). Single left-hand combo, no conflicts.
- Default mic set to Realtek Mic Array (device 3) to avoid Bluetooth HFP profile switching.
- Default volume duck set to **Mute** while speaking.
- Config paths: `%AppData%\Eqho`, `D:\EqhoModels`.

### Added
- **Microphone selector** in tray menu — pick which mic to use for dictation.
- **Volume While Speaking** option in tray menu — Off, 50%, 25%, 10%, or Mute. Silently controls system volume via Windows Core Audio API (pycaw).
- Emergency unmute on app exit via `atexit` — system audio is never left muted if Eqho crashes or is force-closed.
- `pycaw` dependency for silent programmatic volume control.

### Fixed
- Hold mode no longer cuts off the last word. Worker thread drains audio queue and flushes before mic closes.
- Hotkey mode switching (toggle ↔ hold) no longer crashes. Rewrote hold mode to use a single `keyboard.hook()` to avoid the library's internal hook corruption bug.
- Toggle mode no longer double-fires. Added 400ms debounce.
- Settings changes (hotkey mode, paste mode) skip unnecessary model reloads.
- Volume control uses COM per-thread initialization to work from hotkey callback threads.

## [0.1.4] - 2026-03-30

### Fixed
- Text injection now pastes into the correct window (e.g. Word, browser) instead of the PowerShell terminal. Captures the foreground window handle on activation and restores focus before pasting.
- Modifier key release before paste — explicitly releases all modifier keys before simulating Ctrl+V to prevent ghost key conflicts.

### Changed
- Increased post-dictation delay (0.15s → 0.4s) to allow modifier keys to fully release before paste injection.

## [0.1.3] - 2026-03-30

### Changed
- Default model switched from `medium` to **`distil-large-v3`** (English-optimized, ~6x faster than large-v3 with <1% accuracy loss).
- Model menu reorganized: Distil models (English-optimized) listed first, then multilingual models (Turbo, Medium, Small, Base, Tiny, Large v3).
- CUDA device picker now routes all models except `large-v3` to GPU (distil and turbo models fit easily in 6GB VRAM).
- Roadmap updated: Phase 4 is now whisper.cpp migration for native distribution (replacing Python/faster-whisper for the commercial release).

### Added
- New models available in tray menu: `distil-large-v3`, `distil-medium.en`, `distil-small.en`, `large-v3-turbo`.

## [0.1.2] - 2026-03-30

### Fixed
- CUDA inference now falls back to CPU gracefully when `cublas64_12.dll` is missing. A smoke test runs at model load time instead of failing at first transcription.
- Removed Whisper's built-in `vad_filter` from transcribe calls -- it was too aggressive and discarding valid speech. Eqho's own energy-based VAD handles speech/silence detection.
- Lowered silence threshold from 0.01 to 0.003 RMS so quieter microphones are detected properly.
- Overlay now updates on completed segments (previously only updated on partials, so short phrases showed "Listening..." forever).
- Partial transcription now triggers every 1.5s of active speech (previously used a broken even-second-only check that rarely fired).
- Silenced noisy debug logging from PIL, httpx, httpcore, and huggingface_hub.

### Added
- Mic level diagnostic logging every 10 seconds while recording (RMS, peak, threshold, speaking state).
- CUDA Toolkit 12.9 added as a prerequisite for GPU acceleration (RTX 3060 verified working).

## [0.1.1] - 2026-03-30

### Changed
- Renamed project from Echo to **Eqho**.
- Updated all documentation to reflect faster-whisper (Whisper) as the transcription engine (replacing earlier Moonshine references).
- Updated tray icon to use the project logo instead of programmatic fallback.
- Updated config paths: `%AppData%\Eqho`, `D:/EqhoModels`.

## [0.1.0] - 2026-03-29

### Added
- Initial project scaffold with full dictation pipeline.
- faster-whisper integration for real-time speech-to-text (on-device, GPU + CPU, MIT license).
- System tray app with right-click menu: start/stop listening, model selection, hotkey mode, paste mode, language selection, overlay toggle, quit.
- Global hotkey support (`Ctrl+Shift+Space` default) with toggle and hold-to-talk modes.
- Floating overlay bar at screen bottom showing live partial transcription.
- Text injection into the active window via clipboard paste or simulated keystrokes.
- Settings persistence to `%AppData%\Eqho\settings.json` across sessions.
- Audio device enumeration via sounddevice.
- Multi-language support: 13 languages.
- Model selection: Tiny, Base, Small, Medium, Large v3.
- PyInstaller packaging support (`Eqho.spec` + `build.ps1`) for standalone `.exe`.
- Energy-based VAD for speech/silence detection with configurable thresholds.
