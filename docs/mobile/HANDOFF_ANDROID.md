# Eqho Android — Architecture & Build Handoff (`v0.1.0` Free Edition)

> **Purpose:** Self-contained blueprint and handoff specification for starting **Eqho Android** in a dedicated build session. Covers product scope, monetization guardrails, Rust + UniFFI + `whisper.cpp` + Kotlin/Jetpack Compose architecture, Google Play technical compliance, and phased milestones.

---

## 1. Product Vision & Scope (`v0.1.0` — Free First, Pro-Ready Later)

- **Core Experience:** Calm, **100% offline** Android Voice Keyboard (`InputMethodService`) + Companion App powered by `whisper.cpp` on-device. Tap the mic in *any* Android app (WhatsApp, Gmail, Notes, Slack, Terminal, Browsers), speak in any of 99 languages, and words insert cleanly with zero cloud servers, zero accounts, and zero ads.
- **Package ID:** `xyz.damt.eqho` (matches verified `damt.xyz` domain and desktop reverse-DNS identity).
- **Monetization Strategy (Google Play Compliant):**
  - **Ship `v0.1.0` as a Free download** — full core offline dictation, model manager, history, and voice keyboard.
  - **Google Play policy fact:** You *cannot* convert a Free download into an upfront Paid download later, **but you CAN add One-Time In-App Purchases ("Pro Unlock") and/or Subscriptions** to a Free app in any future update under the same `xyz.damt.eqho` package ID.
  - **Zero Billing SDK bloat in `v0.1.0`:** Do **not** add `com.android.billingclient` or the `com.android.vending.BILLING` permission yet (avoids premature Play Console Billing / Data Safety friction).
  - **Clean future Pro seam:** Include a lightweight `EntitlementProvider` interface in Kotlin (`val isPro: StateFlow<Boolean>` defaulting to `true` for all `v0.1.0` features; future Pro-only features like custom vocabulary packs, per-app profiles, or advanced cleanup rules gate behind it when Google Play Billing is wired later).
- **Opt-In Anonymous Analytics (PostHog EU Cloud):**
  - Exact same privacy contract as desktop `0.9.5` ([`rust/crates/damt-app/src/analytics.rs`](../../rust/crates/damt-app/src/analytics.rs)):
    - **Strictly OFF by default** (`analytics_enabled = false`), unchecked opt-in card in Onboarding Step 1 + toggle in Settings (clearing the local random UUID v4 when toggled off).
    - Targets **PostHog EU Cloud** (`https://eu.i.posthog.com/capture/`) with hardcoded `"$process_person_profile": false` and `"$ip": null`.
    - Event payload properties: `app: "eqho"`, `app_version`, `edition: "play"` (or `"direct"`), `os: "android"`, `arch`, `hardware_tier`, plus event-specific anonymous fields (`model`, `language`, etc.). **Never** audio, transcripts, app names, or PII.
    - Build-time injection via Gradle/CI env `EQHO_POSTHOG_KEY` → `BuildConfig.POSTHOG_KEY` (empty/inert in community source builds).
- **AI Disclosure:** Transparently disclose AI-assisted development in Play Store/README/Privacy docs matching [`site/src/pages/privacy.astro`](../../site/src/pages/privacy.astro).

---

## 2. Repository & CI Boundaries (Protect Desktop!)

- **Monorepo placement (recommended for zero-friction `eqho-core` reuse):**
  - `rust/apps/eqho/eqho-mobile-ffi/` — Thin UniFFI Rust bridge crate exposing `eqho-core` + `whisper.cpp` bindings to Kotlin (and later Swift).
  - `mobile/android/` — Standalone Gradle project (Kotlin + Jetpack Compose + `InputMethodService` + JNI/UniFFI `.so` staging).
  - `.github/workflows/android.yml` — Dedicated Android CI on `ubuntu-24.04` (Android SDK, NDK, JDK 17 preinstalled), triggered only on changes to `mobile/android/**`, `rust/apps/eqho/eqho-core/**`, `rust/apps/eqho/eqho-mobile-ffi/**`, or `workflow_dispatch`.
