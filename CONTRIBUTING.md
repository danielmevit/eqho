# Contributing to Eqho

Thanks for your interest! Eqho is a small, focused project — contributions that keep it fast, private, and local are very welcome.

## Ground rules

- **Everything stays local.** No telemetry, no cloud calls, no accounts. PRs adding network dependencies (beyond the one-time model download) won't be accepted.
- **License:** AGPL-3.0. By contributing you agree your changes are licensed the same way. Keep copyright/attribution notices intact.
- **One feature or fix per PR**, with a clear description of the user-visible behavior.

## Dev setup

```bash
git clone https://github.com/danielmevit/eqho.git
cd eqho
python -m venv venv && venv\Scripts\activate   # Windows (see README for Linux/macOS deps)
pip install -r requirements.txt
python run.py
```

## Before you open a PR

1. **Run the smoke gate — it must pass:** `python run.py --smoke` (headless JSON self-check: settings, audio, model load, transcription, text processing, history).
2. **Read the two files that will save you pain:** `docs/ai/GOTCHAS.md` (real traps — threading, tkinter, CUDA, packaging) and `docs/ai/DECISIONS.md` (choices we won't relitigate). UI work: also `docs/ai/DESIGN_SYSTEM.md` — all colors/fonts/spacing come from `src/theme.py`, no inline hex or font tuples.
3. **Match the architecture:** OS-specific code goes ONLY in `src/oskit/`; tk Variables are created via the `TabBase._string_var/_bool_var` helpers (never bare — see GOTCHAS on the default-root deadlock); the version lives in `src/version.py` only.
4. **Commit style:** conventional commits (`type(scope): description`), and add a `CHANGELOG.md` entry for anything user-visible.

## Reporting bugs

Open an issue with: your OS + GPU, the model in use, steps to reproduce, and the tail of `%APPDATA%\Eqho\eqho.log` (on UI freezes it contains a `=== THREAD DUMP` that usually pinpoints the cause).

## Good first areas

Check `ROADMAP.md` → Stretch Goals / Backlog: silero-VAD, streaming partials, per-app paste rules, Wayland support, overlay customization, translations of the README.
