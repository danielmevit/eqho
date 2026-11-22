# Eqho 0.9.x (Rust) — Release & Packaging Guide

## 1. Building `Eqho.exe` (Windows x64)

### Native Windows (MSVC)
```powershell
cargo build --manifest-path rust/Cargo.toml -p eqho-app --release
```

### Cross-compiling from Linux/WSL (`cargo-xwin`)
```bash
CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4 \
  cargo xwin build --manifest-path rust/Cargo.toml -p eqho-app \
  --target x86_64-pc-windows-msvc --release
```

`rust/.cargo/config.toml` enables `+crt-static` for `x86_64-pc-windows-msvc`, so `Eqho.exe` itself has **zero VC++ Redistributable (`VCRUNTIME140.dll`) dependency**.

## 2. Clean-PC `whisper.cpp 1.8.4` DLL Build (CI Recommendation)

The dev bundle uses prebuilt `whisper.cpp 1.8.4` Vulkan/CPU DLLs (`whisper.dll`, `ggml.dll`, `ggml-base.dll`, `ggml-cpu.dll`, `ggml-vulkan.dll`). Two known caveats with third-party prebuilt `pywhispercpp` DLLs:
1. `ggml-cpu.dll` was compiled with MSVC OpenMP (`OPENMP = 1`, depending on `vcomp140.dll`), which faults (`0xC0000005`) on CPU-only inference at `n_threads > 1`. `eqho-host` detects `OPENMP = 1` in `whisper_print_system_info()` and automatically clamps Windows CPU threads to `1` so CPU-only PCs never crash.
2. Prebuilt DLLs dynamically link `VCRUNTIME140.dll` / `MSVCP140.dll` and require AVX2 on CPU.

For clean-PC public releases (Windows 10/11 without VC++ Redistributable installed, plus full multi-core CPU speed and older non-AVX2 CPUs), build `whisper.cpp v1.8.4` in GitHub Actions (`windows-2022` with Vulkan SDK) using:
```powershell
cmake -S whisper.cpp -B build -G Ninja `
  -DCMAKE_BUILD_TYPE=Release `
  -DBUILD_SHARED_LIBS=ON `
  -DGGML_VULKAN=ON `
  -DGGML_OPENMP=OFF `
  -DGGML_CPU_ALL_VARIANTS=ON `
  -DGGML_BACKEND_DL=ON `
  -DCMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded `
  -DWHISPER_BUILD_EXAMPLES=OFF `
  -DWHISPER_BUILD_TESTS=OFF
cmake --build build --config Release
```
With `-DGGML_OPENMP=OFF`, `OPENMP = 1` disappears from `whisper_print_system_info()` and `eqho-host` automatically unlocks multi-threaded CPU inference with zero code changes.

## 3. Portable Bundle, Inno Setup Installer & CI Workflow

- **PowerShell build script:** `packaging/windows/build-rust.ps1` (compiles `Eqho.exe`, stages DLLs + notices, runs `--smoke`, builds portable zip, Inno Setup 6 installer, and `SHA256SUMS`).
- **Inno Setup 6 script:** `packaging/windows/installer-rust.iss` (per-user, no UAC; supports `/DDevIsolated=1` for isolated `Eqho-Rust-0.9` side-by-side dev installs).
- **GitHub Actions CI:** `.github/workflows/rust-windows.yml` (compiles clean-PC `whisper.cpp v1.8.4` DLLs with Vulkan + `GGML_OPENMP=OFF` + `GGML_CPU_ALL_VARIANTS=ON` + static CRT `/MT`, builds `Eqho.exe`, runs `cargo test` + `--smoke`, uploads portable zip, installer, and SHA256SUMS).

Place the following files together in `dist/Eqho/`:
- `Eqho.exe` (~16 MB, embedded `eqho.ico`, PerMonitorV2 manifest, `VERSIONINFO`)
- `whisper.dll`
- `ggml.dll`
- `ggml-base.dll`
- `ggml-cpu.dll` (or per-ISA `ggml-cpu-*.dll` variants when `GGML_CPU_ALL_VARIANTS=ON`)
- `ggml-vulkan.dll`
- `THIRD_PARTY_NOTICES.txt`
- `LICENSE`

## 4. `0.9.0-dev` Isolation vs. `0.9.0` Production Switchover

During `0.9.0-dev` side-by-side testing, the Rust build stays 100% isolated from live Python `0.8.11`:
- Config dir: `%APPDATA%\Eqho-Rust-0.9` (`eqho-core/src/settings.rs`: `DEFAULT_CONFIG_DIR_NAME`)
- Single-instance ID: `xyz.damt.eqho.rust09` (`eqho-app/src/app.rs`: `SINGLE_INSTANCE_ID`)
- Autostart Run-key & toast icon dir: `Eqho-Rust-0.9` (`eqho-app/src/app.rs`: `build_damt_platform`)

When switching over to `0.9.0` on `dev`:
1. Change `DEFAULT_CONFIG_DIR_NAME` to `"Eqho"` (existing `0.8.11` `settings.json` + `history.jsonl` are already schema-compatible, including UTF-8 BOM and string-coerced floats).
2. Change `SINGLE_INSTANCE_ID` to `"xyz.damt.eqho"` and `build_damt_platform()` names to `"Eqho"`.
3. Bump workspace version in `rust/Cargo.toml` from `0.9.0-dev` to `0.9.0`.
4. Verify clean upgrade on Windows over an existing `0.8.11` `%APPDATA%\Eqho` profile.

## 5. Microsoft Store Build & 15-Day Free Trial

Direct-download builds (default features) compile **zero licensing code** and show no trial or purchase UI. The Microsoft Store build opts in via the Cargo feature `store` (`store = ["damt-platform-win/store"]`):

```bash
# Cross-compile Microsoft Store binary from Linux/WSL:
CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=4 \
  cargo xwin build --manifest-path rust/Cargo.toml -p eqho-app --features store \
  --target x86_64-pc-windows-msvc --release
