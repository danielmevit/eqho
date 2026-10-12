# Eqho state audit — 2026-10-12

Worker: AUDIT-1 (vibeglm). Read/run/report only; nothing was fixed. All commands ran from
this worktree (`agent/vibeglm`, synced with `origin/dev` @ 42b939c) unless noted.

## 1. Gate results

- `"/mnt/d/Vibe Coding/Eqho/venv/Scripts/python.exe" run.py --smoke` (from this worktree):
  `{"ok": true, steps: {settings: ok, audio_devices: 17, textproc: ok, history: ok,
  model_load: ok, transcribe_silence: ok}, version: "0.8.8", model_dir: "D:\\EqhoModels",
  silence_text: ""}` — PASS.
- `python3 -m compileall -q src run.py` — rc=0, no output — PASS.
- Owner's venv used read-only; nothing installed or modified.

## 2. Version consistency (single source: `src/version.py` = 0.8.8)

| Location | Value | Matches 0.8.8? |
|---|---|---|
| `src/version.py:6` | `__version__ = "0.8.8"` | yes (source of truth) |
| `CHANGELOG.md:9,20` | latest heading `[0.8.8] - 2026-07-14` | yes |
| `site/src/pages/index.astro:52` | `"softwareVersion": "0.8.8"` | yes |
| `site/src/pages/changelog.astro:8` | top entry `v: "0.8.8", d: "2026-07-14"` | yes |
| Live Pages site (curl danielmevit.github.io/eqho/) | `softwareVersion": "0.8.8"` | yes |
| `packaging/winget/damt.Eqho.installer.yaml:3` | `PackageVersion: 0.8.2` | NO — stale |
| `packaging/winget/damt.Eqho.locale.en-US.yaml:3` | `PackageVersion: 0.8.2` | NO — stale |
| `packaging/winget/damt.Eqho.yaml:3` | `PackageVersion: 0.8.2` | NO — stale |
| `packaging/windows/installer.iss:8-9` | `#define AppVersion "0.0.0"` fallback; real value passed by `build.ps1` from `src/version.py` | n/a (correct design) |
| `README.md` | no version literal (links to Releases) | n/a |

Note: `packaging/winget/damt.Eqho.installer.yaml:13` points at
`.../v0.8.2/Eqho-Setup-0.8.2.exe` — the OLD artifact name. Since 0.8.7 the Windows
installer is `Eqho-Setup-<v>-win-x64.exe` (CHANGELOG 0.8.7; v0.8.8 release assets confirm:
`Eqho-Setup-0.8.8-win-x64.exe`). The next winget bump must change the URL pattern too.

## 3. Docs drift

- `docs/ai/START_HERE.md:5` — "Current public release: **0.8.2** ... `dev` and `main`
  synced at 0.8.2." False: latest release is v0.8.8 (2026-07-14, `gh release list`), and
  main/dev are 1 commit apart each way (see section 4).
- `docs/ai/START_HERE.md:11` — "State (2026-07-12) ... Released 0.8.2 on 2026-07-12" —
  stale; three releases (0.8.6, 0.8.8) shipped after.
