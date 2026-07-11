# Gotchas — operational traps

Things that silently break or waste time. Update the moment a new trap is discovered.

## keyboard library (global hotkeys)
- Use `keyboard.hook()` — **never `keyboard.hook_key()`**, it corrupts the library's internal state.
- **Never call `keyboard.unhook_all()`** — it kills ALL hooks globally, including pystray internals. Use targeted `unhook()` / `remove_hotkey()` per hook.
- Running as a plain script requires the terminal to stay open; the PyInstaller build (`console=False`) is fine.

## Audio / volume
- COM must be initialized **per-thread** for pycaw (`CoInitialize()` before touching the volume endpoint).
- Galaxy Buds2 Pro (any Bluetooth headset): opening its mic flips A2DP→HFP — mono, awful quality, system-wide. Use the Realtek laptop mic (device 3 on Daniel's machine).
- Whisper hallucinates on silence/noise ("Thank you.", "Thanks for watching.") — since v0.3.3 gated in `_do_complete`: near-silent buffers skipped (peak RMS < 1.5×threshold), segments with `no_speech_prob>0.6 & avg_logprob<-1.0` dropped, short blocklisted utterances discarded. Don't additionally raise the VAD threshold (breaks quiet speech).

## CUDA / models
- **Model SWITCHING crashes natively (RTX 3060 laptop / cuDNN / ctranslate2), confirmed 2026-07-11.** Loading a SECOND WhisperModel in the same process segfaults instantly — no Python traceback, watchdog/excepthook catch nothing. Ordering-independent (free-first AND load-first both crash). A FRESH PROCESS loads any model fine (why launch works, switch dies). In-process reordering/gc/locks CANNOT fix it. Fix = load the model in a separate process: either restart-the-app-on-switch (small) or a subprocess model host (seamless, also isolates any native inference crash). Repro: headless loop loading model A then B via _ensure_model.
- **Never run two Eqho instances** (e.g. installed exe autostart + `python run.py` dev copy): both hook the hotkey, fight over the mic, and stack Whisper models into the 6 GB card until CUDA loads hang in WDDM paging — this presented as "app freezes when switching models". A localhost-port single-instance lock (port 48317) exits the second instance since v0.6.6. While developing, disable/uninstall the installed copy's autostart.
- `_ensure_model()` runs a CUDA smoke test at load time so a missing `cublas64_12.dll` fails fast → automatic CPU fallback (don't remove it).
- VAD constants (0.003 RMS / 1.2 s / 0.4 s) are tuned; changing them changes the dictation feel everywhere.
- Model cache resolves (since v0.3.3): `model_dir` setting → legacy `D:\EqhoModels` if it exists (auto-pinned into settings on first load) → platformdirs user cache. `HF_HUB_CACHE` is set from the resolved dir at transcriber construction/model load — not at import.
- Model loading happens on the transcription worker thread (since v0.3.3) so the hotkey callback never blocks; the overlay shows "Loading model…" until ready. Audio spoken during load is queued and transcribed once ready.

## tkinter / customtkinter
- **THE deadlock trap (v0.6.7, cost a full day):** the overlay's `tk.Tk()` is created FIRST on its own thread — tkinter makes it the process-wide default root, so any Variable/CTkFont created without an explicit master binds to the OVERLAY's interpreter; dashboard-thread `.set()` calls then marshal cross-thread and intermittently DEADLOCK (and make everything sluggish). Guards now in place: overlay sets `tk._default_root = None` right after creating its root, and tabs create Variables via `TabBase._string_var/_bool_var` (explicit master). NEVER create a bare `ctk.StringVar()`/`CTkFont()` from code that can run while another Tk root owns default — keep using the helpers.
- **UI freeze diagnosis:** the dashboard heartbeats the watchdog every 1 s; a >5 s stall dumps ALL thread stacks into `eqho.log` (`=== THREAD DUMP`). Read that before theorizing.
- Single-instance lock = localhost port 48317 (`main._acquire_single_instance`). Two instances double-hook the hotkey and stack models into VRAM until CUDA loads hang (WDDM paging).
- **Adaptive VAD (v0.6.8):** speech gate = `clamp(noise_floor×3.5, 0.0009, 0.003)`, floor tracked per session (instant down, 2% creep up). The old fixed 0.003 silently ate quiet mics. Level-bar curve `(rms/0.012)^0.6` targets mid-bar at normal voice.
- Start chime plays BLOCKING after mic start and BEFORE ducking (fire-and-forget raced the mute and was intermittently silent).

## Repo / site
- **Daniel edits public `main` via the GitHub web UI** (landing page, README tweaks). If a main push is rejected: `git pull origin main` (merge), reconcile, push — NEVER force. Then merge main back into dev.
- Landing page lives in `site/` (Astro, its own package.json); `.github/workflows/deploy.yml` builds `./site` → GitHub Pages on every main push. The app screenshot is duplicated: `assets/eqho_app_screenshot.webp` (README + bundled) and `site/public/assets/` (served page).
- Cross-thread tkinter images need the `master=` parameter or they get GC'd/bound to the wrong interpreter.
- `CTkToplevel` sets its own title-bar icon on a deferred `after()` — setting `iconphoto` at construction gets overwritten (deferred issue b; fix planned v0.4.0).
- Theme "system" for the app UI reads `AppsUseLightTheme`; the tray reads `SystemUsesLightTheme` (taskbar). They are different keys on purpose.

## WSL / repo environment
- Editing happens from WSL on `/mnt/d` → **file-watching doesn't work**: run `codegraph sync` after code edits.
- `git push` needs Windows credentials: `cmd.exe /c "cd /d D:\Vibe Coding\Eqho && git push"`.
- `core.filemode false` is set (avoids phantom mode diffs on /mnt).
- Stage specific paths; don't blind `git add -A` sweep in line-ending churn.

## Cross-platform (oskit)
- ALL OS-specific code lives in `src/oskit/` — adding a `sys.platform` branch or ctypes/winreg/osascript call anywhere else is a bug. Base-class methods are safe no-ops; unimplemented capabilities must degrade, not crash.
- Linux runtime deps (not pip): `libportaudio2` (audio), `xclip` (clipboard), `gir1.2-ayatanaappindicator3-0.1` + `python3-gi` (tray on GNOME), `xdotool` (optional focus restore). Hotkeys/injection target X11; on Wayland pynput can't grab global keys (backlog).
- macOS: the app must be granted **Accessibility** and **Input Monitoring** (System Settings → Privacy & Security) or hotkeys/typing silently do nothing. Unsigned .app → right-click → Open on first launch. CTranslate2 has no Metal — CPU int8 on Macs until the whisper.cpp era (Phase 8).
- The `keyboard` lib is Windows-only by requirements markers now — never import it at module level (lazy imports inside the keyboard-backend paths only). Hotkey capture UI (General tab) still uses it, so "click to change hotkey" is Windows-only until ported.
- Chime plays through the default output — on Linux/macOS the volume-duck subprocess calls (wpctl/osascript) are slower than pycaw; the pre-duck chime ordering matters even more there.

## Build / packaging
- Windows build lives in `packaging/windows/` (onedir spec + Inno Setup `installer.iss` + `build.ps1`); the old root onefile spec is gone. Version is parsed from `src/version.py` — never hardcode it in packaging files.
- Unsigned binaries trigger SmartScreen "unknown publisher" ("More info" → "Run anyway") — expected until code signing (backlog).
- The installer is per-user (no UAC); its "start when you sign in" checkbox writes the same HKCU Run value ("Eqho") as the in-app toggle, so they stay in sync and uninstall removes it.
- CI (`release.yml`) builds portable zip + installer on `v*` tags; `workflow_dispatch` for dry runs. The smoke gate runs first — a red smoke fails the release.
