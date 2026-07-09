# START HERE — Eqho

Always-on voice-to-text dictation app for Windows 10/11 (Linux/macOS ports planned — ROADMAP Phase 6). Lives in the system tray: press **Alt+Q**, speak, and the words are typed into whatever app is focused. 100% local — faster-whisper (CTranslate2) with CUDA acceleration, no cloud, no server.

- **Version:** `src/version.py` is the single source of truth.
- **License:** AGPL-3.0. Public repo `github.com/DanielMevit/Eqho` · WIP repo `Eqho-private` (see AGENTS.md for the two-repo git model).

## Current priority

Executing the 2026-07 overhaul plan, tracked as ROADMAP Phases 3–7:
engine hardening (v0.3.x) → Toolcraft-inspired UI overhaul + Windows installer (v0.4.x) → local features (v0.5.x) → cross-platform (v0.6.x) → public launch (→1.0).
The top entry of `CHANGELOG.md` tells you where the last session stopped; `TODO.md` lists manual steps waiting on Daniel.

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