- **Hard Guardrails:**
  - **NEVER touch `main`**, never publish releases/Play Store uploads without Daniel's explicit go.
  - **NEVER break desktop Rust builds:** Keep `eqho-mobile-ffi` cleanly isolated (either excluded from default desktop workspace members or compiling cleanly on desktop host targets too) so `cargo test --manifest-path rust/Cargo.toml --workspace --locked` and `cargo clippy --manifest-path rust/Cargo.toml --workspace --all-targets --locked -- -D warnings` stay 100% green.
  - **NEVER touch unrelated workspaces** (e.g. `/mnt/d/Vibe Coding/_vm/unio/...` or live `%APPDATA%\Eqho`).
  - **Disk awareness on this PC:** `D:\` (`/mnt/d`) has limited free space (~11 GB); keep Gradle caches (`GRADLE_USER_HOME`) and Rust Android target dirs (`CARGO_TARGET_DIR`) on WSL `/` (`/tmp` or `~/.cache`, ~850 GB free) or Linux host storage, NOT on `D:\`!

---

## 3. Technical Architecture

### 3.1 Shared Rust Core (`eqho-core` + `eqho-mobile-ffi`)
Good news: [`rust/apps/eqho/eqho-core/Cargo.toml`](../../rust/apps/eqho/eqho-core/Cargo.toml) is already **100% pure Rust** (`jiff`, `regex`, `serde`, `serde_json`, `thiserror`) with zero desktop OS dependencies!

Reuse directly from `eqho-core`:
- `settings::LANGUAGE_TIERS` — All 99 Whisper languages (13 `Tuned`, 20 `Strong`, 66 `Experimental`).
- `modelstore::MODELS` — Reuse model catalog & HuggingFace GGML URLs; add a mobile-friendly `tiny-q5` (~32 MB) entry if needed alongside existing `base-q5` (60 MB), `small.en-q5` (190 MB), `small-q5` (190 MB), and `large-v3-turbo-q5` (574 MB).
- `textproc` — Deterministic punctuation/cleanup & custom replacements.
- `vad` — Adaptive energy VAD for endpointing/silence trimming.
- `history` — Local JSONL transcript history + soft-delete retention.
- `recovery` — Last-dictation safety buffer.

UniFFI surface (`eqho-mobile-ffi`):
- Keep the FFI boundary coarse-grained & thread-safe (avoid chattering per audio sample over JNI):
  - `init_engine(model_path: String, threads: u32, use_gpu: bool) -> Result<()>`
  - `unload_engine()` (called on Android memory pressure / idle timeout)
  - `transcribe_pcm16(samples: Vec<i16>, language: String, initial_prompt: String) -> Result<String>` (or streaming chunk session object)
  - `compute_rms_levels(samples: Vec<i16>) -> f32` (for live waveform pill)
  - `list_languages()`, `list_mobile_models(ram_mb: u64)`, `format_transcript(...)`

### 3.2 Native Speech Engine (`whisper.cpp v1.8.4` on Android NDK)
- **ABIs:**
  - `arm64-v8a` (required primary ABI — ~99% of modern Android phones).
  - `x86_64` (debug/CI emulator testing only; release `.aab` ships `arm64-v8a`, optionally `armeabi-v7a` only if needed).
- **Build & Optimization:**
  - Pin `whisper.cpp` tag `v1.8.4` matching desktop.
  - Enable ARM64 NEON / FP16 / dotprod via NDK Clang (`-DGGML_OPENMP=OFF` for clean self-contained `.so` loading without `libomp.so` mismatches; keep CPU threads clamped to performance cores, typically `4` threads on mobile to avoid thermal throttling and efficiency-core slowdown).
  - Evaluate Android Vulkan (`libggml-vulkan.so`) as an opt-in Phase 2 enhancement — **ship CPU NEON/FP16 first** for universal reliability across Snapdragon, Exynos, Tensor, and MediaTek SoCs.
- **MANDATORY Google Play 16 KB Page-Size Compliance (Android 15+):**
  - Every native `.so` (`libwhisper.so`, `libggml*.so`, `libeqho_mobile_ffi.so`) MUST be linked with `-Wl,-z,max-page-size=16384`.
  - Add an automated CI verification step checking `readelf -l <so>` confirms `LOAD` segment alignment `0x4000` (16384 bytes) on all packaged `arm64-v8a` libraries!
- **Memory & Lifecycle Hygiene (Critical for Android IMEs):**
  - Android kills background/IME processes aggressively if RSS spikes.
  - Auto-recommend models by detected device RAM (`ActivityManager.MemoryInfo.totalMem`):
    - `<= 4 GB RAM`: `tiny-q5` / `base-q5` (60 MB)
    - `6–8 GB RAM`: `small.en-q5` / `small-q5` (190 MB)
    - `12 GB+ RAM`: `small-q5` default, `large-v3-turbo-q5` (574 MB) available with RAM note
  - Unload the Whisper context cleanly after configurable idle timeout (e.g. 2–5 minutes after keyboard hides) AND immediately on `onTrimMemory(TRIM_MEMORY_RUNNING_LOW | TRIM_MEMORY_COMPLETE)`.

### 3.3 Android App & Voice Keyboard (Kotlin + Jetpack Compose)
- **SDK Targets:** `minSdk = 29` (Android 10+, covers ~94%+ of active devices cleanly without legacy scoped-storage/audio hacks), `compileSdk = 35`, `targetSdk = 35`.
- **Visual Identity:** Strictly follow [`docs/ai/BRAND_STYLE_GUIDE.md`](../ai/BRAND_STYLE_GUIDE.md):
  - Dark mode `#0A0A0B` / `#151517`, subtle `1px` `#2A2A30` borders, Deep Navy `#1A489B` active fills + Bright `#58A6FF` accents; Light mode `#F5F6F8` / `#FFFFFF`, `#0969DA` blue.
  - **Semantic Color Law:** Warning/Success/Error colors are **accent-only** (`3px` left rail, `6px` status dot, or restrained text) — NEVER full yellow/green/red banner fills.
