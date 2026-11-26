# Microsoft Store Listing — Eqho (Ready to Paste)

Field-by-field Partner Center copy for Eqho 0.9.4. Calm, warm, concrete, factual tone.
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

## What's New in This Version (0.9.4)
- Live Overlay Preview on Screen: tune pill size, transparency, position, and live transcript styling directly on your desktop in real time from Settings → Overlay
- Streamlined Models & Settings: clean active-model blue accenting, animated model download states with inline progress, one-click Engine shortcut (Auto · GPU · CPU), and separate Engine & Driver diagnostics in About
- Crisp pure-vector header wordmark, unified tray & window monogram icon, frosted interactive 4-step section guide cards, and obsidian-charcoal dark theme with blurred Settings modal backdrop
- More reliable global hotkey activation (including live Alt+Q testing inside first-run Setup Step 6) and Windows 11 Acrylic frosted overlay polish

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
Generated via `docs/store/screenshots/` (onboarding-style 6-step walkthrough with real `eqho_core::pillfx` pill renders and 0.9.4 UI captures):
1. **01 — Press Alt+Q Anywhere:** *"Speak anywhere. It just types."* — Eqho tray monogram + live Acrylic preview + real iridescent listening pill + under-pill bar (`100% offline` · `No account needed` · `15-day free trial`).
2. **02 — Quick Floating Controls:** *"Switch across 99 languages without leaving your app"* — Real listening pill + under-pill bar + searchable 99-language menu with vector SVG flags & recent chips.
3. **03 — Dictate Your Way:** *"Your pace: Batch or Live"* — 0.9.4 Obsidian Dark General dashboard (`Audio Input`, `Hotkey`, `Speech`, `Delivery Mode`, `Custom Vocabulary`).
4. **04 — Fast Local Models:** *"Fast on your GPU — or any laptop CPU"* — 0.9.4 Models tab + guided onboarding model picker (`NVIDIA · AMD · Intel Vulkan` and compact `60 MB` Q5 laptop models).
5. **05 — Private & Never Lost:** *"Private by design. Never lose a word."* — Local History with Trash & failed-paste recovery + blurred Settings modal (`Live Preview on Screen`) + local privacy guarantees.
6. **06 — Ready in 60 Seconds:** *"Guided setup. Light & dark. Buy once."* — Obsidian Dark with frosted Interactive Guide + Crisp Light dashboard (`15-day free trial` · `One-time purchase`).

---

## Notes for Certification
Eqho is a tray app for local offline voice-to-text dictation. On first launch, a 6-step setup wizard walks through language selection, microphone testing, downloading a local GGML speech model (e.g., Base Q5 is ~60 MB), optional audio ducking, and a live dictation test box. Afterward, left-click the Eqho tray icon to open the Dashboard, or focus any text field and press **Alt+Q** to start/stop dictation. No account is required. Includes a 15-day free trial managed by the Microsoft Store licence.
