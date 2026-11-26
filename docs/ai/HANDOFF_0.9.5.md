# Handoff — Starting Eqho `0.9.5` on Windows Main Station

Prepared from the Ubuntu GNOME Wayland testing session (`dev` @ `ce1dd2c` + docs) so the Windows main-station session can pick up seamlessly and stay cross-platform clean before returning to Linux for `0.9.5` verification.

## 1. Sync first on Windows (`dev` branch)
All `0.9.4` Linux + cross-platform fixes are already committed, pushed, and CI-verified (Linux, Windows, and macOS all green at `ce1dd2c`) on **`origin/dev`**. `main` is untouched.

```powershell
git checkout dev
git pull origin dev
```
*(If editing from WSL on `/mnt/d`, run `codegraph sync` after pulling.)*

## 2. What landed on `dev` since the last Windows session (already in your tree after pull)
- **Global PC-spec model recommendation (`eqho-core/src/modelpick.rs`, `eqho-ui/src/viewmodel.rs`, `eqho-app/src/app.rs`):**
  - Replaced the 2-state `HardwareClass`-only check with `HardwareSpecs` (`gpu_vram_mb`, `cpu_threads`, `cpu_fast_simd`, `available_ram_mb`) + `HardwareTier` (`GpuHigh`, `GpuMid`, `GpuEntry`, `CpuFast`, `CpuLight`).
  - Removed the hardcoded `, recommended` tag from `distil-large-v3` in `eqho-core/src/modelstore.rs`.
  - Integrated x86 GPUs now report `0` dedicated VRAM across all three OSes (Windows DXGI, Linux Vulkan, macOS Metal) so laptop iGPUs never mis-recommend `distil-large-v3`.
  - Models tab header displays real probed specs (`hardware_summary_for_specs`).
- **Linux Wayland end-to-end parity (`eqho-platform-linux`, `eqho-overlay/src/linux.rs`, `eqho-host/src/downloader.rs`, `damt-platform-linux`):**
  - Fixed `XCloseDisplay` `SIGSEGV` (`OnceLock` library caching), fresh Unix model download directory creation, PipeWire `libpulse.so.0` mic enumeration, `wpctl --help` probe, automatic GNOME Wayland `Alt+Q` global hotkey (`gsettings` + Unix IPC socket `--toggle`), Wayland `org.freedesktop.portal.Clipboard` v1 + `RemoteDesktop` v2 (`persist_mode = 2` + `restore_token` + `NotifyKeyboardKeysym` fallback), and full Linux floating overlay parity (live top transcript box, hover/pinned under-pill control bar, 99-language menu with flags/recents, recovery card).

## 3. Committed scope to include in `0.9.5` (Owner-agreed, global)
1. **Bump workspace version** `0.9.4` → `0.9.5` (stay in `0.9.x`, never `1.0` — see `AGENTS.md`).
2. **Global hardware-aware default for `Show live text` (`overlay_show_text`)** (see `docs/ai/DECISIONS.md` & `ROADMAP.md`):
   - On first launch across **Windows, macOS, and Linux**, default `overlay_show_text` to **`true`** when `hw_specs.hardware_class() == HardwareClass::Gpu` (discrete GPU / Apple Silicon where mid-sentence partials stream every ~1s) and **`false`** on CPU-only PCs (where partials are skipped and a 10–15s delayed pop-up feels broken).
   - Always honor the user's explicit toggle in **Settings → Overlay** (never silently override an explicit user setting).
   - Add a calm 1-line subtitle under the toggle: *"Real-time on GPU; updates after pauses on CPU."*
3. **Any new `0.9.5` features Daniel adds on Windows.**

## 4. Guardrails so Linux & macOS stay green while building `0.9.5` on Windows
- **Don't break `eqho-platform` frozen traits** (`rust/apps/eqho/eqho-platform/src/lib.rs`); if a new platform capability is needed, add a default no-op / stub on Linux & macOS too so all 3 CI workflows stay green.
- **Any new `OverlayCmd` or `OverlayConfig` field** added for Windows `eqho-overlay/src/win.rs` must also be handled (or cleanly stubbed) in `eqho-overlay/src/lib.rs`, `linux.rs`, and `macos.rs` so `cargo clippy --workspace --all-targets --locked -- -D warnings` passes on all 3 OSes.
- **Avoid Windows-only UI copy** where cross-platform wording is cleaner (e.g. `Start at login` vs `Start with Windows` when touching Settings labels).
- **Keep `main` untouched** — push all `0.9.5` work to `dev`. When ready for Linux porting/testing, return to the Linux laptop, `git pull origin dev`, and we'll adapt any Windows-only `0.9.5` additions to Linux and live-test the new AppImage on Ubuntu GNOME Wayland.
