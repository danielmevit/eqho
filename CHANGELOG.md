# Changelog

All notable changes to Eqho are tracked here.

Date format: `YYYY-MM-DD`.

## [Unreleased]

## [0.8.8] - 2026-07-14

The dashboard, restructured: top pill navigation + Phosphor icons (structure from Daniel's ref/ui — Eqho's own palettes, warmth from the accent blue).

### Changed
- **Sidebar → top bar.** Logo left; a **centered pill nav** with the three main sections — General, Models, History — each with a Phosphor icon, the active segment an accent-filled pill; top-right: a **sun/moon theme toggle** (one click flips light↔dark) and a **gear** opening the Settings view. No search — deliberately.
- **Settings view (gear)** hosts everything that isn't a main section: Theme (Light/Dark/System — System still lives here after the quick toggle), the full **Overlay** controls, and **About**, embedded as icon-labeled sections. Old deep-links (`--tab=overlay/about`, tray) transparently alias to it.
- **Phosphor Icons** (phosphoricons.com, MIT) vendored as a TTF in `assets/fonts` — auto-registered by the existing font loader, rendered as text glyphs: crisp at any UI zoom, theme-colorable. Tab headers now carry accent-colored icons; credits updated.
- Content area is full-width (no sidebar), so the responsive grid gets more room at every window size.
- **Polish round (Daniel's review):** nav segments paint their rounded pill correctly in every state (CTk "transparent" labels don't repaint when the parent fill changes — labels now get explicit backgrounds); the top-right theme/gear buttons are small tight circles (28 px) instead of pills; pages no longer repeat their own section title — just a slim model·hotkey·language status line, then the options (embedded Overlay/About sections drop their inner labels too); **UI Zoom moved from General into Settings**, sitting beside Theme in a two-column Appearance row; **History gained a date filter** — a period dropdown (Today / Yesterday / Last 7 / Last 30 days) plus the search box now also matches date stamps, so typing `2026-07-14` finds a day directly.

## [0.8.7] - 2026-07-14

All 99 Whisper languages, honestly tiered.

### Added
- **Three-tier language picker** (`LANGUAGE_TIERS` in settings.py): **hand-tuned** (the original 13 — tested in Eqho, dictation-grade), **strong accuracy** (20 more — excellent published Whisper accuracy, lightly tested here: Dutch, Polish, Turkish, Hindi, Indonesian, Thai, Hebrew…), **experimental** (the remaining 66 — quality varies a lot, clearly labeled, never oversold). The dashboard dropdown shows all 99 grouped under muted section headers (the themed dropdown gained non-clickable header rows; it already scrolled).
- **English-only guard**: picking a non-English language while a distil/`.en` model is active shows a warning under the picker ("This model is English-only — pick a multilingual model in Models"), updating live when either side changes. New `is_english_only_model()` helper.
- Tray keeps the tuned 13 (99 radio items would be unusable) plus the currently active language when it's from a lower tier, and an "All 99 languages… (Dashboard)" entry.

### Changed
- README **Languages** section and the website now state the tiers explicitly — 13 tuned, 20 strong, 66 experimental — instead of "13 languages".
- **Windows release artifacts now say what they are**: `Eqho-Setup-<version>-win-x64.exe` and `Eqho-portable-<version>-win-x64.zip` (was no platform in the name — confusing next to the Linux/macOS files). Release workflow globs and the website's download matching already accept both patterns; remember the winget manifest references the installer URL on the next winget bump.

## [0.8.6] - 2026-07-14

Light-pill polish + a brand-new product website. Released together with the 0.8.3–0.8.5 overlay work (the pill era begins).

### Fixed
- **Light pill top-edge flicker** — the pill window's color-key was near-black, so anti-aliased edge pixels blended toward black; on the white pill that painted a pulsing dark fringe as the capsule breathed. The key now matches the theme (near-white in light, near-black in dark).
- **Light pill shine parity** — full-strength specular sheen in both themes (light was reduced 40%).

