# START HERE — Eqho

Always-on voice-to-text dictation app. System tray, **Alt+Q**, speak, text lands in the focused app. 100% local: faster-whisper (CTranslate2) + CUDA. Windows primary; Linux X11 + macOS core dictation since v0.6.0.

- **Version:** `src/version.py` only. Current: 0.6.9, publicly released (tag `v0.6.9` → CI built installers for all 3 OSes).
- **Repos:** `origin` = github.com/danielmevit/eqho (PUBLIC, `main` = releases) · `private` = danielmevit/Eqho-private (`dev` = all work). Merge dev→main ONLY on Daniel's explicit release request. ⚠ Daniel sometimes edits `main` via the GitHub web UI — if a push bounces, `git pull origin main`, merge, NEVER force-push, then merge main back into dev.
- **Landing page:** `site/` (Astro) → GitHub Pages at danielmevit.github.io/eqho via `.github/workflows/deploy.yml` (builds `./site` on every main push). Exists for SEO + AI-scraper reach (`site/public/llms.txt`).

## State (2026-07-11)

Everything through ROADMAP Phase 6 is DONE and released. Recent deep fixes a next agent must NOT regress:
- **tkinter cross-interpreter deadlock** (v0.6.7) — see GOTCHAS "tkinter", the most important trap in this codebase.
- **Adaptive VAD** (v0.6.8) — speech gate = clamp(noise_floor×3.5, 0.0009, 0.003); never reintroduce a fixed threshold.
- **Single-instance lock** (v0.6.6) — localhost port 48317.

**Awaiting Daniel's confirmation:** model-switch freeze gone, dictation transcribes at his mic level, chime reliable. If the UI ever stalls >5 s, `%APPDATA%\Eqho\eqho.log` gets a `=== THREAD DUMP` — read it before theorizing.

**Next work, in order:** 1) fixes from Daniel's testing; 2) winget manifest PR (Releases now has an installer URL); 3) CONTRIBUTING.md + demo GIF (Phase 7 finishers); 4) backlog: silero-VAD, streaming partials, per-app paste rules, VAD sensitivity slider, more overlay customization.

## Run / verify

```bash
# from WSL (the usual agent environment); the cd path stays UNQUOTED
cmd.exe /c 'cd /d D:\Vibe Coding\Eqho && venv\Scripts\python.exe run.py'                             # run the app
cmd.exe /c 'cd /d D:\Vibe Coding\Eqho && venv\Scripts\python.exe run.py --smoke'                      # headless gate — MUST pass before every commit
cmd.exe /c 'cd /d D:\Vibe Coding\Eqho && venv\Scripts\python.exe run.py --diagnose distil-large-v3'   # timed model-switch replay
```

Per change: implement → `--smoke` → `CHANGELOG.md` entry → conventional commit on dev → push (`cmd.exe /c 'cd /d D:\Vibe Coding\Eqho && git push'`) → `codegraph sync`.

## Navigate

- Code structure, callers, call paths → CodeGraph (`codegraph explore "..."`); `codegraph sync` after edits (WSL /mnt has no file-watching).
- UI tokens & layout laws → `DESIGN_SYSTEM.md` · Why-choices → `DECISIONS.md` · **Traps → `GOTCHAS.md` (read it)**
- Plans → `ROADMAP.md` · Shipped → `CHANGELOG.md` · Daniel's manual steps → `TODO.md`
