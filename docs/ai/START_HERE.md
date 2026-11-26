# START HERE — Eqho

Always-on offline voice-to-text dictation app. System tray, **Alt+Q**, speak, text lands in the focused app. 100% local via **`whisper.cpp 1.8.4`** (Vulkan GPU on NVIDIA/AMD/Intel + multi-threaded CPU & Q5 quantized models).

## Architecture (Eqho 0.9.0 — Rust Rewrite)

Eqho 0.9.0 is a **Rust 1.95 + Slint 1.19 (`renderer-software`) + `whisper.cpp 1.8.4`** Cargo workspace under **`rust/`** (Windows x64 primary). The legacy Python implementation under `src/` is preserved for reference and Linux/macOS 0.8.x continuity.

- **Key docs to read for 0.9.x work:**
  - [`docs/rust/PLAN.md`](../rust/PLAN.md) — Rust workspace architecture, measured budgets (~3.4 MB idle RAM, ~38 ms tray startup, ~13 MB installer, ~0.25–0.38 s RTX 3060 Vulkan), and parity checklist
  - [`docs/rust/RELEASE.md`](../rust/RELEASE.md) — building `Eqho.exe`, clean-PC `whisper.cpp` DLLs, Inno Setup installer, and **Microsoft Store MSIX packaging & Partner Center submission**
  - [`docs/ai/BRAND_STYLE_GUIDE.md`](BRAND_STYLE_GUIDE.md) — mandatory UI visual rules (Deep Navy Blend `#1A489B` + `#58A6FF` accents, strict accent-only semantic rails/dots, 4× HiDPI SVG flags, Store compliance)
- **Workspace crates (`rust/`):**
  - `rust/crates/damt-*` — shared `damt.xyz` kit (`damt-app`, `damt-build`, `damt-platform`, `damt-platform-win`, `damt-theme`, `damt-ui`)
  - `rust/apps/eqho/` — `eqho-core` (pure logic: settings, history, textproc, adaptive VAD, pillfx, modelpick/modelstore), `eqho-platform` + `eqho-platform-win` (WASAPI, COM volume, `WH_KEYBOARD_LL`, clipboard/injector, DXGI), `eqho-host` (`--model-host` child + WinHTTP GGML downloader), `eqho-engine` (coordinator), `eqho-overlay` (native Win32 layered pill, under-pill micro-bar, live text panel), `eqho-ui` (Slint 1.19 Dashboard & 6-step Onboarding), `eqho-app` (`Eqho.exe` binary + optional `--features store` 15-day trial licensing)
- **Version:** `rust/Cargo.toml` (`[workspace.package] version`) for Rust 0.9.x (`src/version.py` for legacy Python reference). Stay in `0.x` (`0.9.x`); never release `1.0` without Daniel's explicit go.
- **Repo:** `origin` = `github.com/danielmevit/eqho` (PUBLIC; `dev` = all work, `main` = releases). Merge `dev` → `main` ONLY on Daniel's explicit release request. If a push bounces after a GitHub web UI edit on `main`, `git pull origin main`, merge, NEVER force-push, then merge `main` back into `dev`.
- **Landing page:** `site/` (Astro) → GitHub Pages at `eqho.damt.xyz` via `.github/workflows/deploy.yml` (builds `./site` on every `main` push), plus `site/public/llms.txt`.

## Non-Negotiable Invariants

- **Out-of-process model host (`Eqho.exe --model-host`):** never load a Whisper model in the main UI/tray process. Model/engine switches kill and respawn the child so VRAM releases cleanly and native faults never crash the app.
- **Idle means idle:** Slint 1.19 `renderer-software` ONLY (never compile in `femtovg`). No dashboard window created until opened.
- **Adaptive VAD & bounded buffers:** self-calibrating noise floor + bounded pre-roll ring buffer and audio queues.
- **Brand & Store compliance:** accent-only semantic colors (`docs/ai/BRAND_STYLE_GUIDE.md`); direct-download builds compile zero licensing code; Store builds (`--features store`) contain zero GitHub links and show `"Made with Slint"` attribution.
- **Windows MSIX packaging:** `packaging/windows/AppxManifest.xml`, `build_msix.ps1` (pure ASCII), `generate_assets.py` + `packaging/windows/assets/`.

## Run / Verify

### Rust 0.9.x (primary)
```bash
# Workspace unit & integration tests (Linux/WSL or Windows):
CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4 cargo test --manifest-path rust/Cargo.toml --workspace

# Cross-compile Windows x64 binary from WSL:
CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4 \
  cargo xwin build --manifest-path rust/Cargo.toml -p eqho-app --target x86_64-pc-windows-msvc --release

# Verify MSIX logo assets & manifest:
python3 packaging/windows/generate_assets.py --check
```

```powershell
# Native Windows build + headless smoke gate:
cargo build --manifest-path rust/Cargo.toml -p eqho-app --release
.\dist\Eqho\Eqho.exe --smoke

# Build portable zip + Inno Setup installer:
powershell -ExecutionPolicy Bypass -File packaging\windows\build-rust.ps1 -DllDir <whisper-dlls>

# Build Microsoft Store MSIX package:
powershell -ExecutionPolicy Bypass -File packaging\windows\build_msix.ps1 -Store -DllDir <whisper-dlls>
```

### Legacy Python `src/` (reference only)
```bash
cmd.exe /c 'cd /d D:\Vibe Coding\Eqho && venv\Scripts\python.exe run.py --smoke'
```

## Navigate

- Code structure, callers, call paths → CodeGraph (`codegraph explore "..."`); run `codegraph sync` after edits (WSL `/mnt` has no file-watching).
- Rust architecture & benchmarks → [`docs/rust/PLAN.md`](../rust/PLAN.md) · Packaging & Store → [`docs/rust/RELEASE.md`](../rust/RELEASE.md)
- Visual & Store rules → [`docs/ai/BRAND_STYLE_GUIDE.md`](BRAND_STYLE_GUIDE.md) · Tokens & layout → [`DESIGN_SYSTEM.md`](DESIGN_SYSTEM.md)
- Durable choices → [`DECISIONS.md`](DECISIONS.md) · Traps → [`GOTCHAS.md`](GOTCHAS.md)
- Plans → `ROADMAP.md` · Shipped → `CHANGELOG.md` · Daniel's manual steps → `TODO.md`
