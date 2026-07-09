# Eqho Design System

> Source of truth for values: `src/theme.py` (tokens + `ThemeColors` palettes). This file owns the *laws* — how tokens are used. A Toolcraft-inspired retheme is planned for v0.4.0 (ROADMAP Phase 4); this doc will be rewritten then.

## Tokens (current, v0.3.x)

- **Radii:** `RADIUS_SM=6` (inputs/buttons/chips) · `RADIUS_MD=10` (cards/panels/dropdowns) · `RADIUS_LG=14` (overlay bar/modals) · `RADIUS_XL=18` (defined, currently unused).
- **Accent:** Eqho blue `#58a6ff` in dark mode; **overridden to `#0969da` in light mode** (contrast). Hover/muted variants in theme.py.
- **Type:** Inter (bundled `assets/fonts/`, loaded per-process via `AddFontResourceExW`). `FONT_SIZES`: xs 10 / sm 11 / base 13 / lg 15 / xl 18 / 2xl 22 / 3xl 28 (3xl unused). Font tuples `(FONT_FAMILY, size, weight)` are currently inlined at call sites — a `font()` helper lands with the retheme.
- **Spacing:** xs 4 / sm 8 / md 12 / lg 16 / xl 24 / 2xl 32.
- **Palettes:** `DARK` = GitHub-dark family (bg `#0d1117`, cards `#161b22`, inputs `#21262d`, border `#30363d`, fg `#e6edf3`). `LIGHT` = gray canvas `#f0f2f5`, white cards, fg `#1f2328`. All widgets must read `self._colors.<slot>` — never hardcode hex in UI code.

## Layout laws

- Dashboard window 720×520, min 600×420; fixed 170px sidebar; content area holds one scrollable tab frame at a time.
- **Responsive breakpoints measured on the content area width** (sidebar already excluded): `BP_2COL=560`, `BP_3COL=900` → 1/2/3 column grid (`uniform="col"` weights).
- Building blocks (currently in `dashboard.py`, moving to `src/ui/` in v0.4.0): `_card`, `_setting_row` (label left / control right), `_section_label` (uppercase, muted), `_tab_header`, `_make_grid_container`.
- Known inconsistency (deferred issue e, fixed in v0.4.0): card outer padding varies 24 vs 4 between single-column and grid paths.

## Theming model

- Three modes: dark / light / system, persisted in settings; switcher pill at sidebar bottom.
- **Two different Windows registry keys, intentionally:** the app UI resolves "system" via `AppsUseLightTheme` (apps theme, `theme.get_system_theme()`); the **tray icon** follows `SystemUsesLightTheme` (taskbar theme, `tray._get_taskbar_theme()`) because the taskbar can be dark while apps are light.
- Theme switch does a full dashboard teardown + rebuild (`_rebuild_ui`).
- Overlay reads `overlay_bg/fg/accent` slots and re-reads them on every show.