- `docs/ai/START_HERE.md:14` — "UI Inference Engine picker (General → Model)". Stale:
  0.8.8 moved the picker to the Models section (CHANGELOG.md:23, "Models gained the
  Inference Engine picker (moved from General)").
- `docs/ai/START_HERE.md:24` — "note #400796 was the 0.6.9 submission, still pending a
  moderator". False: `gh pr view 400796 -R microsoft/winget-pkgs` → state MERGED,
  mergedAt 2026-07-28T21:41:14Z.
- `TODO.md:75` — "winget PR #400796 — waiting on Microsoft moderator ... Nothing to do
  but wait." Stale: merged 2026-07-28; `winget install eqho` should now be live at 0.6.9.
- `ROADMAP.md` Phase 7 — "winget manifest submitted (microsoft/winget-pkgs PR #400796)"
  is now merged (could be checked off); Backlog "AMD/Intel GPU support — ... needs Vulkan
  build + packaging + AMD-hardware verification" is stale: TODO.md marks all four steps
  DONE 2026-07-12 and 0.8.2 shipped it.
- `ROADMAP.md` Phase 7 heading "(→ v1.0)" conflicts with the AGENTS.md:39 rule that 1.0
  is reserved for Daniel's explicit out-of-beta call (cosmetic, but confusing).
- `CHANGELOG.md:7` — `[Unreleased]` section is empty (no pending entries; nothing
  unlogged).
- `AGENTS.md` — no drift found; versioning rules and repo layout match reality.
- `docs/ai/GOTCHAS.md` — current; matches code spot-checks (port 48317 at
  `src/main.py:427`, subprocess host at `src/model_host.py`).

## 4. Distribution state

- **Releases** (`gh release list -R danielmevit/eqho`): v0.8.8 (Latest, 2026-07-14),
  v0.8.6 (2026-07-14), v0.8.2 (2026-07-12), v0.6.9 (2026-07-11), v0.6.3 (2026-07-10).
  v0.8.8 assets: `Eqho-Setup-0.8.8-win-x64.exe`, `Eqho-portable-0.8.8-win-x64.zip`,
  `Eqho-linux-0.8.8.tar.gz`, `Eqho-0.8.8-x86_64.AppImage`, `Eqho-macos-0.8.8.dmg`.
- **CI** (`gh run list -R danielmevit/eqho -L 15`): all 15 latest runs completed/success.
  Last run: Pages deploy 2026-07-16 ("Update index.astro", 37s). Last Release run: v0.8.8
  tag 2026-07-14, success. No runs since 2026-07-16.
- **Pages**: live and serving version 0.8.8 (curl of the live site shows
  `softwareVersion": "0.8.8"`); last deploy succeeded 2026-07-16.
- **winget**: microsoft/winget-pkgs `manifests/d/damt/Eqho` contains ONLY `0.6.9`
  (`gh api .../contents/manifests/d/damt/Eqho`). PR #400796 (0.6.9) MERGED 2026-07-28.
  The repo's `packaging/winget/` holds 0.8.2 manifests that were never submitted — winget
  users get 0.6.9, 4 releases behind.
- **Branch divergence** (refs in this worktree): `origin/main` has exactly 1 commit not
  in `origin/dev`: e81c4af "Update index.astro" (2026-07-16, adds `<link rel="canonical">`
  to the landing page — Daniel's web-UI edit). `origin/dev` has 1 commit not in main:
  42b939c ".gitattributes" (2026-07-15). Last commit on either branch: 2026-07-16 —
  confirms "no commits since 2026-07-15" (dev) / 07-16 (main).

## 5. Dependencies

`requirements.txt` pins only floors: `faster-whisper>=1.2.1`, `customtkinter>=5.2.0`;
the rest unpinned (`sounddevice, numpy, platformdirs, pystray, Pillow, pynput, pyperclip`);
Windows markers for `keyboard`, `pycaw`; `pywhispercpp` optional (comment only).
No lockfile; CI builds install latest-at-build-time (reproducibility risk).

Installed in the owner's venv (`pip list --format=freeze`, read-only) and OSV-checked
(`api.osv.dev/v1/query`, PyPI ecosystem, exact installed version):

| Package | Installed | OSV advisories |
|---|---|---|
| faster-whisper | 1.2.1 | 0 |
| ctranslate2 | 4.7.1 | 0 |
| numpy | 2.5.1 | 0 |
| Pillow | 12.1.1 | **36** (18 GHSA: 11 HIGH, 7 MODERATE; rest PYSEC duplicates) |
| customtkinter | 5.2.2 | 0 |
| pynput | 1.8.1 | 0 |
| pyperclip | 1.11.0 | 0 |
| pystray | 0.19.5 | 0 |
| sounddevice | 0.5.5 | 0 |
| platformdirs | 4.10.0 | 0 |
| keyboard | 0.13.5 | 0 |
| pycaw | 20251023 | 0 |
| requests | 2.34.2 | 0 |
| huggingface-hub | 1.8.0 | 0 |
| onnxruntime | 1.24.4 | 0 |
| tokenizers | 0.22.2 | 0 |
| tqdm | 4.68.4 | 0 |

Pillow 12.1.1: all advisories are malformed-image attack surface (decompression bombs,
OOB writes in PSD/TGA/FITS/JPEG2000/PDF parsers, font-file issues). Eqho only decodes
its own bundled assets (logo, fonts, pill renders via `pillfx.py`), so practical exposure
is low — but it is the only dependency with open advisories and worth a bump.

## 6. Open issues / PRs

`gh issue list -R danielmevit/eqho --state all` and `gh pr list ... --state all` both
returned empty — zero issues and zero PRs (open or closed) on the public repo.

## 7. Code health quick scan

- `grep -rn "TODO|FIXME|HACK|XXX" src/ run.py` — **zero markers**.
- No bare `except:`; 115 broad `except Exception` (consistent defensive style, several
  intentional per GOTCHAS).
- No `shell=True`, no `eval`/`exec`, no pickle; subprocess calls use list argv
  (`src/oskit/darwin.py:23`, `src/oskit/linux.py:22`, `src/ui/tabs/models.py:134-136`).

Concrete defects / risks observed:

1. **winget manifest URL pattern is dead** — `packaging/winget/damt.Eqho.installer.yaml:13`
   uses `Eqho-Setup-0.8.2.exe`; artifacts are now `Eqho-Setup-<v>-win-x64.exe`
   (v0.8.8 assets). A version bump alone would 404. Confidence: high. Risk: medium.
2. **Docs actively mislead the next agent** — START_HERE.md:5/24 and TODO.md:75 state
   0.8.2 is latest and PR #400796 is pending; both false (see section 3). Confidence:
   high. Risk: high (agents plan off START_HERE).
3. **main/dev divergence unresolved** — e81c4af (canonical-URL landing-page fix) exists
   only on main; 42b939c (.gitattributes) only on dev. GOTCHAS says merge main back into
   dev after web-UI edits; not done since 2026-07-16. Confidence: high. Risk: low-medium
   (next dev→main merge will carry it, but dev's `site/` lacks the canonical link until then).
4. **Single-instance lock leaks a socket on the failure path** — `src/main.py:419-431`:
   if `bind()` raises, `s` is never closed before returning False (process exits anyway,
   so impact is negligible). Confidence: high. Risk: low.
5. **Port 48317 has no ownership check** — any unrelated app binding 127.0.0.1:48317
   makes Eqho refuse to start with no diagnostic (`src/main.py:427`). Confidence: high.
   Risk: low.
6. **Unpinned requirements** — no lockfile; CI release builds resolve deps at build time,
   so a future upstream breakage can fail a release non-reproducibly. Confidence: high.
   Risk: medium.
7. **Pillow 12.1.1 advisories** — see section 5. Confidence: high (OSV). Risk: low in
   practice (no untrusted image input path found).
8. **`os._exit(0)` on restart drops buffered log lines** — `src/main.py:371`; intentional
   (GOTCHAS: full VRAM release) but the "Restarting to apply model change..." line may be
   lost. Confidence: medium. Risk: low.
9. **History cap is silent** — `src/history.py:20` `MAX_ENTRIES = 1000`, pruned on write
   (`_prune_locked`); no user-visible warning or export-before-drop. Confidence: high.
   Risk: low (data loss is bounded and by design).
10. **START_HERE run commands point at the main checkout** — `docs/ai/START_HERE.md`
    hardcodes `cd /d D:\Vibe Coding\Eqho` (the owner's install, not a worktree); running
    them while the installed exe autostarts violates the two-instance gotcha. Confidence:
    medium. Risk: low.

## 8. Recommended next tasks

Agent can do:
1. **Docs truth pass** (S) — update START_HERE.md (0.8.8, merged winget PR, engine-picker
   location), TODO.md:75, ROADMAP Phase 7/backlog ticks. Why now: every future session
   starts from these. Files: docs/ai/STARTHERE.md, TODO.md, ROADMAP.md. Risk: none.
2. **winget manifest bump to 0.8.8 + URL-pattern fix** (S) — packaging/winget/*.yaml to
   0.8.8 with `Eqho-Setup-0.8.8-win-x64.exe` + real SHA256; owner submits the PR.
   Why now: winget users are 4 releases behind on 0.6.9. Files: packaging/winget/.
   Risk: low (submission is the owner's step).
3. **Merge main back into dev** (S) — bring e81c4af (canonical link) into dev per
   GOTCHAS. Why now: closes the only divergence. Files: site/src/pages/index.astro.
   Risk: low (clean merge expected).
4. **Dependency floor/lock hygiene** (M) — pin or lock build deps (e.g. a
   requirements-lock.txt used by CI), and bump Pillow. Why now: only dep with open
   advisories; releases must be reproducible. Files: requirements.txt,
   .github/workflows/release.yml. Risk: medium (needs a full smoke + build).
5. **Landing-page demo GIF** prep (M) — scaffold the asset slot + README/site embed;
   Daniel records the GIF (TODO.md). Why now: oldest open Phase 7 item. Files: README.md,
   site/. Risk: low.

Owner manual steps:
6. **Submit the winget 0.8.8 update PR** to microsoft/winget-pkgs (S) — after task 2;
   needs Daniel's GitHub account. Risk: none.
7. **Final QA pass on a clean Windows install** (M) — open Phase 7 checkbox; verify
   installer, autostart, hotkey, single-instance on a clean VM. Risk: none.
8. **Decide 0.9 scope** (S) — DECISIONS.md reserves 0.9.0 for a headline release; nothing
   is currently queued in [Unreleased]. Picking the 0.9 theme unblocks planning.
   Risk: none.

---
SUMMARY: Eqho is healthy at 0.8.8 — smoke and syntax gates pass, releases/CI/Pages all
green, zero open issues, no code TODO markers. The real debt is informational: START_HERE/
TODO/ROADMAP still describe 0.8.2 and a winget PR that merged on 2026-07-28, the repo's
winget manifests (0.8.2, dead URL pattern) were never submitted while winget serves 0.6.9,
main/dev each carry one unmerged commit, and Pillow 12.1.1 is the only dependency with
open OSV advisories (low practical exposure).
