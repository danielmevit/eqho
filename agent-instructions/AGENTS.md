# Codex rules for this repo (Eqho)

## Reading ritual
- At session start, read SOUL.md, then AGENTS.md, then README.md silently and obey them.
- Do not summarize them unless Daniel explicitly asks.

## SOUL handling
- Treat SOUL.md as identity/standards. Update it only when those evolve (not as a task log).

## Workflow
- Don't ask for permission or micromanage. Plan, implement, run, summarize in one go.
- Work in milestone-sized steps: Plan -> implement -> run -> summarize.
- One task should deliver a visible improvement and include verification (run/test) and a CHANGELOG.md entry.
- Prefer minimal dependencies; justify each new one.
- Always verify changes work after making them (`python run.py` or import check at minimum).
- Do not modify anything outside this repository.
- Do not rename or change the folder structure. The project source lives in `src/`; that folder stays named `src`.

## Documentation
- CHANGELOG.md is the timestamped release log. Append every meaningful change with a `YYYY-MM-DD` header.
- README.md is the project overview only. Update feature lists and structure section when new modules are added. Do not add progress logs or session notes to README.
- ROADMAP.md tracks planned features and milestones. Update when priorities shift.
- TODO.md tracks manual steps Daniel needs to do. Keep it short and actionable.
- Keep entries concise. No long logs, no file lists.

## Architecture
- Python 3.10+, runs on Windows 10/11.
- Entry point: `run.py` (calls `src.main.main()`).
- Transcription engine: faster-whisper (CTranslate2 backend, MIT license). Will migrate to whisper.cpp in Phase 4 for native distribution.
- Default model: `distil-large-v3` (English-optimized, ~6x faster than large-v3). Multilingual models also available.
- GPU: CUDA Toolkit 12.x required for GPU inference (NVIDIA). Auto-falls back to CPU if cuBLAS DLLs are missing.
- Models cached to `D:\EqhoModels` (not in the repo). First run downloads from HuggingFace.
- Each responsibility lives in its own module inside `src/`:
  - `settings.py` -- config persistence (`%AppData%\Eqho\settings.json`)
  - `transcriber.py` -- faster-whisper wrapper with energy-based VAD, CUDA fallback to CPU
  - `audio.py` -- device enumeration (sounddevice)
  - `overlay.py` -- floating transcription preview (tkinter, bottom-center)
  - `hotkey.py` -- global hotkey (keyboard library, toggle + hold modes)
  - `injector.py` -- text injection into active app (pynput + pyperclip + ctypes Win32 for window focus restore)
  - `tray.py` -- system tray icon and menu (pystray + Pillow), loads logo-based PNGs from `assets/`
  - `settings_ui.py` -- legacy hotkey capture window (kept for reference, replaced by dashboard)
  - `dashboard.py` -- main settings dashboard (customtkinter, sidebar nav, all settings)
  - `theme.py` -- design tokens, color palettes, dark/light/system theme system
  - `main.py` -- wires everything together, manages activate/deactivate lifecycle
- Settings persist in `%AppData%\Eqho\settings.json`.

## Known quirks
- Whisper's built-in `vad_filter` is too aggressive and discards valid speech. We use our own energy-based VAD instead (silence threshold 0.003 RMS, 1.2s silence timeout).
- The CUDA smoke test in `_ensure_model()` catches missing cuBLAS at startup rather than at first transcription.
- `keyboard` library requires the terminal to stay open (no console-free mode without PyInstaller packaging).
- `keyboard.unhook_all()` must never be used -- it kills all hooks globally (including pystray internals). Use targeted `unhook()` / `remove_hotkey()` per hook.
- `large-v3` is forced to CPU (too large for 6GB VRAM). All other models (including distil and turbo) run on GPU.

## Future architecture (Phase 4)
- The commercial/public release will use **whisper.cpp** (pure C++) instead of faster-whisper (Python + CTranslate2).
- App shell will likely be Tauri (Rust) or C#/WPF instead of tkinter + pystray.
- This eliminates the Python runtime dependency and makes packaging a single lightweight installer.
- The current Python stack is for development velocity; whisper.cpp is for shipping.

## Versioning
- Current version line: **v0.3.1** -- Dashboard polish, custom dropdowns, public release prep.
- Tag releases as `vMAJOR.MINOR.PATCH` when publishing milestones.

## Repository setup
- **Two GitHub repos, one local folder:**
  - `origin` → `github.com/DanielMevit/Eqho` — **PUBLIC** repo. Only gets stable, polished updates.
  - `private` → `github.com/DanielMevit/Eqho-private` — **Private** repo. Gets all work-in-progress commits.
- **Two branches:**
  - `dev` — daily working branch. Default push goes to `private/dev`.
  - `main` — public-facing branch. Only merge from `dev` when ready to release.
- Agent instruction files live in `agent-instructions/` (not in repo root).

## Commit & push
- Commit message format: `type(scope): short description` (conventional commits).
- Daily work: commit to `dev`, push to `private` repo (`git push`).
- Public release: `git checkout main` → `git merge dev` → `git push origin main` → `git checkout dev`. Only do this when Daniel explicitly says to publish.
