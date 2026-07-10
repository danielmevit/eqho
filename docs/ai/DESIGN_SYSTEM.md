# Eqho Design System (v0.4.0 — Toolcraft-inspired)

> Source of truth for values: `src/theme.py`. This file owns the *laws* — how tokens are used. The look is inspired by the design language of `@pixel-point/toolcraft` but is a 100% original implementation (see DECISIONS.md — their license forbids code reuse). Both themes share the same geometry; only palettes differ.

## Tokens

- **Radii:** `RADIUS_SM=4` (buttons/inputs/chips) · `RADIUS_MD=6` (dropdown popups/menus) · `RADIUS_LG=8` (cards/panels) · `RADIUS_XL=12` (overlay bar, large containers).
- **Accent:** Eqho blue `#58a6ff` (dark) / `#0969da` (light). Toolcraft-blue `#0c8ce9` is a deliberate one-line swap option, commented in theme.py. Text ON the accent fill uses the `on_accent` slot — never hardcode white.
- **Type:** Inter, via **`theme.font(size, weight)` only** — no inline font tuples anywhere in `src/ui/` (grep enforced). Scale: xs 10 / sm 11 / base 13 / lg 15 / xl 17 / 2xl 20. Dense, pro-tool sizing: controls use `sm`, body `base`.
- **Spacing:** xs 4 / sm 8 / md 12 / lg 16 / xl 24 / 2xl 32. **Law:** card/section horizontal inset from tab edge = `md`; row padding inside a card = `md`; gap between sections = `lg`. Single-column and grid layouts use the SAME padding (that inconsistency was deferred issue e).
- **Palettes** (`ThemeColors`): DARK = near-black ramp `#0a0a0b` canvas / `#151517` cards+sidebar / `#1f1f23` inputs / `#29292e` hover / hairlines `#2a2a30` (controls) + `#232327` (cards) / text `#f5f6f8` · `#a1a1aa` · `#85858f`. LIGHT = `#f5f6f8` canvas / white cards / `#ecedf0` inputs / hairlines `#d4d6dc` + `#e4e6ea` / text `#1a1c1f` · `#4b4f58` · `#74747e`. All UI reads `colors.<slot>` — zero hex literals outside theme.py.

## Components

- **Cards:** `RADIUS_LG`, `border_width=1`, `border_color=colors.border_subtle`. Hairline-bordered panels, no shadows (customtkinter can't).
- **Buttons** (recipes in `src/ui/widgets.py`): `primary_button` (accent fill, `on_accent` text), `secondary_button` (bg_tertiary + 1px border), `ghost_button` (transparent + hover). Height 28, `RADIUS_SM`, `font("sm")`.
- **ThemedDropdown** (`src/ui/widgets.py`): closed state = 1px-bordered frame, value left, chevron `▾` pinned right; popup = `RADIUS_MD` + 1px border + DWM rounding, ≤8 items visible then scrolls; keyboard: Up/Down/Enter/Escape, selected item scrolled into view.
- **Overlay:** frameless, `overlay_*` slots, DWM-rounded, pulsing accent dot (650 ms), alpha fade in/out.

## Layout laws

- Dashboard 720×520 (min 600×420); 170px fixed sidebar (`bg_secondary`); content = one scrollable tab at a time.
- **Breakpoints on content-area width** (sidebar excluded): `BP_2COL=560`, `BP_3COL=900` → 1/2/3 columns.
- Every tab records the column count it was built with; `_show_tab` rebuilds a tab whose count is stale (fixes deferred issue d — no more resize+re-click).

## Theming model

- dark / light / system, persisted; pill switcher at sidebar bottom (active segment = accent fill + `on_accent` text).
- App UI resolves "system" via `AppsUseLightTheme`; the **tray icon** follows `SystemUsesLightTheme` (taskbar key) and re-polls it every 15 s to catch live theme flips.
- **Title bar:** `DwmSetWindowAttribute` attr 20 (fallback 19) matches the title bar to the theme; title-bar icon is theme-matched and set via `after(260ms)` to outlast CTkToplevel's own deferred icon call (that race was deferred issue b).
- Theme switch = full rebuild (`_rebuild_ui`) + title bar + icon re-application.

## customtkinter limits (don't fight these)

No shadows, no blur/acrylic/mica, no gradients (image-only), whole-window alpha only. DWM calls (rounding, dark title bar) are the sanctioned escape hatch — pattern in `src/ui/win32.py` and `overlay.py`.
