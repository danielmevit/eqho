# TODO — Manual Steps

Things Daniel needs to do that can't be automated by the agent.

## Open

### whisper.cpp engine — ✅ DONE 2026-07-12 (built · runtime-verified · UI picker · installer-bundled; end-user-ready)
The v0.8.1 code auto-selects faster-whisper on NVIDIA, so nothing's broken. To light up
whisper.cpp (AMD/Intel/CPU), the Vulkan build must succeed. STATUS:
- ✅ **VERIFIED 2026-07-12:** Vulkan wheel builds clean (`BUILD_EXITCODE=0`), **`ggml-vulkan.dll`
  produced** (Vulkan backend compiled + linked), and `import _pywhispercpp` succeeds in the Eqho
  venv (`pywhispercpp-0.0.0-cp313-cp313-win_amd64.whl` installed). Wheel at `D:\pwcpp-build\`.
- ✅ **(1) RUNTIME PROOF DONE 2026-07-12** — `Model('tiny').transcribe(...)` ran on Vulkan and printed
  `ggml_vulkan: Found 2 Vulkan devices: 0 = NVIDIA RTX 3060 Laptop, 1 = AMD Radeon(TM) Graphics` →
  returned 1 `Segment`, exit 0. **The AMD device enumerated — the exact AMD-support use case works.**
  (pywhispercpp needs `requests`+`tqdm` in the venv to download the GGML model — now installed. The
  app's `_WhisperCppBackend` is already API-correct: `Segment.text` exists; `use_gpu` defaults on.)
- ✅ **(2) Distributable wheel DONE 2026-07-12.** Self-contained **17 MB** wheel at
  `D:\pwcpp-build\selfcontained\pywhispercpp-0.0.0-cp313-cp313-win_amd64.whl` — VERIFIED: `pip install`
  (`--no-deps`) + `import _pywhispercpp` OK (`SC_EXIT=0`), Vulkan inference runs. Built by
  `D:\pwcpp-build\mkwheel.py`, which injects the 5 DLLs (`whisper/ggml/ggml-base/ggml-cpu/ggml-vulkan.dll`)
  at the wheel ROOT (they install beside `_pywhispercpp.pyd`; Windows searches a loaded module's own dir
  for its deps — the reliable co-location path) and patches `RECORD` for clean pip uninstall. **This is
  the wheel to ship/commit for cp313 Windows.** (Its runtime deps `numpy/requests/tqdm/platformdirs`
  install normally; `requests`+`tqdm` are needed for the GGML model download.)
  - Why not `delvewheel`/`repairwheel`: delvewheel vendored fine (with `--add-path ...\_pywhispercpp\bin`,
    NOT the `bin\Release` pywhispercpp's setup.py wrongly guesses) into `D:\pwcpp-build\repaired\`, but its
    mangled-`.libs` auto-load needs a package `__init__` hook — and `_pywhispercpp` is a TOP-LEVEL module,
    so the patched `.pyd` hits `DLL load failed`. The root-co-location wheel above sidesteps that entirely.
- ✅ **(3) Installer bundling DONE 2026-07-12.** `packaging/windows/eqho-win.spec` now bundles pywhispercpp:
  it locates `_pywhispercpp.pyd` via `find_spec` and adds all 5 DLLs as `binaries` — ⚠️ explicitly, because
  PyInstaller's static analysis misses `ggml-vulkan.dll`/`ggml-cpu.dll` (`ggml.dll` `LoadLibrary`s them at
  runtime) — plus pywhispercpp/requests/tqdm hiddenimports. VERIFIED: a PyInstaller build placed all 6 files
  in `dist\Eqho\_internal\` (co-located with the `.pyd`), and it no-ops cleanly if pywhispercpp isn't
  installed. `installer.iss` already globs the dist folder → no installer change needed. Wheel builder
  committed at `tools/mkwheel.py`. **A build made with pywhispercpp in the build venv now ships working
  whisper.cpp/Vulkan to end users.**
- ✅ **(4) UI Engine picker DONE 2026-07-12** — General tab → Model card dropdown (Auto / faster-whisper /
  whisper.cpp, "(not installed)" hint); seamless runtime switch (`ModelHost.set_backend`,
  `VoiceTranscriber.set_engine`, reconciled in `App._on_settings_changed`). Verified headless render + logic
  + end-to-end runtime switch (faster-whisper → whisper.cpp transcribes on Vulkan).
- ✅ **C++ compiler installed** — VS 2022 BuildTools + VCTools (`cl.exe` at MSVC 14.44).
- ✅ **Vulkan SDK Core installed** — `C:\VulkanSDK\1.4.350.0`; build detects it (`glslc` found).
- ✅ **ROOT CAUSE FOUND (2026-07-12) — it is NOT a compiler-inheritance problem.** The old
  "nested `project()` can't find the compiler" theory was wrong. `cl.exe` compiles the test
  file fine; the nested `vulkan-shaders-gen` build dies at the **LINK** step with
  `LNK1104: cannot open file '...\intermediate.manifest'`. Measured the failing path = **exactly
  260 chars = Windows MAX_PATH.** `LongPathsEnabled` is off (and MSVC `link.exe`/`mt.exe`
  ignore it anyway). The space hog is setuptools' `build\temp.win-amd64-cpython-313\Release\
  _pywhispercpp\` prefix (~78 chars) stacked under `...\ggml-vulkan\vulkan-shaders-gen-prefix\
  src\vulkan-shaders-gen-build\CMakeFiles\CMakeScratch\TryCompile-...`.
- **THE FIX = build from a SHORT SOURCE path.** ⚠️ `build_ext --build-temp C:\t` does NOT work:
  `bdist_wheel` re-runs `build_ext` with its own default temp and ignores the flag, so the path
  stayed 260. What works: put the repo itself on a short root so even the default `build\temp...`
  tree stays short. Copy the recursive clone to `C:\w` and build there → nested path ~234 < 260.
  One-time wheel build, then `pip install` it (end users never compile). Runner:
  `D:\pwcpp-build\fix_build2.bat` (gitignored) = robocopy `D:\pwcpp-build\pywhispercpp`→`C:\w`
  + `vcvars64` + `GGML_VULKAN=1` + `CMAKE_GENERATOR=Ninja` + `NO_REPAIR=1` + `setup.py bdist_wheel`
  + `pip install C:\w\dist\*.whl` into the Eqho venv (whl also copied to `D:\pwcpp-build\`).
  Log: `D:\pwcpp-build\fix2.log`. Verify `ggml_vulkan: Found N devices` (this NVIDIA box exposes a
  Vulkan device, so it CAN verify end-to-end). ⚠️ `NO_REPAIR=1` skips DLL vendoring — whisper.dll
  /ggml*.dll sit next to `_pywhispercpp.pyd` (usually loads fine); if `ModuleNotFoundError:
  _pywhispercpp` or a DLL-load error appears, `pip install repairwheel` and rebuild without NO_REPAIR.
- Fallbacks if ever needed: [pre-built Vulkan whisper.cpp](https://github.com/jerryshell/whisper.cpp-windows-vulkan-bin),
  or CPU-only `pip install pywhispercpp` (~15 MB, no compiler/Vulkan — validates the backend).
- **Zero-hassle fallback (no compiler/Vulkan, CPU only, ~15 MB):** `pip install pywhispercpp`.
  Validates the whole backend on CPU; GPU/Vulkan can come later.
3. **Sanity check** — look for `ggml_vulkan: Found ... devices` in the output:
   ```powershell
   venv\Scripts\python.exe -c "from pywhispercpp.model import Model; import numpy as np; print(len(Model('tiny').transcribe(np.zeros(16000,dtype=np.float32))))"
   ```
4. **Test in app:** set `engine_backend` = `whisper.cpp` in `%APPDATA%\Eqho\settings.json`; the log prints `Inference backend: whisper.cpp`. ✅ **DONE 2026-07-12:** `_WhisperCppBackend` checked against pywhispercpp's real API — correct as-is (`Segment.text`; `use_gpu` defaults on). **UI Engine picker shipped** (General tab → Model card: Auto / faster-whisper / whisper.cpp, "(not installed)" hint) with seamless runtime switching (`ModelHost.set_backend`, `VoiceTranscriber.set_engine`, reconciled in `App._on_settings_changed`); verified end-to-end (faster-whisper → switch → whisper.cpp transcribes on Vulkan, 2 devices incl. AMD).
- [ ] Record a short demo GIF (hotkey → speak → text appears) for README + landing page. Tools: ScreenToGif or Xbox Game Bar.
- [ ] Final QA on a clean Windows machine/VM: install Eqho-Setup, dictate, uninstall.
- [ ] **winget PR #400796 — waiting on Microsoft moderator.** CLA signed ✓, validation passed ✓. Nothing to do but wait for a volunteer moderator to merge (do NOT comment/resubmit — it resets the queue). When merged, `winget install eqho` goes live.
- [ ] Autostart: use the "Start with Windows" toggle in the tray/dashboard (the v0.4.1 installer will also offer it as a checkbox).
- [ ] Public launch (ROADMAP Phase 7, do last): public README with screenshots + demo GIF; publish a GitHub Release with the installer; flip the repo public.

## Done (reference)
- [x] venv + `requirements.txt` deps + CUDA Toolkit 12.9 installed; dictation verified end-to-end (Alt+Q).
- [x] Git: two-repo setup (`origin` public, `private` WIP) with dev/main branches.
- [x] License chosen: AGPL-3.0.
- [x] Internal docs relocated — now root `AGENTS.md`/`SOUL.md`/`TODO.md` + `docs/ai/` (was `agent-instructions/`).
