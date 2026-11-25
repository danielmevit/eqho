# Microsoft Store Listing — Eqho (Ready to Paste)

Field-by-field Partner Center copy for Eqho 0.9.0. Calm, warm, concrete, factual tone.
**Owner rule:** No GitHub repository URLs and no competitor names anywhere in the listing.

---

## Product Name
**Launch name:** `Eqho`
*(Optional descriptive variant if desired and available in Partner Center: `Eqho - Offline Voice Dictation`. Whatever you reserve, set `EQHO_STORE_DISPLAY_NAME` to match it character-for-character.)*

---

## Description* (Required)
Speak anywhere Windows lets you type. Press a hotkey, talk naturally, and your words appear in whatever app is focused — documents, email, chat, browsers, notes, or code editors.

Eqho runs 100% locally on your computer. Your voice is never sent to a cloud server, no account is needed, and there are no word limits or subscriptions. Choose a compact speech model once during setup, and dictate completely offline from then on.

Built to stay light and out of your way:
• Quiet in the system tray — uses about 3.4 MB of memory while waiting and starts in a fraction of a second
• A small floating listening pill that moves with your voice and never steals focus from your active app
• Hover the pill (or pin the bar open) to cancel silently, switch languages mid-dictation, or flip between Batch and Live delivery

Fast on virtually any Windows PC:
• Hardware acceleration via Vulkan on NVIDIA, AMD, and Intel graphics cards
• Fast multi-core CPU inference with compact Q5 models (starting at 60 MB) for laptops and PCs without a dedicated graphics card
• Pick the balance of speed and accuracy that fits your PC

99 languages, honestly tiered:
• 13 hand-tuned dictation-grade languages: English, Spanish, Mandarin, Japanese, Korean, Vietnamese, Arabic, Ukrainian, French, German, Portuguese, Russian, and Italian
• 20 strong languages including Dutch, Polish, Turkish, Swedish, Czech, Romanian, Greek, Bulgarian, Hindi, and more
• 66 additional experimental languages supported by Whisper
• Switch languages in seconds with a searchable menu and quick recent-language chips

Thoughtful details for daily use:
• Batch mode (paste cleanly when you stop) or Live mode (deliver phrase by phrase while speaking)
• Private clipboard paste (kept out of Windows Win+V clipboard history) or simulated Unicode typing, with per-app paste rules
• Custom vocabulary for names and technical terms, literal text replacements, automatic punctuation cleanup, and optional voice commands ("new line", "period", "delete that")
• Local searchable transcript history with one-click copy and export
• Calm Deep Navy dark theme, crisp light theme, or follow Windows

Good to know:
• Powered by local on-device Whisper AI speech recognition (whisper.cpp); Eqho transcribes what you say without rewriting or inventing content
• No account, no ads, no tracking — network access is used only when you download a speech model or when Windows checks your Store licence
• Try it free for 15 days, then buy it once — no subscription

---

## Short Description (Optional, ~240 characters)
Calm, 100% offline voice-to-text dictation for Windows. Press Alt+Q, speak, and your words appear in any app — powered by local Whisper speech recognition on your GPU or CPU. 99 languages, no cloud, no account, 15-day free trial.

---

## Product Features (Up to 20 Short Bullets)
- Speak into any Windows app with a global hotkey (Toggle or Hold-to-Talk)
- 100% offline speech recognition running locally on your PC (whisper.cpp)
- Fast Vulkan GPU acceleration on NVIDIA, AMD, and Intel graphics cards
- Multi-core CPU mode with compact Q5 models (from 60 MB) for laptops
- 99 supported languages with crisp vector flags and live searchable switching
- Hover-reveal or pinnable under-pill bar with silent cancel and quick controls
- Batch mode (paste on stop) or Live mode (deliver phrase by phrase)
- Never steals keyboard focus from your active app
- Private clipboard paste (stays out of Windows Win+V history) or Unicode typing
- Custom vocabulary, text replacements, auto-punctuation, and opt-in voice commands
- Local searchable transcript history with date filters and one-click export
- Optional volume ducking to lower or mute other audio while you speak
- Lightweight native app (~3.4 MB idle RAM in the system tray)
- Deep Navy dark mode, light mode, and automatic Windows theme following
- Guided 6-step first-run setup with live microphone test
- No account, no cloud uploads, no ads, no telemetry
- Try free for 15 days, then buy once (no subscription)

---

## What's New in This Version (0.9.0)
- Native Rust + Slint desktop app: tiny footprint (~3.4 MB idle RAM), instant tray launch, and zero extra runtime dependencies
- Fast local Vulkan GPU acceleration (NVIDIA, AMD, Intel) and multi-core CPU inference with 4 compact Q5 speech models
- Hover-reveal or pinnable under-pill control bar with silent cancel, live timer, searchable 99-language menu with recent chips, and Batch / Live delivery toggle
- Deep Navy dark theme and crisp light theme, vector flags for all 99 languages, and a 6-step first-run setup wizard
- 24-bit and 32-bit studio microphone support, multi-monitor overlay following, and private clipboard paste

---

## Keywords (Up to 7)
voice to text · offline dictation · speech to text · voice typing · local whisper · hands free typing · talk to type

---

## Copyright and Trademark Info (Optional)
© 2026 Daniel Mevit · damt.xyz

---

## Category
**Productivity** *(optional secondary category: Utilities & tools)*

---

## Privacy Questionnaire Summary
- Eqho collects no personal data, sends no telemetry, and never uploads microphone audio or transcripts.
- All speech recognition runs locally on the user's device.
- Network requests occur only when the user explicitly downloads an offline Whisper GGML speech model (HTTPS from Hugging Face) or when Windows checks the Store trial/purchase licence via `Windows.Services.Store.StoreContext`.
- Privacy Policy URL: `https://danielmevit.github.io/eqho/privacy/`

---

## Screenshots Plan (3840×2160 & 1920×1080)
Generated via `docs/store/screenshots/` (brand slide first, calm Deep Navy / light alternating rhythm):
1. **01 — Hero:** *"Speak anywhere. It just types."* — Eqho brand + floating iridescent listening pill + under-pill bar + key badges (`100% offline` · `99 languages` · `15-day free trial`).
2. **02 — Under-pill & 99 Languages:** *"Switch languages without leaving your app"* — Floating pill + under-pill micro-bar + searchable 99-language menu with vector SVG flags & dynamic recent chips.
3. **03 — Batch & Live Delivery:** *"Your pace: Batch or Live"* — General dashboard showing `Delivery Mode`, `Hotkey Mode`, `Custom Vocabulary`, and `FlagDropdown`.
4. **04 — Fast on Any PC (GPU & CPU):** *"Fast on your GPU — or any laptop CPU"* — Models tab showing Vulkan GPU + compact Q5 CPU-fast models (`Base Q5 60 MB` through `Large v3 Turbo`).
5. **05 — Calm, Local History & Privacy:** *"Private by design, always on your PC"* — History tab + local storage / zero cloud / private clipboard guarantees.
6. **06 — Light & Dark + Guided Setup:** *"Ready in under a minute"* — Light & Dark dashboard themes + 6-step onboarding readiness view.

---

## Notes for Certification
Eqho is a tray app for local offline voice-to-text dictation. On first launch, a 6-step setup wizard walks through language selection, microphone testing, downloading a local GGML speech model (e.g., Base Q5 is ~60 MB), optional audio ducking, and a live dictation test box. Afterward, left-click the Eqho tray icon to open the Dashboard, or focus any text field and press **Alt+Q** to start/stop dictation. No account is required. Includes a 15-day free trial managed by the Microsoft Store licence.
