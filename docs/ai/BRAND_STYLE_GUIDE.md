# Eqho Brand & UI Style Guide

> **Mandatory visual rules for all Eqho UI surfaces (Dashboard, Onboarding, Banners, Badges, Store Trial UI, and Floating Pill).** Every PR and agent task touching `.slint` or theme code MUST follow this guide.

---

## 1. Visual Identity & Tone

- **Aesthetic:** Calm, monochrome precision instrument (dark near-black `#0A0A0B` / `#151517` or crisp light `#F5F6F8` / `#FFFFFF`), subtle neutral hairline `1px` borders (`#2A2A30` dark / `#D4D6DC` light), generous whitespace, and a cohesive two-tier **Eqho Blue** system:
  - **Dark mode:** **Deep Navy Blend (`#1A489B`)** with crisp white (`#FFFFFF`) text for borderless filled buttons, active nav pills, active tab segments, and ON toggles; **Bright Accent (`#58A6FF`)** (`accent-line` / `accent-text`) for `1px–2px` thin accent strokes, inset focus rings, focused inputs, selected card borders, off-toggle rings, links, and the right leading edge of download/progress gradients (`#1A489B → #58A6FF`). Hover changes color only — never font weight.
  - **Light mode:** **Eqho Brand Blue (`#0969DA`)** with white (`#FFFFFF`) text.
- **Copy tone:** Quiet, direct, never alarmist or nagging. State what happened and give one clear action.

---

## 2. Semantic Color Law — ACCENT ONLY (NEVER FULL FILLS)

> **STRICT BAN:** Never flood a banner, notice card, tab, section, or badge container with solid or muted semantic warning/status fills (`Theme.warning-muted`, `Theme.success-muted`, `Theme.danger-muted`, or raw yellow/green/red backgrounds).

Semantic colors (**amber/yellow** = warning/attention, **green** = ready/approved/verified, **red** = error/destructive) exist to **signal state with surgical restraint**, never to paint surfaces.

### Allowed Uses of Warning / Success / Error Colors
1. **Banners & Notices** (e.g. `RecoveryBanner`, `StoreTrialBanner`, error callouts):
   - Container background: **always neutral** (`Theme.surface`).
   - Container border: **always neutral** (`1px Theme.border`).
   - Status signal: a crisp **`3px` left accent rail** (`Theme.warning` / `Theme.error` / `Theme.success`) clipped inside the rounded card + an optional **`6–7px` circular status dot** next to a neutral `Theme.text` bold heading.
   - Body copy: `Theme.text-muted` (never yellow/red paragraphs).
2. **Status Pills & Badges** (e.g. `✓ Ready`, `Download failed`, checklist indicators):
   - Background: **always neutral** (`Theme.surface-tertiary` or `transparent`).
   - Border: `1px Theme.border-subtle` (or `1px Theme.error` only on active failure states).
   - Status signal: **`6px` colored status dot** (`Theme.success` / `Theme.warning` / `Theme.error`) or restrained status text color.
3. **Inline Field Hints** (e.g. English-only model warning, mic blocked message):
   - Plain `Theme.warning` / `Theme.error` text directly below the control—no colored background box around it.
4. **Primary Brand Accent (`Theme.accent` blue):**
   - Reserved for active navigation pills, primary action buttons (`ButtonKind.accent`), toggles, slider tracks, selected checkmarks, and interactive links.

### Forbidden Anti-Patterns
- Full-width yellow/amber-filled banners or tabs (`background: Theme.warning-muted`).
- Green-filled "Success!" boxes or green-washed model cards (`background: Theme.success-muted`).
- Red-filled error blocks (`background: Theme.danger-muted`).
- Stacked or competing accent badges in the same row.

---

## 3. Card Structure & Hierarchy

- **Section Cards (`SectionCard`):** Card-integrated uppercase muted section header (`11px`, `font-weight: 700`, `letter-spacing: 0.6px`, `Theme.text-muted`) separated from card content by a crisp `1px` full-width `Theme.border-subtle` divider.
- **Spacing:** Consistent `16px` outer tab padding, `12px–16px` between cards, `12px` inside cards.
- **No nested cards-inside-cards** unless representing distinct interactive selection options (e.g. Onboarding audio ducking cards).

---

## 4. Icons, Flags & Imagery

- **Vector-first:** All UI icons use Slint `Path` vectors; all 99 language flags render from vector SVG geometry via `resvg` at **4× HiDPI (`84×56` for `21×14` logical)** with rounded corners and a subtle hairline rim so white flags remain legible on light backgrounds.
- **Never upscale tiny aliased bitmaps.**

---

## 5. Store Build Compliance

- **Zero GitHub URLs or mentions** anywhere in Store UI strings or Store-build surfaces.
- **Calm trial UI:**
  - Active trial: single quiet bottom footer line (`"N day(s) left in your trial · Buy Eqho"` + `"Made with Slint"`).
  - Expired trial: neutral `Theme.surface` top banner with accent `Buy Eqho` button—never red/yellow alarm chrome.
