# TODO — Manual Steps

Things Daniel needs to do that can't be automated by the agent.

## Open

### Activate the whisper.cpp engine (needs good internet — parked on travel connection 2026-07-12)
The dual-engine code is DONE and committed (v0.8.1), dormant — the app auto-selects
faster-whisper on NVIDIA, so nothing's broken. To light up whisper.cpp (AMD/Intel/CPU):

1. **Install a C++ compiler** (the blocker — needs ~3-5 GB, failed on travel internet):
   ```powershell
   winget install --id Microsoft.VisualStudio.2022.BuildTools --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended" --accept-package-agreements --accept-source-agreements
   ```
   (Approve the UAC prompt.) The Vulkan SDK Core is ALREADY installed at `C:\VulkanSDK\1.4.350.0`.
2. **Build pywhispercpp with Vulkan** (PowerShell, in the venv):
   ```powershell
   cd "D:\Vibe Coding\Eqho"; venv\Scripts\activate
   $env:VULKAN_SDK = "C:\VulkanSDK\1.4.350.0"; $env:GGML_VULKAN = "1"
   pip install --no-cache-dir git+https://github.com/absadiki/pywhispercpp
   ```
   (Or CPU-only, no compiler/Vulkan needed: `pip install pywhispercpp` — ~15 MB PyPI wheel.)
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
