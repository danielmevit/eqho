# TODO — Manual Steps

Things Daniel needs to do that can't be automated by the agent.

## Open
- [ ] Record a short demo GIF (hotkey → speak → text appears) for README + landing page. Tools: ScreenToGif or Xbox Game Bar.
- [ ] Final QA on a clean Windows machine/VM: install Eqho-Setup, dictate, uninstall.
- [ ] **winget PR #400796 — SIGN THE CLA (only you can):** comment `@microsoft-github-policy-service agree` on https://github.com/microsoft/winget-pkgs/pull/400796 . Validation already PASSED; this is the only blocker, then a moderator merges (days). Do NOT open a new PR.
- [ ] Autostart: use the "Start with Windows" toggle in the tray/dashboard (the v0.4.1 installer will also offer it as a checkbox).
- [ ] Public launch (ROADMAP Phase 7, do last): public README with screenshots + demo GIF; publish a GitHub Release with the installer; flip the repo public.

## Done (reference)
- [x] venv + `requirements.txt` deps + CUDA Toolkit 12.9 installed; dictation verified end-to-end (Alt+Q).
- [x] Git: two-repo setup (`origin` public, `private` WIP) with dev/main branches.
- [x] License chosen: AGPL-3.0.
- [x] Internal docs relocated — now root `AGENTS.md`/`SOUL.md`/`TODO.md` + `docs/ai/` (was `agent-instructions/`).
