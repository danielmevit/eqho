# Decisions — durable "why" choices

Reasoning that shouldn't be re-litigated. Append when a durable choice is made; never duplicate the changelog.

- **faster-whisper now, whisper.cpp later.** The Python + faster-whisper (CTranslate2) stack is for development velocity. The eventual native release (ROADMAP Phase 8) targets whisper.cpp: no Python runtime, small installer, CPU-fast, CUDA/Vulkan, native bindings. Don't start that migration early — installers for the Python stack ship first (Phase 4).
- **Default model `distil-large-v3`.** ~6× faster than large-v3 with <1% English accuracy loss. English-optimized; multilingual users pick large-v3-turbo/medium.
- **`large-v3` is forced to CPU.** Too large for the 6 GB VRAM RTX 3060 Laptop reference machine. Revisit with dynamic VRAM detection (planned v0.3.3+).
- **Own energy-based VAD instead of Whisper's `vad_filter`.** The built-in filter was too aggressive and discarded valid speech. Ours: RMS threshold 0.003, 1.2 s silence timeout, 0.4 s min phrase.
- **AGPL-3.0.** Strong copyleft for the public repo; keeps forks open.
- **Two repos, two branches.** `origin` (public) sees only stable releases via `main`; `private` gets every `dev` commit. GitHub default branch stays `main` on the public repo.
- **customtkinter for the dashboard.** Modern rounded widgets on plain tkinter — no heavier UI framework until the Phase-8 native shell decision.
- **UI style: Toolcraft-inspired, zero code reuse (2026-07-09).** `@pixel-point/toolcraft` is proprietary-licensed (incompatible with AGPL redistribution), so Eqho re-implements only the *design language* in original code: near-black dark surface ramp, hairline 1 px borders, compact 4–12 px radii, 11/13 px Inter density. Dark palette is Toolcraft-like; light palette stays Eqho's; geometry/component styling is shared by both themes. Accent stays Eqho blue `#58a6ff` (brand).
- **Default mic pinned to device 3 (Realtek Mic Array) on Daniel's machine.** Opening the Galaxy Buds2 Pro mic flips Bluetooth A2DP→HFP (mono, low quality). The *shipped* default becomes system-default (`None`) in v0.3.3; Daniel's setting persists.
- **`keyboard` library on Windows, `pynput` for future ports.** `keyboard.hook()` works well on Windows (after the hook_key() bug, see GOTCHAS); it needs root on Linux and fails on macOS, so Phase 6 adds a pynput backend per-OS while Windows keeps `keyboard` until validated.