- **Surface 1 — Eqho Voice Keyboard (`EqhoInputMethodService`):**
  - Clean, compact keyboard view (Compose inside `AbstractComposeView` hosted by `InputMethodService` with proper `LifecycleOwner` + `SavedStateRegistryOwner` wiring — standard Jetpack pattern).
  - Controls:
    - Large centered tactile **Eqho Mic Pill** (tap to start/stop, optional hold-to-talk) with smooth voice-reactive waveform bars + quiet status text (`Tap to speak`, `Listening…`, `Transcribing…`).
    - Top utility strip: **Language Chip** (opens compact searchable recent + 99-language sheet), **Active Model badge**, **Settings gear** shortcut.
    - Bottom utility strip: **Globe / Switch Keyboard** button (`switchToPreviousInputMethod()` or `showInputMethodPicker()`), **Space**, **Undo last insert**, **Backspace** (supports hold-to-repeat), **Enter / Send** (respects `EditorInfo.imeOptions` action).
  - Text insertion: `currentInputConnection?.commitText(formatted, 1)`; if `currentInputConnection` is null or rejects commit, copy to clipboard and show an inline neutral recovery strip (`Saved & copied — tap Paste`).
- **Surface 2 — Companion App (Jetpack Compose):**
  - **Onboarding (4 calm steps):**
    1. Welcome + **unchecked** optional PostHog EU analytics toggle.
    2. Dictation Language + Phone-RAM-recommended Model selection.
    3. Grant Microphone permission (`RECORD_AUDIO`) + Enable Eqho Voice Keyboard (deep link to `Settings.ACTION_INPUT_METHOD_SETTINGS` + live check via `InputMethodManager.enabledInputMethodList` + one-tap `showInputMethodPicker()`).
    4. Download selected model (`WorkManager`) + live "Try dictating here" text field.
  - **Dashboard Tabs (Bottom Nav):**
    - **Dictate / Home:** Quick scratchpad box to test voice input, active keyboard status banner (if Eqho keyboard isn't enabled), active model & language summary.
    - **Models:** Phone-RAM specs header, English Optimized & Multilingual cards with `"Recommended for this device"`, `WorkManager` downloads (progress bar, cancel, retry, delete, active check icon).
    - **History:** Local search, one-tap copy, share, soft-delete → Recently Deleted with configurable auto-clear retention.
    - **Settings:** Voice Keyboard behavior (tap vs hold, haptics, auto-switch back after dictate, idle model unload timer), Theme (System / Dark / Light), Audio/VAD options, Offline Model Storage info, Opt-in PostHog EU analytics toggle, About + Privacy Policy (`https://eqho.damt.xyz/privacy/`) + AI disclosure.
- **Audio Capture:**
  - `AudioRecord` at **16,000 Hz**, `CHANNEL_IN_MONO`, `ENCODING_PCM_16BIT` (converted to `f32` in Rust for Whisper), `MediaRecorder.AudioSource.VOICE_RECOGNITION`.
  - Release `AudioRecord` immediately on stop/cancel/window-hide so the Android green mic indicator turns off right away!

---

## 4. Phased Build Milestones

| Milestone | Deliverable | Exit Verification Gate |
| --- | --- | --- |
| **M0 — Toolchain & Scaffolding** | Check/install JDK 17, Android SDK/NDK, `cargo-ndk`, `aarch64-linux-android` + `x86_64-linux-android` Rust targets; scaffold `rust/apps/eqho/eqho-mobile-ffi/` and `mobile/android/` Gradle project; ensure desktop `cargo test --workspace --locked` stays untouched & green. | Gradle syncs cleanly; desktop CI/tests unchanged and green. |
| **M1 — Native Engine & 16 KB Proof** | Cross-compile `whisper.cpp v1.8.4` + `eqho-mobile-ffi` (`arm64-v8a` + `x86_64`) via UniFFI with `-Wl,-z,max-page-size=16384`; wire Kotlin wrapper + unit/instrumented test transcribing a bundled 16 kHz WAV clip using `base-q5`. | `readelf -l` confirms `0x4000` (16 KB) alignment on all `.so` files; WAV clip transcribes accurately on Android emulator/device. |
| **M2 — Voice Keyboard (`InputMethodService`) MVP** | Build `EqhoInputMethodService` with Compose UI, `AudioRecord` capture, live waveform pill, VAD/stop, local transcription, `commitText()`, Undo, Backspace, Enter/Send, Language picker, and Switch-Keyboard globe button. | Dictate cleanly into Android Notes/Chrome/Messages; mic indicator shuts off immediately on stop; zero crashes on keyboard hide/rotation. |
| **M3 — Companion App, `WorkManager` Downloads & Analytics** | Complete Onboarding, RAM-aware Model Manager (`WorkManager` resumable downloads with checksum/size validation), History + Recently Deleted, Settings, `EntitlementProvider` stub (all unlocked in Free v1), and strictly opt-in PostHog EU (`os: "android"`). | Fresh install onboarding flow works end-to-end; model download survives screen-off; analytics sends 0 events while OFF and sends clean anonymous events when toggled ON. |
| **M4 — CI & Google Play Readiness** | Add `.github/workflows/android.yml` (builds debug `.apk` + unsigned release `.aab`, runs Kotlin tests + Rust tests + 16 KB ELF alignment check), finalize Privacy Policy / Play Data Safety notes, and prepare Closed Testing checklist (14-day / 12-tester gate if Personal Play account, or direct for Org account). | Green `android.yml` CI run with verified downloadable `.apk` + `.aab` artifacts ready for Daniel's real Android phone test. |

---

## 5. Starter Prompt for the New Android Session

Copy and paste the block below into a fresh AGY session opened on `/mnt/d/Vibe Coding/_vm/eqho/repo`:

```text
Read docs/mobile/HANDOFF_ANDROID.md and docs/ai/BRAND_STYLE_GUIDE.md completely before doing anything.

We are starting Eqho Android v0.1.0 (Free edition, 100% offline local voice-to-text Android Voice Keyboard + Companion App, package ID xyz.damt.eqho) using Rust (reusing rust/apps/eqho/eqho-core via UniFFI) + whisper.cpp v1.8.4 (Android NDK arm64-v8a, 16 KB page-aligned) + Kotlin / Jetpack Compose.

Hard constraints:
- Work on a dedicated feature branch branched from origin/dev (e.g. feat/android-v0.1). NEVER touch main, NEVER publish to Google Play or GitHub Releases.
- Keep existing desktop Rust builds 100% green (cargo test --manifest-path rust/Cargo.toml --workspace --locked and clippy -D warnings).
- Keep Gradle caches and Rust Android target directories off D:\ (put them on WSL / or Linux home/tmp — D:\ is nearly full).
- Free v0.1.0 only: include a clean EntitlementProvider interface stub in Kotlin (unlocked in v1) with ZERO Google Play Billing SDK / BILLING permission yet.
- PostHog EU analytics must match desktop 0.9.5: strictly OFF by default, eu.i.posthog.com, $process_person_profile: false, $ip: null, zero audio/transcripts/PII, build-time EQHO_POSTHOG_KEY.

Start with Milestone M0 & M1:
1. Check local Android toolchain availability (JDK 17+, Android SDK/NDK, cargo-ndk, Android Rust targets) and tell me cleanly if anything needs installing locally vs building via GitHub Actions ubuntu-24.04.
2. Scaffold rust/apps/eqho/eqho-mobile-ffi/ (UniFFI) + mobile/android/ (Gradle + Kotlin + Compose) + .github/workflows/android.yml.
3. Prove whisper.cpp v1.8.4 + eqho-mobile-ffi cross-compilation for arm64-v8a with verified 16 KB ELF page alignment (readelf -l Align 0x4000) before moving into M2 (EqhoInputMethodService Voice Keyboard).
```
