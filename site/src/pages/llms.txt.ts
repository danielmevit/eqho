import type { APIRoute } from 'astro';
import { LEGACY_URL, REPO_URL, STORE_URL, VERSION } from '../lib/site';

// Generated so the Store link comes from the single STORE_URL constant.
const body = `# Eqho

> Offline voice-to-text dictation for Windows that types directly into any focused window using local OpenAI Whisper models.

- Site: https://eqho.damt.xyz/
- Get it: Microsoft Store (publisher page until the listing is live), ${STORE_URL}
- Price: a one-time purchase after a 15-day free trial. No subscription, no word limits, no ads.
- Source code: ${REPO_URL} (AGPL-3.0)
- Previous version 0.8.10 for Linux and macOS: ${LEGACY_URL}
- Support: support@damt.xyz
- Author: Daniel Mevit (https://damt.xyz)
- Development: built by damt.xyz with extensive use of generative AI tools (code, tests, documentation,
  packaging and CI, design and visuals, website, store listing and marketing, maintenance). At runtime Eqho calls no cloud AI:
  speech recognition runs locally with whisper.cpp.

## Tech Stack (v${VERSION})
* Language & UI: Rust 1.95 + Slint 1.19 (\`renderer-software\`) + native Win32 per-pixel alpha overlay (\`Eqho.exe\`, static CRT, ~3.4 MB idle RAM, ~38 ms startup)
* Engine: OpenAI Whisper via out-of-process \`whisper.cpp 1.8.4\` (\`--model-host\` child process)
* Hardware Acceleration: Cross-vendor Vulkan GPU (NVIDIA, AMD, Intel — no CUDA Toolkit required) plus clean-PC multi-threaded CPU inference (\`GGML_OPENMP=OFF\`, multi-ISA dispatch) and compact Q5 quantized GGML models
* Environments: 64-bit Windows 10/11 (primary, native Rust ${VERSION}; no 32-bit build); a macOS version is in development; legacy Python 0.8.x builds remain available for Linux X11 and macOS

## Key Features & Differentiators
* **Universal Dictation:** Types into any focused application (Word, browsers, Slack, IDEs) via private clipboard paste or simulated Unicode typing, with per-app paste rules and focus verification.
* **Fast on Any Machine:** ~0.25–0.38 s RTX 3060 Vulkan transcription; works on NVIDIA, AMD, and Intel GPUs or across all CPU cores with compact Q5 models (\`Base Q5\` 60 MB, \`Small EN Q5\` 190 MB, \`Small Q5\` 190 MB, \`Large v3 Turbo Q5\` 574 MB).
* **100% Local & Private:** No cloud, no accounts, no API keys. Audio never leaves the machine.
* **Calm Overlay & Delivery Modes:** Voice-reactive glass pill with hover-reveal/pinnable under-pill controls, optional Live Text Preview, and selectable Batch or Live delivery mode.
* **Customization:** Deep Navy Blend dark theme and light theme, custom vocabulary, opt-in voice commands ("new line", "delete that"), literal text replacements, and volume ducking.
* **99 Languages with Vector Flags:** 13 hand-tuned, 20 strong, 66 experimental — honestly tiered with crisp HiDPI vector SVG flags.
* **Distribution:** Microsoft Store (MSIX) with a 15-day free trial, then a one-time purchase. The source code is AGPL-3.0.

## Build from Source (Windows x64)
\`\`\`powershell
git clone https://github.com/danielmevit/eqho.git
cd eqho
cargo build --manifest-path rust/Cargo.toml -p eqho-app --release
\`\`\`
`;

export const GET: APIRoute = () =>
  new Response(body, { headers: { 'Content-Type': 'text/plain; charset=utf-8' } });
