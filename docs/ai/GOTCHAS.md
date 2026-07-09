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
- `_ensure_model()` runs a CUDA smoke test at load time so a missing `cublas64_12.dll` fails fast → automatic CPU fallback (don't remove it).
- VAD constants (0.003 RMS / 1.2 s / 0.4 s) are tuned; changing them changes the dictation feel everywhere.
- Model cache resolves (since v0.3.3): `model_dir` setting → legacy `D:\EqhoModels` if it exists (auto-pinned into settings on first load) → platformdirs user cache. `HF_HUB_CACHE` is set from the resolved dir at transcriber construction/model load — not at import.
- Model loading happens on the transcription worker thread (since v0.3.3) so the hotkey callback never blocks; the overlay shows "Loading model…" until ready. Audio spoken during load is queued and transcribed once ready.

## tkinter / customtkinter
- Cross-thread tkinter images need the `master=` parameter or they get GC'd/bound to the wrong interpreter.
- `CTkToplevel` sets its own title-bar icon on a deferred `after()` — setting `iconphoto` at construction gets overwritten (deferred issue b; fix planned v0.4.0).
- Theme "system" for the app UI reads `AppsUseLightTheme`; the tray reads `SystemUsesLightTheme` (taskbar). They are different keys on purpose.

## WSL / repo environment
- Editing happens from WSL on `/mnt/d` → **file-watching doesn't work**: run `codegraph sync` after code edits.
- `git push` needs Windows credentials: `cmd.exe /c "cd /d D:\Vibe Coding\Eqho && git push"`.
- `core.filemode false` is set (avoids phantom mode diffs on /mnt).
- Stage specific paths; don't blind `git add -A` sweep in line-ending churn.

## Build / packaging
- PyInstaller onefile spec at repo root until v0.4.1 (then `packaging/windows/`, onedir + Inno Setup). Unsigned binaries trigger SmartScreen "unknown publisher" — expected until code signing (backlog).
