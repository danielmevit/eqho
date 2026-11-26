# Eqho `0.9.5` — Ubuntu Linux (GNOME Wayland) Test Report

Tested on `DL7490` (Ubuntu GNOME Wayland, Intel i7-8650U 8T AVX2, Intel UHD 620 iGPU, 32 GB RAM) against `origin/dev` @ `8d7ce45` (code `3a5d16c`) and CI run `45876287332` artifacts.

---

## Summary (PASS / FAIL)

| # | Area | Status | Notes |
|---|---|---|---|
| 1 | Repo sync (`origin/dev`) & SHA256 verify all 3 artifacts | **PASS** | `.deb`, `.AppImage`, `.tar.gz` match `SHA256SUMS-0.9.5-linux-x64.txt` byte-for-byte; `phc_…` key + `eu.i.posthog.com` compiled into all 3 binaries. |
| 2 | `.deb` install + `--version` + `--smoke` + `.AppImage` + `.tar.gz` | **PASS** *(deps note)* | `sudo apt install` succeeded cleanly, pulled `wl-clipboard`/`wtype`/`xclip`, ran desktop/icon triggers. `eqho --version` → `Eqho 0.9.5`. `--smoke` 6/6 OK across all 3 formats (`whisper.cpp 1.8.4`, Vulkan UHD 620 + Haswell CPU). AppImage runs cleanly **without `libfuse2`** (host has only `libfuse3-4`). See `.deb` dependency notes below. |
| 3 | Fresh `~/.config/Eqho` defaults, UI copy, GNOME Wayland | **PARTIAL / 2 GLOBAL BUGS** | Theme switching (`Light`/`Dark`/`System`), compiled 0.9.5 copy strings, GNOME `<Alt>q` hotkey, persistent `wayland-remote-token`, `small.en-q5` CPU load, and auto-paste dictation (`paste_failed: null`, zero errors) all worked. Found **2 global UI/overlay issues** below. |
| 4 | Opt-in PostHog EU analytics | **PARTIAL** | Local state verified: toggling analytics ON persisted `"analytics_enabled": true` and valid UUID v4 `"analytics_distinct_id"` in `~/.config/Eqho/settings.json`; 1 dictation completed while enabled. Not yet confirmed on PostHog EU Live events dashboard, and restart (`app_started`) + toggle-OFF (`""` + 0-event) pass not yet run (analytics was enabled early during Step 3). |

---

## Global Bugs / Changes to Address on Main Station

### 1. UI Zoom (`ui_scale`) is a no-op across all platforms — **BUG**
- **Observed:** Changing **Settings → Appearance → UI Zoom** (e.g. `125%` → `200%`) persists `"ui_scale": 2.0` into `settings.json`, but the dashboard and Settings modal do not resize or scale at all.
- **Root cause:**
  - `rust/apps/eqho/eqho-app/src/app.rs:2210-2217` (`on_ui_scale_changed`) updates `settings.ui_scale` and calls `save_settings(false)`, and `rust/apps/eqho/eqho-ui/src/viewmodel.rs:775` syncs the tab index (`set_ui_scale_index`).
  - Nothing anywhere in the workspace (`eqho-app`, `eqho-ui`, `damt-ui`, `damt-theme`) reads `settings.ui_scale` to apply scaling on startup or at runtime.

