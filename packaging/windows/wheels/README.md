# Windows wheels bundled with Eqho releases

## pywhispercpp-0.0.0-cp313-cp313-win_amd64.whl

A self-contained `pywhispercpp` wheel (Python 3.13, Windows x64) that provides
the **whisper.cpp** inference engine with **Vulkan** GPU support — this is what
makes the Windows downloads use AMD and Intel graphics cards (and NVIDIA via
Vulkan too), instead of running on the processor.

- **What is inside:** the `pywhispercpp` Python package plus the 5 whisper.cpp
  DLLs (`whisper.dll`, `ggml.dll`, `ggml-base.dll`, `ggml-cpu.dll`,
  `ggml-vulkan.dll`) at the wheel root, so they install next to
  `_pywhispercpp.pyd` in site-packages and Windows finds them there.
- **How it was built:** `tools/mkwheel.py <upstream-wheel> <dll-dir> <this-wheel>`
  — it repacks the upstream wheel with the DLLs added at the root and RECORD
  updated. The whisper.cpp libraries were compiled with `GGML_VULKAN=1`.
- **CPU baseline:** the CPU part (`ggml-cpu.dll`) was compiled for
  **AVX2 + FMA + F16C**. It would crash on CPUs without AVX2 (e.g. Celeron /
  Pentium Silver) — which is why `src/model_host.py` only auto-selects
  whisper.cpp on Windows when the CPU reports AVX2
  (`IsProcessorFeaturePresent(PF_AVX2_INSTRUCTIONS_AVAILABLE)`).
- **Licences:** pywhispercpp and whisper.cpp are both **MIT** licensed. Eqho
  itself is AGPL-3.0 (the MIT components remain MIT).

## Rebuild / replace

1. Build the whisper.cpp DLLs with `GGML_VULKAN=1` and repack with
   `tools/mkwheel.py` (same file name, cp313 win_amd64).
2. Replace the `.whl` in this folder.
3. Update `SHA256SUMS` (`sha256sum pywhispercpp-*.whl > SHA256SUMS`) — CI
   verifies the hash before installing and fails the release on mismatch.
