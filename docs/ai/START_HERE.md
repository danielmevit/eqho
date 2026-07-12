# START HERE — Eqho

Always-on voice-to-text dictation app. System tray, **Alt+Q**, speak, text lands in the focused app. 100% local: faster-whisper (CTranslate2) + CUDA. Windows primary; Linux X11 + macOS core dictation since v0.6.0.

- **Version:** `src/version.py` only. Current **public** release: **0.8.2** (tag `v0.8.2`, 2026-07-12 → CI built + published installers for all 3 OSes; GitHub Release live). `dev` and `main` synced at 0.8.2.
- **Repos:** `origin` = github.com/danielmevit/eqho (PUBLIC, `main` = releases) · `private` = danielmevit/Eqho-private (`dev` = all work). Merge dev→main ONLY on Daniel's explicit release request. ⚠ Daniel sometimes edits `main` via the GitHub web UI — if a push bounces, `git pull origin main`, merge, NEVER force-push, then merge main back into dev.
- **Landing page:** `site/` (Astro) → GitHub Pages at danielmevit.github.io/eqho via `.github/workflows/deploy.yml` (builds `./site` on every main push). Exists for SEO + AI-scraper reach (`site/public/llms.txt`).

## State (2026-07-12)

Everything through ROADMAP **Phase 7 (public launch)** is DONE — AGPL, public README, landing page, CONTRIBUTING, repo public. **Released 0.8.2 on 2026-07-12** (first public release since 0.6.9), shipping the **v0.7.x–0.8.x engine arc**:
- **Auto-format** Tier-1 text cleanup (v0.7.0) · **mic-sensitivity slider** + **per-app paste rules** (v0.7.4).
- **Seamless model switching** — a subprocess **model host** (v0.8.0); inference runs in a CHILD process, the main app never loads a model (GOTCHAS "CUDA / models").
- **Dual engine + cross-vendor GPU** (v0.8.1–0.8.2) — a **whisper.cpp** backend (`pywhispercpp`/Vulkan) beside faster-whisper; `engine_backend="auto"` = faster-whisper on NVIDIA+CUDA, whisper.cpp (AMD/Intel/CPU) otherwise; UI **Inference Engine picker** (General → Model) switches at runtime. ✅ **Runtime-verified on hardware 2026-07-12:** Vulkan enumerated NVIDIA RTX 3060 **+ AMD Radeon** and transcribed — the AMD-support goal is proven.

Deep fixes a next agent must NOT regress:
- **Subprocess model host** (v0.8.0) — never load a Whisper model in the main process.
- **tkinter cross-interpreter deadlock** (v0.6.7) — GOTCHAS "tkinter", the most important trap here.
- **Adaptive VAD** (v0.6.8) — gate = clamp(noise_floor×3.5, 0.0009, 0.003); never a fixed threshold.
- **Single-instance lock** (v0.6.6) — localhost port 48317.

If the UI ever stalls >5 s, `%APPDATA%\Eqho\eqho.log` gets a `=== THREAD DUMP` — read it before theorizing.

**Next work:** 0.8.2 is shipped — installers + landing page are live. Follow-ups: 1) **winget** — `damt.Eqho` manifests are bumped to 0.8.2 (`packaging/winget/`, real exe SHA256); submit/update the microsoft/winget-pkgs PR (note #400796 was the 0.6.9 submission, still pending a moderator); 2) demo GIF (TODO.md, Daniel); 3) backlog: silero-VAD, streaming partials, more overlay customization, Eqho Mobile shared engine. Minor known issue: the start chime can be swallowed over a Bluetooth Hands-Free (HFP) mic.

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