### 2. Overlay `Text Box See-Through` (`overlay_panel_opacity`) needs `0%–100%` in steps of `5%` — **GLOBAL CHANGE**
- **Requested by Daniel:** Text Box See-Through should span **`0%` to `100%`** in **increments of `5%`** (currently capped at `0%–70%`), and `100%` see-through should make the text box background/border genuinely transparent while keeping text readable/opaque.
- **Current `0.3` (`70%`) clamp & alpha floor sites:**
  - `rust/apps/eqho/eqho-ui/ui/settings_modal.slint:588-596` — slider currently sets `minimum: 0; maximum: 70; step: 5;` (change `maximum: 100`).
  - `rust/apps/eqho/eqho-core/src/settings.rs:534` — `self.overlay_panel_opacity = self.overlay_panel_opacity.clamp(0.3, 1.0);` (and proptest assertion at `settings.rs:1214`).
  - `rust/apps/eqho/eqho-overlay/src/pure.rs:87` — `settings.overlay_panel_opacity.clamp(0.3, 1.0)`.
  - `rust/apps/eqho/eqho-overlay/src/linux.rs:1625-1640` — clamps `panel_opacity` to `0.3..=1.0`, clamps `bg_alpha` to `140.0..=236.0` (dark) / `155.0..=244.0` (light), and uses fixed border alpha `48`/`40`. Should scale `bg_alpha` and `panel_border` alpha cleanly down to `0` at `panel_opacity = 0.0` (100% see-through).
  - `rust/apps/eqho/eqho-overlay/src/win.rs:1185-1188` & `1349-1364` — clamps `panel_opacity` to `0.3..=1.0`, clamps Acrylic `panel_tint` to `48..=168`, clamps `bg_alpha` to `72..=176` / `88..=190`, and uses fixed border alpha `44`/`36`. At `panel_opacity = 0.0`, Acrylic blur/tint and `bg_alpha`/border should also drop to transparent so 100% see-through matches on Windows.

---

## Linux `.deb` Packaging & Runtime Notes (`packaging/linux/build-rust.sh`)

- **`curl` missing from `.deb` `Depends`/`Recommends`:**
  - `which curl` found `/usr/bin/curl` (`8.18.0-1ubuntu2.8`) on this test machine, **but fresh stock Ubuntu Desktop installs do not always preinstall `curl`**.
  - Both `rust/crates/damt-app/src/analytics.rs:544` (Unix PostHog EU HTTPS POST) and `rust/apps/eqho/eqho-host/src/downloader.rs:432,528` (primary Whisper model downloader) execute `curl`.
  - Recommend adding **`curl`** to `.deb` `Depends` (or at minimum `Recommends`) in `packaging/linux/build-rust.sh:163-164`.
- **Other shared libs worth declaring in `.deb` `Depends`/`Recommends`:**
  - `libstdc++6` — directly `NEEDED` by bundled `/usr/lib/eqho/lib/libwhisper.so` and `libggml*.so`.
  - `libdbus-1-3`, `libcairo2`, `libpangocairo-1.0-0` — loaded via `dlopen` for Wayland Clipboard/RemoteDesktop portals and the floating overlay pill/text box (present on Ubuntu GNOME, could be absent on minimal installs).
- **`libfuse2` NOT required for `.AppImage`:**
  - Confirmed host has **neither `libfuse2` nor `libfuse2t64`** (only `libfuse3-4`), and `Eqho-0.9.5-linux-x64.AppImage --version` + `--smoke` both pass cleanly.
- **Autostart + `.deb` wrapper:**
  - Confirmed `/usr/lib/eqho/Eqho --smoke` passes **6/6** even without `--dll-dir` or `LD_LIBRARY_PATH` (via `paths.rs` `<exe_dir>/lib` lookup), so XDG autostart pointing at `/usr/lib/eqho/Eqho` works cleanly.
- **Minor icon polish:**
  - `/usr/share/icons/hicolor/64x64/apps/eqho.png` (copied from `assets/logo_64_dark.png`) is `62×62` px rather than exact `64×64` px.
- **Minor Step 3 copy note:**
  - Verified compiled into the `0.9.5` binary: `"Share anonymous usage analytics (optional)"`, `"Open system settings"`, `"Start at login"`, `"Light, dark, or follow system"`, `"Real-time on GPU; updates after pauses on CPU."`, `"Share Anonymous Usage Analytics"`, `"Recommended for this PC"`. Old Windows strings (`"Open Windows settings"`, `"Start with Windows"`) are absent.
  - Note: Onboarding Step 3's `"Open system settings"` button only renders when `mic-error != ""` (`onboarding.slint:387-409`).
  - Minor leftover Windows-centric placeholder in `settings_modal.slint:451,455` (`app.exe = typing`, `e.g. notepad.exe = typing`) under Dictation → Per-App Paste Rules.
