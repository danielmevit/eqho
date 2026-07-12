# TODO — Manual Steps

Things Daniel needs to do that can't be automated by the agent.

## Open

### Activate the whisper.cpp engine (dual-engine code DONE + dormant; build blocked, parked 2026-07-12)
The v0.8.1 code auto-selects faster-whisper on NVIDIA, so nothing's broken. To light up
whisper.cpp (AMD/Intel/CPU), the Vulkan build must succeed. STATUS:
- ✅ **C++ compiler installed** — VS 2022 BuildTools + VCTools (`cl.exe` at MSVC 14.44).
- ✅ **Vulkan SDK Core installed** — `C:\VulkanSDK\1.4.350.0`; build detects it (`glslc` found).
- ❌ **Build fails at `vulkan-shaders-gen`** — the nested host-tool build errors at CMake
  `project()` = it isn't inheriting the C++ compiler (a known whisper.cpp Windows-Vulkan
  pain point). Plain build AND vcvars64 both hit it.
- **NEXT FIX TO TRY:** force the Ninja generator so the nested build uses `cl.exe` too.
  Run `_pwcpp_build.bat` (in repo root, gitignored): it does `call vcvars64.bat` +
  `pip install ninja` + `CMAKE_GENERATOR=Ninja` + `GGML_VULKAN=1` + the pip install.
  If Ninja still fails: fall back to a [pre-built Vulkan whisper.cpp binary](https://github.com/jerryshell/whisper.cpp-windows-vulkan-bin),
  or pin an older pywhispercpp/whisper.cpp (v1.8.0 broke Vulkan; ~1.7.6 worked).
- **Zero-hassle fallback (no compiler/Vulkan, CPU only, ~15 MB):** `pip install pywhispercpp`.
  Validates the whole backend on CPU; GPU/Vulkan can come later.
3. **Sanity check** — look for `ggml_vulkan: Found ... devices` in the output:
   ```powershell
   venv\Scripts\python.exe -c "from pywhispercpp.model import Model; import numpy as np; print(len(Model('tiny').transcribe(np.zeros(16000,dtype=np.float32))))"
   ```
4. **Test in app:** set `engine_backend` = `whisper.cpp` in `%APPDATA%\Eqho\settings.json`; the log prints `Inference backend: whisper.cpp`. Then ping the agent to reconcile any pywhispercpp API mismatch in `_WhisperCppBackend` (likely spots: the `transcribe()` numpy call, model-name handling) and add a UI Engine picker.
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