```

### Licensing Behavior
- **Four states:** `Full`, `Trial { ends_at }`, `TrialExpired`, and `Unknown`. Only a definite `TrialExpired` suspends dictation (quietly unregisters the global hotkey, disables tray `Start Dictation`, and shows a calm top banner with `Buy Eqho`). Offline errors and `Unknown` states never lock a user out.
- **Offline cache:** Atomic `<config_dir>/license.json` (`*.tmp` → `rename`), never storing `Unknown`.
- **Check cadence:** Startup (immediately after reading cache), every 4 hours (`LICENSE_RECHECK_INTERVAL`) with an in-flight guard, on OS resume (`SessionEvent::Resumed`), and right after purchase.
- **Purchase dialog:** `StoreContext::RequestPurchaseAsync(STORE_ID)` initialized with the dashboard Win32 HWND via `IInitializeWithWindow`. `Succeeded` / `AlreadyPurchased` immediately unlock `Full`; `NotPurchased` (cancel) is a silent no-op.
- **Attribution & compliance:** Zero GitHub links in the Store build; `"Made with Slint"` attribution is shown in the trial footer and `Settings → About Eqho` (required by Slint's royalty-free license).

### Testing All Trial States Without a Store Package
In Store builds (`--features store`), combine `--demo-dashboard` (which creates an isolated temporary config dir and never touches `%APPDATA%` or `license.json`) with `--demo-license`:

```powershell
.\Eqho.exe --demo-dashboard --demo-license=trial
.\Eqho.exe --demo-dashboard --demo-license=trial-last-day
.\Eqho.exe --demo-dashboard --demo-license=trial-ended
.\Eqho.exe --demo-dashboard --demo-license=full
.\Eqho.exe --demo-dashboard --demo-license=unknown
```

In isolated demo runs, the Store API is never called, `license.json` is never written, and clicking **Buy Eqho** logs a no-op (`demo: purchase skipped`). Outside an isolated demo run (or in direct-download builds), `--demo-license` is ignored with one warning line on `stderr`.

### Partner Center Pre-Submission Checklist
1. Reserve Eqho's product name in Partner Center and replace placeholder `STORE_ID` (`"9NBLGGH4XXXX"`) in `rust/apps/eqho/eqho-app/src/license.rs` before building the Store `.msix`.
2. Configure **MSIX** package with `runFullTrust` capability and `desktop:StartupTask` (`TaskId="EqhoStartup"`).
3. Set a **paid base price** and enable a **15-day free trial** (managed by the Microsoft Store).
4. In Partner Center privacy/commerce declarations: **Uses the Microsoft commerce platform = Yes**, **Collects personal information = No**.