### Added
- **White border** on the light pill (3 px, applied post tone-map so it stays crisp).
- **Light pill translucency** — translucent but not transparent: 90% visible (window alpha; the panel stays at the user's overlay opacity). Mirrored in Eqho Mobile v0.5.3.
- **Website rebuilt as a product page** (`site/`): plain-language copy that sells the app instead of describing the repo; big responsive type; a recreated overlay (live panel + the animated listening pill) as the hero; the pill woven in for both themes (seamless-loop WebP renders from `pillfx`, 121/139 KB); a Downloads section with OS icons whose links resolve to the newest GitHub release assets via a tiny progressive-enhancement script (static fallback: the releases page); FAQ; footer. Mobile/tablet responsive throughout.
- **README "Under the hood"** — the tech specifics (engines, model host, VAD, textproc, packaging) now live in the README; the website stays non-technical on purpose.

The pill goes on-brand: blue, vibrant, and smaller.

### Changed
- **Brand-blue palette** (anchored on theme `ACCENT #58a6ff` / light `#0969da`) replaces the purple look in both themes: navy body, indigo drift, azure main ribbon, ice-cyan secondary, electric-blue caustic arc, blue rim — multiple blue shades, no purple. Dark additive gains rebalanced so the shades stay saturated and separated instead of piling into a pale haze.
- **Light theme pops now** — deeper pigment ribbons at stronger mix, frost wash cut from 0.16 → 0.06; vibrancy comparable to dark.
- **Pill resized** ~20% shorter and ~30% narrower (400×64 → 280×51), per Daniel.
- Mirrored 1:1 in Eqho Mobile v0.5.2 (AGSL shader + gradient fallback).

The pill now genuinely rides your voice.

### Fixed
- **The pill reacted barely-visibly to real dictation.** The transcriber's level signal is heavily compressed — `(rms/0.012)^0.6` pegs near 1.0 for any voiced sound — so the pill sat swollen-and-static while talking. The overlay now recovers the voice dynamics with a temporal-contrast envelope: a fast syllable follower against a slow speech baseline produces onset punches plus a sustained-voice floor — talking swings the pill through ~0.2–0.9 at roughly syllable rate, pauses let it rest (validated against synthetic speech before shipping). The transcriber itself is untouched.

### Added
- The ribbons **speed up with your voice** — animation phase is integrated per frame (`1 + 0.9×level`), so tempo rises smoothly while you speak and settles when you pause, with no visual jumps.

The dictation overlay, redesigned: the iridescent pill + the upward-filling live panel — the same look Eqho Mobile ships, now on desktop.

### Added
- **The pill** (`src/pillfx.py` + overlay rewrite) — the old dot-and-level-bar overlay is now a breathing glass capsule running an iridescent fluid animation (undulating chromatic-fringed ribbons, lower caustic arc, fresnel rim, glassy sheen — translated from the `ref/anim` orbs). The live mic level drives ribbon amplitude, fringe width and breathing. Dark theme = additive indigo glow; light theme = pigment on frosted pearl. Pure numpy → PPM → `tk.PhotoImage`, ~12 ms/frame at 25 fps, no new dependencies. On Windows the pill window is color-keyed transparent, so the capsule floats free of any plate.
- **The live panel** — partial transcription now lands in a separate rounded panel stacked toward screen center from the pill (above it for bottom anchors), **filling upward** line by line as you speak, Gemini-style. Appears with the first real words, tails long dictations (latest words win), follows theme + `overlay_font_size` + `overlay_opacity`.

### Changed
- `TranscriptionOverlay` public API unchanged (`start/show/update_text/set_level/hide/shutdown`) — `main.py` needed no changes. `overlay_position` still honored (the pill anchors; the panel stacks toward center). `overlay_show_level=False` now pins the pill's voice reactivity instead of hiding a bar (the idle animation remains).
- Mirror rule: the pill math lives in both `src/pillfx.py` (desktop) and the mobile AGSL shader (`eqho-mobile` RecordingPill) — tune them together.

## [0.8.2] - 2026-07-12

Completes and hardware-verifies the dual-engine arc — whisper.cpp/Vulkan (cross-vendor GPU, incl. AMD) now runs and is verified alongside faster-whisper, with a UI engine picker and seamless runtime switching. First release since 0.6.9, so it also carries the v0.7.x–0.8.x work (auto-format, per-app paste rules, mic sensitivity, subprocess model host). (0.9.0 is reserved for a larger milestone.)

### Added
- **CONTRIBUTING.md** — dev setup, smoke-gate requirement, architecture rules (oskit-only OS code, theme-only styling, Variable helpers), bug-report guidance incl. eqho.log thread dumps. Phase 7 is now complete except Daniel's manual QA + demo GIF.
- **winget manifests** (`packaging/winget/`, PackageIdentifier `damt.Eqho`) — submitted to microsoft/winget-pkgs as [PR #400796](https://github.com/microsoft/winget-pkgs/pull/400796); once merged, `winget install eqho` works. For future releases: bump PackageVersion + InstallerUrl + SHA256 (`sha256sum` of the release exe, uppercase) and resubmit via a fork branch (see the PR for the pattern).
- **Inference Engine picker (UI) + seamless runtime engine switching.** The General tab's Model card now has an **Inference Engine** dropdown — Auto / faster-whisper (NVIDIA·CPU) / whisper.cpp (AMD·Intel·CPU); shows "not installed" when `pywhispercpp` is absent. Switching respawns the subprocess model host on the new backend with no app restart, via new `ModelHost.set_backend()` + `VoiceTranscriber.set_engine()`, reconciled in `App._on_settings_changed` (a no-op unless `engine_backend` actually changed, so mic/model changes are unaffected).
- **whisper.cpp/Vulkan backend is now runtime-verified end-to-end** (supersedes the v0.8.1 "not yet verified" note). A Vulkan-enabled `pywhispercpp` wheel builds + imports, and dictation transcribes on Vulkan: `ggml_vulkan: Found 2 Vulkan devices` (NVIDIA RTX 3060 + **AMD Radeon**), proving the AMD-support goal. A runtime engine switch (faster-whisper → whisper.cpp) was verified transcribing through the real model host. Build notes in `TODO.md`: the old failure was Windows **MAX_PATH(260)** at the nested `vulkan-shaders-gen` link step (not compiler inheritance); a self-contained redistributable wheel is produced by `mkwheel.py` (DLLs co-located at wheel root).
- **Windows build now bundles whisper.cpp.** `packaging/windows/eqho-win.spec` collects `pywhispercpp` + all five whisper/ggml DLLs — **explicitly**, since PyInstaller's static analysis misses the runtime-`LoadLibrary`'d `ggml-vulkan.dll`/`ggml-cpu.dll`. A build was verified to place all six files in `dist\Eqho\_internal\`; the Inno installer already globs the folder, so no `installer.iss` change was needed. The wheel builder is committed at `tools/mkwheel.py`. Builds without pywhispercpp still succeed (whisper.cpp simply absent → `engine_backend` "auto" uses faster-whisper).

### Changed
- **Landing page moved into `site/`** (was scattered at the repo root with `src/pages/` inside the Python package) — Pages workflow builds `./site`; screenshot copied to `site/public/assets/` so the page's relative image path resolves; page links updated to `danielmevit/eqho`.
- Agent docs rewritten for handoff: START_HERE now reflects the v0.8.x engine arc (dual-engine, subprocess model host) with whisper.cpp/Vulkan hardware-verified; GOTCHAS gains the tkinter default-root deadlock trap, watchdog usage, single-instance port, adaptive VAD, and the web-UI-edits-on-main sync rule.

### Fixed
- **Dropdown popups now size correctly at UI zoom.** The themed dropdown's popup is a raw `tk.Toplevel` sized in real screen pixels, but its CustomTkinter contents render at the global widget scaling (1.5 at the default 150% zoom) — so at zoom the popup box was too short (the last item clipped off the bottom) and too narrow (item text truncated). This made the **whisper.cpp** engine option look like it was missing entirely. The popup now multiplies its width/height (and the off-screen check) by the widget scaling, so every item and full label shows at any zoom level. Affects all themed dropdowns (engine, model, language, etc.).

## [0.8.1] - 2026-07-12

### Added
- **whisper.cpp backend wired (dual-engine).** The model host's second backend is now real (via `pywhispercpp`): cross-vendor GPU through Vulkan (AMD/Intel/NVIDIA) or CPU — the same engine as Eqho Mobile. `engine_backend` is now **"auto"**: NVIDIA+CUDA → faster-whisper (fastest there), otherwise whisper.cpp if installed, else faster-whisper CPU. Includes Eqho→whisper.cpp model-name mapping (distil variants → closest standard model) and safe-default segment confidence (whisper.cpp exposes text only).
- The app runs unchanged without pywhispercpp installed (auto stays on faster-whisper). Activating whisper.cpp needs a Vulkan-enabled `pywhispercpp` build — see TODO/setup. **The whisper.cpp backend code itself is not yet runtime-verified** (pending the binding install); resolve/auto-detect and the faster-whisper path are verified.

## [0.8.0] - 2026-07-11

### Changed
- **Model switching is now seamless — no restart (Option B: subprocess model host).** The Whisper model runs in a CHILD process (`src/model_host.py`); switching models kills that child and spawns a fresh one, which only ever loads ONE model — the only reliable path on CUDA stacks where a second in-process model crashes natively. The main app never loads a model, so a native inference crash kills only the child (auto-respawned), never the app. Replaces the v0.7.2 restart-on-switch flow: model changes now swap in the background (~a few seconds; overlay shows "Loading model…") while the dashboard stays open.
- The transcriber no longer imports faster-whisper in the main process (only the child does), so the app starts lighter and never holds CUDA in the UI process.

### Added
- **Pluggable inference backend** behind the model host. `faster-whisper` (NVIDIA CUDA / CPU) is the default; a **`whisper.cpp` backend** is scaffolded for **AMD/Intel GPUs via Vulkan** (and the shared mobile engine / desktop Phase 8). Selectable via the `engine_backend` setting. The whisper.cpp path still needs a Vulkan-enabled build wired + packaged and can't be verified without AMD hardware — see ROADMAP.
- `multiprocessing.freeze_support()` in run.py (required so the packaged exe spawns model-host children, not whole-app copies).

### Note
- AMD GPU acceleration: the current faster-whisper/CTranslate2 engine is CUDA-only. AMD requires the whisper.cpp/Vulkan backend (scaffolded) — the next engine milestone.

## [0.7.4] - 2026-07-11

### Added
- **Mic Sensitivity slider** (General → Dictation) — scales the adaptive VAD threshold from Low (needs louder speech, for noisy rooms) to High (picks up quieter speech, for soft mics). Directly tunable if speech isn't triggering.
- **Per-App Paste Rules** (General → Dictation) — force simulated typing or clipboard paste for specific apps by executable name (e.g. `slack.exe = typing`), overriding the global default. The target app is detected by process name at injection time.

## [0.7.3] - 2026-07-11

### Changed
- **The dashboard reopens after a model-change restart** — it comes back to the tab you were on (e.g. Models), instead of leaving you at just the tray. The restart passes `--open-dashboard --tab=<name>` to the fresh process, which reopens the dashboard ~1.2s after launch. (Tray-initiated model changes still just restart to the tray, since you weren't in the dashboard.)

## [0.7.2] - 2026-07-11

### Fixed
- **Model switching is now crash-proof (Option A: restart-on-switch).** Changing the Whisper model cleanly restarts the app so the new model loads in a fresh process — the ONLY reliable path on CUDA stacks where loading a second model in-process crashes natively. A one-time dialog explains the restart, with a **"Don't show this again"** checkbox that's remembered; after that, model changes just restart silently. Tray model changes restart too (with a notification).
- **Language changes no longer reload the model** — the language is passed per-transcription, so switching it is now instant (it was needlessly triggering the fragile model reload).

### Note
- The seamless alternative (Option B — subprocess model host, no restart) remains on the backlog as a future upgrade; restart-on-switch is the reliable fix for now.

## [0.7.1] - 2026-07-11

### Fixed
- **Lost transcription during/after a model switch** — a rapid model reload could null the model at the exact moment a transcribe ran, raising `'NoneType' object has no attribute 'transcribe'` and silently dropping the text. Transcribe calls now capture the model first and skip gracefully (logged) instead of erroring. (The model loads fine; this was the reload nulling it mid-call.)

### Note
- The deeper model-SWITCH native crash (loading a 2nd CUDA model in-process — see GOTCHAS) is separate and still pending the process-isolation fix (restart-on-switch vs subprocess host).

## [0.7.0] - 2026-07-11

### Added
- **Auto-format (Tier-1 text cleanup)** — a light-touch, fully deterministic pass over the final transcript before it's typed: fixes sentence capitalization, lone "i" → "I", and mechanical spacing (no space before punctuation, collapsed runs, chunk-seam casing). It is **safe by design — it never rephrases, reorders, or invents words** (idempotent; leaves "had had", "iOS", code-like text intact). On by default; toggle in General → Dictation. Lives in `src/textproc.py` so it's shared logic that ports to a future mobile keyboard.
- **Remove filler words** (opt-in sub-toggle) — strips a conservative set ("um", "uh", "er", "erm", "hmm", …) when auto-format is on; deliberately excludes meaning-carrying words ("like", "so", "well").
- Smoke gate now asserts cleanup behavior + idempotence.

### Note
- LLM "polish" (a local model that could rewrite the transcript into cleaner prose) is intentionally NOT included — parked on the roadmap. Dictation returns exactly what you said.

## [0.6.9] - 2026-07-11

### Changed
- GitHub rename adopted everywhere: `danielmevit/eqho` (was `DanielMevit/Eqho`) — remotes, README, installer support URL, About link, agent docs.
- Tray logo assets renamed to their true size: `logo_64_dark/white.png` (were misnamed `logo_62_*`); all code references updated. Future logo exports should use the `_64` names.

## [0.6.8] - 2026-07-11

### Fixed
- **"Mic shows signal but nothing transcribes"** — the speech gate was a fixed RMS threshold (0.003) that quiet mics/voices never crossed. The VAD threshold is now **adaptive**: it tracks the ambient noise floor and triggers at 3.5× it (clamped between 0.0009 and the old 0.003), so quiet setups transcribe reliably while noisy rooms behave as before. Hallucination gating scales with the live threshold, and the periodic mic-level log line now includes the adaptive threshold + noise floor for diagnosis.

### Changed
- **Audio-level bar recalibrated** — normal speaking volume now lands around the **middle** of the overlay (curve `(rms/0.012)^0.6`); only loud speech approaches full width. Nobody should have to shout at a dictation app.

## [0.6.7] - 2026-07-10

### Fixed
- **THE model-switch freeze — root cause caught by the watchdog and eliminated.** Thread dumps showed the dashboard thread deadlocked inside `StringVar.set` → `tk.globalsetvar`: the overlay's window (created first, on its own thread) became tkinter's default root, so every dashboard Variable/font silently belonged to the overlay's interpreter — cross-thread Tcl calls that intermittently deadlocked and made the whole dashboard sluggish. The overlay now releases the default-root claim, all tab Variables bind explicitly to the dashboard's root, and the constant cross-interpreter chatter (the "heavy/laggy" feel) is gone.
- **Start chime is now reliable** — mic starts first, the blip plays to completion, THEN volume ducking mutes the output (the fire-and-forget blip raced the mute and was sometimes silent).
- **Overlay bottom-center is exact** — positions anchor to the Windows work area (excludes the taskbar) with a tighter 24 px margin.
- **Multi-line overlay text aligns left** (was centered per-line), long dictations show the **latest tail** with a leading ellipsis, and the box re-fits/repositions as lines wrap.

### Changed
- **Livelier audio-level line** — sqrt loudness curve, up to 90% of the overlay width (was 35%), faster attack/gentler release.
- **Snappier partials** — live preview updates every 1.0 s of speech (was 1.5 s).
- Window drag-resizes debounce the responsive rebuild (150 ms) instead of rebuilding mid-drag.

## [0.6.6] - 2026-07-10

### Changed
- **Branding: damt.xyz** — the Windows installer's publisher is now `damt.xyz` (was the personal name), publisher URL points to https://damt.xyz, the About tab gains a Website row, README credits "Made by damt.xyz", and the macOS bundle/LaunchAgent id is `xyz.damt.eqho`. Author credit stays in About + README license.

### Fixed
- **The model-switch freeze — root cause identified from the log.** Frozen sessions were never dying: multiple Eqho instances (installed autostart copy + dev runs + killed-but-zombie relaunches) each held a Whisper model in the 6 GB GPU, and the next CUDA load hung in Windows WDDM paging instead of erroring. Fixes:
  - **Single-instance lock** — a second Eqho now shows "already running" and exits instead of double-hooking the hotkey and stacking VRAM.
  - Model switches force garbage collection so the old model's VRAM is freed *before* the new one loads.
- **Hotkey registration hardened** — register/unregister is serialized behind a lock (background settings threads could race it into a dead state → Alt+Q stopped working), registration failures now log loudly, and settings applies are single-flight.

## [0.6.5] - 2026-07-10

### Fixed
- **Stale "active model" display** — with in-place card updates, tab headers kept showing the previous model. Headers now refresh in place on model change, and hidden tabs rebuild on next show.

### Added
- **Freeze watchdog** — the dashboard beats once a second from inside its Tk loop; if the UI stalls >5 s, every thread's stack is dumped into `eqho.log`, so a freeze pinpoints its own cause.
- **`python run.py --diagnose [model]`** — headless model-switch diagnostic: timed cache checks for all models, timed load of the current model, timed switch to the target (exact in-app reload path), with automatic thread dumps if any phase exceeds 60 s. Never saves settings.
- Model-load phase timings in the log (weights, CUDA verification).
- `is_model_cached` results memoized for 5 s (the Models tab asks 9× per build; the first faster-whisper import costs ~0.9 s).

## [0.6.4] - 2026-07-10

### Fixed
- **Downloaded distil models showed "Not downloaded"** (Select disabled) — the cache check missed the `faster-distil-whisper-*` repo naming; it now asks faster-whisper itself with `local_files_only=True`.
- Settings changes apply on a background thread for every caller (tray menu included) and eagerly preload the newly selected model with tray notifications.

### Added
- Rotating file log at the config dir (`eqho.log`) + unhandled-exception hooks for main thread and all threads — crashes in the windowed exe finally leave evidence.

## [0.6.3] - 2026-07-10

Review round 3 + first public GitHub Release.

### Fixed
- **The italic headings, actually fixed.** Root cause found: systems with many installed Inter variants (variable-font instances incl. italics) make Windows' bold-matching resolve "Inter bold" to an *italic* instance. Bold text now binds directly to the "Inter SemiBold" family — no bold matching, no italics. Applies to tab titles, section labels, card names, everything.
- **Models tab flashing** — selecting a model or starting a download no longer rebuilds the whole tab; cards update in place (border, ● marker, status, buttons).
- **Silent download failures** — a failed download now shows "Download failed — check logs" on the card and brings the ↓ button back for retry; the progress poller no longer dies when one card misbehaves.
- Model switching lag reduced (no tab rebuild; the engine reload already ran on a background thread).

### Changed
- **Tighter vertical rhythm** — title→subtitle gap halved, subtitle→content reduced ~25%, section headers pulled in; the whole UI reads denser and less chunky.
- **Smoother scrolling** — smaller canvas scroll increment (the default felt jumpy, especially at higher UI zoom).
- **Default UI zoom is 125%** (was 150%). Existing installs keep their saved zoom — change it in General → Interface.
- Dropdown text nudged 1 px down (Inter metrics sit text slightly high in boxes).

### Added
- **Overlay audio-level indicator** — a short accent line along the overlay's bottom edge that eases with your live mic level; subtle, never covers the text. Toggleable: Overlay → "Audio Level Indicator".
- README: SEO keyword footer for search discoverability.

## [0.6.2] - 2026-07-10

Second review round + public launch push (dev merged to public `main` at v0.6.1).

### Fixed
- **Dark-theme logo actually shows the dark-theme artwork now.** Root cause: `logo/new logo/` sources were already mode-named, so the v0.6.1 file swap double-flipped them. Verified by pixel luminance; `tools/make_icons.py` is now a same-name copy with the convention documented.

### Changed
- **Stacked setting rows** — every label+control pair now stacks vertically (control below, left-aligned) instead of side-by-side, so controls no longer crush into labels at narrow window widths, in any column count.
- **UI is 50% bigger by default** — dashboard renders at 150% zoom, and a new **UI Zoom** setting (General → Interface: 100–200%) rescales the whole dashboard live. Responsive breakpoints scale with zoom.

### Added
- **Model downloads in the Models tab** — non-downloaded models show a disabled Select button plus a square ↓ download button; downloading cards show a live percent readout and a thin progress bar filling left→right along the card bottom (size-based estimation). Select enables automatically when the download completes.

## [0.6.1] - 2026-07-10

Polish pass from Daniel's first installed-build review.

### Changed
- **Softer chime** — lower register (G4/B4), ~10 dB quieter, full swell envelope; nothing urgent-sounding, both start and stop.
- **Theme switching is fast now** — tabs build lazily (only the visible tab rebuilds on a theme change or first open; others build when visited).
- **Segmented controls** (Hotkey Mode, Paste Mode) — rounder corners, inset trough, and a new `accent_selected` fill tuned so the text passes AA contrast in both themes.
- **Toggle switches** — the knob is now accent-blue like the track (was a near-black circle on the blue track in light mode).
- **No italics anywhere** — all UI fonts are now created with slant explicitly forced to roman.
- **Section labels lost the "▶" icon** — plain uppercase text.
- **Logo asset naming convention flipped to theme-served**: `*_dark.png` is the file shown IN dark mode (light artwork) and `*_white.png`/`*_light.png` in light mode. Files swapped in `assets/` + `logo/`, all code references updated, `tools/make_icons.py` maps the color-named sources in `logo/new logo/` accordingly.
- **README rewritten** for the public launch — tighter, approachable, SEO-friendly (offline/private/free voice-to-text keywords, install-first structure, FAQ), with the AGPL attribution requirement stated plainly.

## [0.6.0] - 2026-07-10

Cross-platform groundwork + Linux/macOS packages. Core dictation (hotkey → record → transcribe → inject) now targets all three OSes; extras degrade gracefully.

### Added
- **`src/oskit/` platform layer** — volume ducking, window focus, autostart, bundled-font loading, theme detection, and mic-ducking policy behind one interface with windows / linux / darwin implementations (pycaw+winreg+gdi32 / wpctl-pactl+XDG+fontconfig+gsettings+xdotool / osascript+LaunchAgent+AppleInterfaceStyle). Base methods are safe no-ops, so missing capabilities degrade instead of crashing.
- **pynput hotkey backend** — `hotkey_backend: auto` setting keeps Windows on the proven `keyboard` lib and gives Linux (X11) and macOS working global hotkeys in both toggle and hold modes.
- **Linux packaging** — `packaging/linux/`: onedir spec + `build.sh` producing `Eqho-linux-<v>.tar.gz` and (when appimagetool is present) an AppImage; `.desktop` entry.
- **macOS packaging** — `packaging/macos/`: `.app` bundle spec (LSUIElement menu-bar app, mic-usage description) + `build.sh` producing an unsigned `Eqho-macos-<v>.dmg`; `assets/eqho.icns` generated by `tools/make_icons.py`.
- **CI build matrix** — release workflow now also runs ubuntu (xvfb-headless smoke gate) and macos jobs, attaching per-OS artifacts to every `v*` release.

### Changed
- `requirements.txt` platform markers: `keyboard` and `pycaw` install on Windows only.
- Windows-only code paths removed from feature modules: fonts/theme/tray/injector/main/transcriber now delegate to oskit (public interfaces unchanged); the overlay's DWM call is guarded off-Windows.
- Known limits documented in GOTCHAS: Wayland hotkeys (X11 is the Linux v1 target), macOS Accessibility/Input-Monitoring permissions, hotkey-capture UI still Windows-only.

## [0.5.0] - 2026-07-10

All Phase-5 local features — no cloud, no server, everything on-device.

### Added
- **Transcript history** — every dictation is saved to `history.jsonl` in the config dir (pruned at 1000); the History tab is now functional: newest-first list, live search, per-entry copy/delete, Clear All, export to .txt. Toggle: General → Dictation → "Save History".
- **Custom vocabulary** — a General-tab textbox whose content is passed to Whisper as `initial_prompt`, biasing recognition toward your names/jargon.
- **Text replacements** — user-defined substitutions (word-boundary, case-insensitive) applied before injection; small `spoken => written` editor dialog.
- **Voice commands** (opt-in) — whole-utterance commands: "new line", "new paragraph", "period", "comma", "question mark", "colon", …; "delete that" removes the previous utterance (or backspaces the last injected text). Punctuation and newlines join smartly (no stray spaces).
- **Sound feedback** — synthesized start/stop blips via sounddevice (no assets, cross-platform); start blip plays before volume ducking so it stays audible.
- `--smoke` now also asserts textproc command/replacement/join behavior and a history read/write/prune round-trip.

## [0.4.1] - 2026-07-10

### Added
- **Windows packaging pipeline** (`packaging/windows/`) — onedir PyInstaller spec (version parsed from `src/version.py`), `build.ps1` producing `dist\Eqho\`, `Eqho-portable-<version>.zip`, and (when Inno Setup is installed) `Eqho-Setup-<version>.exe`.
- **Inno Setup installer** — per-user (no UAC), Start Menu shortcut, optional desktop icon, "start when you sign in" checkbox that writes the same Run-key value as the in-app toggle, launch-after-install, clean uninstall that preserves settings and models.
- **GitHub Actions release workflow** — every `v*` tag builds the portable zip + installer on `windows-latest` (smoke gate first) and attaches them to the GitHub Release; `workflow_dispatch` for dry runs.

### Changed
- Root `Eqho.spec` and `build.ps1` retired in favor of `packaging/windows/`; COMMANDS.md updated.
- Verified: the packaged `Eqho.exe --smoke` passes the full headless gate (settings, 17 audio devices, model load, silence transcription).

## [0.4.0] - 2026-07-10

### Changed
- **UI overhaul (Toolcraft-inspired, original implementation)** — near-black dark theme (`#0a0a0b` canvas, `#151517` cards), hairline 1px borders on cards/inputs/dropdowns, compact radii (4/6/8/12), denser Inter type scale. Light theme keeps Eqho's light palette with the same new geometry. Accent remains Eqho blue `#58a6ff`.
- **`dashboard.py` split into `src/ui/` package** — widgets / layout / context / tabs / orchestrator (~1700-line file retired); tabs own their state and talk via a small pub-sub (`DashboardContext`); dead `settings_ui.py` removed.
- **Theme is the single styling source** — new `theme.font()` helper; zero inline font tuples or hex literals in `src/ui/`; primary/secondary/ghost button recipes.

### Fixed (the five deferred 0.3.1 issues)
- **Dropdown polish** — closed state has a hairline border and a right-pinned chevron; popup supports Up/Down/Enter/Escape and scrolls the selection into view.
- **Title-bar icon** — now set 260 ms after creation (outlasting CTkToplevel's own deferred icon call that kept overwriting it) and theme-matched (white mark on dark, blue on light).
- **Tray icon theme detection** — the taskbar theme is now re-polled every 15 s, so switching Windows theme updates the tray icon without toggling recording.
- **Responsive rebuild glitch** — each tab records the column count it was built with and rebuilds on show if stale (no more resize + re-click).
- **Column spacing** — unified padding law (card inset = 12 px everywhere, single-column and grid alike).

### Added
- **Dark title bar** via DWM (attr 20/19) matching the app theme, reapplied on theme switch.
- **Overlay animations** — pulsing accent recording dot, alpha fade in/out.
- **New logo assets adopted** — `logo/new logo/` PNGs copied into `assets/` + `logo/`, `eqho.ico` regenerated (`tools/make_icons.py`).

## [0.3.3] - 2026-07-09

### Added
- **`python run.py --smoke`** — headless self-check (settings → audio devices → tiny-model load → silence transcription) printing a JSON report; the standard verification gate for future milestones and CI.
- **Hallucination gating** — near-silent buffers are skipped (peak RMS < 1.5× VAD threshold), segments with `no_speech_prob > 0.6` and `avg_logprob < -1.0` are dropped, and short utterances matching known Whisper artifacts ("Thank you.", "Thanks for watching.", …) are discarded.
- **CPU warm-up** — a dummy inference after CPU model load removes first-phrase lag (CUDA already had a smoke test).
- **`model_dir` setting** — model cache location is configurable; resolves to the legacy `D:\EqhoModels` when present (auto-pinned on first load, nothing re-downloads) or the platform cache dir on fresh installs.

### Changed
- **Config paths via `platformdirs`** (new dependency) — same `%APPDATA%\Eqho` on Windows, correct native dirs on Linux/macOS (groundwork for Phase 6).
- **Model loading moved off the hotkey thread** — first activation no longer freezes hotkey handling; the overlay shows "Loading model…" and audio spoken during the load is queued and transcribed once ready. Also serialized behind a dedicated lock (fixes the preload-vs-hotkey double-load race).
- **Audio buffering is O(n)** — chunk list instead of repeated `np.concatenate`; live partials re-transcribe at most the last 10 s (finals still use the full buffer).
- **Default mic is now the system default** (`audio_device: null`) — the old hardcoded device 3 stays only where it's already persisted in settings.
- **Toggle-mode state authority moved to the app** — the hotkey layer only debounces, so a failed mic start no longer desyncs the toggle.
- Clipboard paste falls back to simulated typing when the clipboard is unavailable; a non-text clipboard is no longer clobbered with an empty string.
- Mic errors surface via `consume_mic_error()` instead of private attribute pokes; `_target_hwnd`/`_saved_volume`/pending text are consistently lock-guarded.

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
