# Agent rules — Eqho

## Reading ritual (start of every session)
1. `SOUL.md` — identity & standards
2. `AGENTS.md` — this file
3. `docs/ai/START_HERE.md` — orientation; follow its links as the task needs

## Navigation — find code without crawling files
- Structure / "where is X?", callers, call paths → CodeGraph (`codegraph_explore` MCP tool or `codegraph explore "..."` in the shell). Trust it; don't grep-loop.
- Intent / why / gotchas → the right file in `docs/ai/` (map below).
- Do NOT hand-maintain a file map in Markdown — CodeGraph owns structure.
- This repo is usually edited from WSL on `/mnt/d` — file-watching doesn't work there, so run `codegraph sync` after editing code (see GOTCHAS).

## docs/ai/ map
| File | Owns |
|------|------|
| START_HERE.md | Orientation, current priority, how to run |
| DESIGN_SYSTEM.md | Design tokens, layout laws, theming |
| DECISIONS.md | Durable "why" choices |
| GOTCHAS.md | Build/run/env traps, known bugs |

## Workflow
- Don't ask for permission or micromanage. Plan → implement → verify → summarize in one go, in milestone-sized steps.
- Each milestone delivers a visible improvement and includes verification (run the app or `--smoke`) + a `CHANGELOG.md` entry.
- Prefer minimal dependencies; justify each new one.
- Do not modify anything outside this repository.

## Commit & push
- Conventional commits: `type(scope): short description`.
- Work on `dev`; merge to `main` ONLY on Daniel's explicit release request.
- Two remotes: `origin` → `github.com/danielmevit/eqho` (**PUBLIC**, stable only) · `private` → `github.com/danielmevit/Eqho-private` (all WIP). Plain `git push` on `dev` goes to `private`.
- Pushing from WSL needs Windows-side credentials: `cmd.exe /c "cd /d D:\Vibe Coding\Eqho && git push"`.

## Documentation upkeep
- `CHANGELOG.md` = what shipped, dated (the session log; don't make a second one).
- Update `DECISIONS.md` the moment a durable choice is made; `DESIGN_SYSTEM.md` on token/layout changes.
- Keep START_HERE's "current priority" in sync with `ROADMAP.md` / `TODO.md`.
- The version lives in `src/version.py` ONLY — the About tab and build scripts read it; never hardcode it elsewhere.
