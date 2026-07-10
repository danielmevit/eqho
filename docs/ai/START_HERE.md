# START HERE — Eqho

Always-on voice-to-text dictation app for Windows 10/11 (Linux/macOS ports planned — ROADMAP Phase 6). Lives in the system tray: press **Alt+Q**, speak, and the words are typed into whatever app is focused. 100% local — faster-whisper (CTranslate2) with CUDA acceleration, no cloud, no server.

- **Version:** `src/version.py` is the single source of truth.
- **License:** AGPL-3.0. Public repo `github.com/DanielMevit/Eqho` · WIP repo `Eqho-private` (see AGENTS.md for the two-repo git model).

## Current priority

The 2026-07 overhaul (ROADMAP Phases 3–6) is COMPLETE at v0.6.0: engine hardening, Toolcraft-inspired UI, Windows installer + CI, local features (history/vocab/commands/chime), and cross-platform core with Linux/macOS packages.

Next up: **Daniel's eyeball pass on the new UI + install test**, then **Phase 7 — public launch** (public README with screenshots, first tagged release through the CI pipeline, winget manifest, flip repo public). Tag `v0.6.0` on dev to dry-run the release workflow. The top entry of `CHANGELOG.md` tells you where the last session stopped; `TODO.md` lists manual steps waiting on Daniel.

## Run it

From PowerShell in `D:\Vibe Coding\Eqho`:

```powershell
venv\Scripts\activate
python run.py
```

From WSL (the usual agent environment):

```bash
cmd.exe /c "cd /d D:\Vibe Coding\Eqho && venv\Scripts\python.exe run.py"
```

First run downloads the default model (distil-large-v3, ~1.5 GB). Full command reference: `COMMANDS.md`.

## Navigate

- Code structure, callers, call paths → CodeGraph (`codegraph explore "..."`); run `codegraph sync` after edits from WSL.
- Design tokens & layout laws → `DESIGN_SYSTEM.md`
- Durable why-choices → `DECISIONS.md` · Operational traps → `GOTCHAS.md`
- Plans → `ROADMAP.md` · Shipped → `CHANGELOG.md` · Manual steps → `TODO.md`
