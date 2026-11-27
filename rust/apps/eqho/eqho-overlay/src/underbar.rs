//! Pure cross-platform geometry, visibility state machine, hit-testing, elapsed timer formatting,
//! 99-language search filtering, and dynamic recent-languages chip layout for Eqho's under-pill
//! action strip and searchable language menu.

use eqho_core::settings::{LANGUAGE_TIERS, LanguageEntry};

use crate::pure::{self, OverlayRect, WorkArea};

/// Height of the under-pill micro-bar capsule in pixels.
pub const UNDERBAR_HEIGHT: u32 = 26;
/// Vertical bridge gap in pixels between the pill and the under-pill bar.
pub const UNDERBAR_GAP: i32 = 6;
/// Grace linger duration in milliseconds after the cursor leaves the hover zone.
pub const UNDERBAR_LINGER_MS: u64 = 250;
/// Minimum horizontal inset in pixels from `WorkArea` edges.
pub const UNDERBAR_EDGE_MARGIN: i32 = 8;
/// Corner radius of the under-pill frosted capsule.
pub const UNDERBAR_RADIUS: f32 = 13.0;
/// Bottom margin reserved below a bottom-anchored pill when controls are enabled so the
/// 26px underbar fits comfortably 6px below the pill with breathing room above the taskbar.
pub const UNDERBAR_BOTTOM_RESERVE: i32 = UNDERBAR_GAP + UNDERBAR_HEIGHT as i32 + 8;

/// Width of the searchable 99-language popup menu in pixels.
pub const POPUP_WIDTH: u32 = 236;
/// Horizontal padding inside the language popup in pixels.
pub const POPUP_PADDING_X: i32 = 8;
/// Top padding inside the language popup in pixels.
pub const POPUP_PADDING_TOP: i32 = 8;
/// Height of the search input box at the top of the language popup.
pub const POPUP_SEARCH_HEIGHT: u32 = 26;
/// Gap between the search box and the language list.
pub const POPUP_SEARCH_GAP: i32 = 4;
/// Height of each language list row in pixels.
pub const LANG_ROW_HEIGHT: u32 = 28;
/// Maximum number of visible language rows in the popup viewport.
pub const LANG_VISIBLE_ROWS: usize = 7;
/// Gap between the popup and the under-pill bar.
pub const POPUP_GAP: i32 = 6;
/// Bottom padding below the language list when no recent strip is visible.
pub const POPUP_BOTTOM_PAD: u32 = 6;
/// Height of each compact recent-language chip in pixels.
pub const RECENT_CHIP_HEIGHT: u32 = 22;
/// Total height of the compact bottom recent-language strip (1px divider + padding + 22px chips).
pub const RECENT_STRIP_HEIGHT: u32 = 32;
/// Horizontal gap between adjacent recent-language chips in pixels.
pub const RECENT_CHIP_GAP: u32 = 4;
/// Width of the subtle fade hint on the right/left edges of a scrollable recent chip strip.
pub const RECENT_FADE_WIDTH: u32 = 18;
/// Text shown in the under-pill bar during finishing / transcription paste.
pub const FINISHING_LABEL: &str = "Writing…";
/// Translucent opacity multiplier for the under-pill bar at rest (with frosted blur behind it).
pub const UNDERBAR_REST_ALPHA_RATIO: f32 = 0.72;
/// Elevated opacity multiplier for the under-pill bar while hovered/interacting (never 100%).
pub const UNDERBAR_HOVER_ALPHA_RATIO: f32 = 0.88;
/// Dimensions of the floating failed-paste recovery card anchored at the pill's screen position.
/// Corner radius matches Windows 11 `DWMWCP_ROUND` (`8 px`) so Acrylic backdrop clips flush.
pub const RECOVERY_CARD_WIDTH: u32 = 264;
pub const RECOVERY_CARD_HEIGHT: u32 = 76;
pub const RECOVERY_CARD_RADIUS: f32 = 8.0;

/// RGBA color tuple `(r, g, b, a)` in straight 8-bit channels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    /// Construct an opaque color from `(r, g, b)`.
    #[must_use]
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    /// Construct a color with explicit alpha `(r, g, b, a)`.
    #[must_use]
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Return a copy of this color with a replaced alpha byte.
    #[must_use]
    pub const fn with_alpha(self, a: u8) -> Self {
        Self { a, ..self }
    }
}

/// Resolved color palette for the under-pill bar and searchable language popup.
///
/// Matches Eqho's brand tokens:
/// - Dark: neutral `#151517` + `#2A2A30` hairline, Deep Navy Blend `#1A489B` filled primary,
///   Bright Accent `#58A6FF` for thin accents, active pin icon, and selected language text.
/// - Light: `#FFFFFF` + `#D4D6DC` hairline, Eqho Brand Blue `#0969DA` primary/accent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnderbarTheme {
    pub bar_bg: Rgba,
    pub bar_border: Rgba,
    pub divider: Rgba,
    pub text_primary: Rgba,
    pub text_muted: Rgba,
    pub hover_bg: Rgba,
    pub chip_bg: Rgba,
    pub chip_hover_bg: Rgba,
    pub accent_text: Rgba,
    pub pin_inactive: Rgba,
    pub pin_active: Rgba,
    pub done_bg: Rgba,
    pub done_hover_bg: Rgba,
    pub done_fg: Rgba,
    pub popup_bg: Rgba,
    pub popup_border: Rgba,
    pub search_bg: Rgba,
    pub search_border: Rgba,
    pub row_hover_bg: Rgba,
    pub scrollbar_thumb: Rgba,
    pub flag_rim: Rgba,
}

impl UnderbarTheme {
    /// Return the dark or light palette matching Eqho's brand style guide.
    #[must_use]
    pub const fn for_theme(dark: bool) -> Self {
        if dark {
            Self {
                bar_bg: Rgba::new(0x16, 0x16, 0x1A, 228),
                bar_border: Rgba::new(0x3E, 0x3E, 0x48, 225),
                divider: Rgba::new(0x3A, 0x3A, 0x42, 210),
                text_primary: Rgba::rgb(0xF5, 0xF6, 0xF8),
                text_muted: Rgba::rgb(0x98, 0x98, 0xA2),
                hover_bg: Rgba::new(0x26, 0x26, 0x2D, 220),
                chip_bg: Rgba::new(0x1E, 0x1E, 0x24, 240),
                chip_hover_bg: Rgba::new(0x28, 0x28, 0x31, 245),
                accent_text: Rgba::rgb(0x58, 0xA6, 0xFF),
                pin_inactive: Rgba::rgb(0x8E, 0x8E, 0x98),
                pin_active: Rgba::rgb(0x58, 0xA6, 0xFF),
                done_bg: Rgba::rgb(0x1A, 0x48, 0x9B),
                done_hover_bg: Rgba::rgb(0x22, 0x58, 0xB8),
                done_fg: Rgba::rgb(0xFF, 0xFF, 0xFF),
                popup_bg: Rgba::new(0x15, 0x15, 0x17, 244),
                popup_border: Rgba::rgb(0x2A, 0x2A, 0x30),
                search_bg: Rgba::rgb(0x1C, 0x1C, 0x21),
                search_border: Rgba::rgb(0x2A, 0x2A, 0x30),
                row_hover_bg: Rgba::rgb(0x22, 0x22, 0x29),
                scrollbar_thumb: Rgba::new(0x6E, 0x6E, 0x78, 170),
                flag_rim: Rgba::new(0xFF, 0xFF, 0xFF, 38),
            }
        } else {
            Self {
                bar_bg: Rgba::new(0xFF, 0xFF, 0xFF, 236),
                bar_border: Rgba::new(0xC8, 0xCB, 0xD2, 232),
                divider: Rgba::new(0xD4, 0xD6, 0xDC, 220),
                text_primary: Rgba::rgb(0x1A, 0x1C, 0x1F),
                text_muted: Rgba::rgb(0x68, 0x6A, 0x74),
                hover_bg: Rgba::new(0xEE, 0xF0, 0xF4, 225),
                chip_bg: Rgba::rgb(0xF5, 0xF6, 0xF8),
                chip_hover_bg: Rgba::rgb(0xE8, 0xEB, 0xF0),
                accent_text: Rgba::rgb(0x09, 0x69, 0xDA),
                pin_inactive: Rgba::rgb(0x74, 0x74, 0x7E),
                pin_active: Rgba::rgb(0x09, 0x69, 0xDA),
                done_bg: Rgba::rgb(0x09, 0x69, 0xDA),
                done_hover_bg: Rgba::rgb(0x05, 0x50, 0xAE),
                done_fg: Rgba::rgb(0xFF, 0xFF, 0xFF),
                popup_bg: Rgba::new(0xFF, 0xFF, 0xFF, 246),
                popup_border: Rgba::rgb(0xD4, 0xD6, 0xDC),
                search_bg: Rgba::rgb(0xF5, 0xF6, 0xF8),
                search_border: Rgba::rgb(0xD4, 0xD6, 0xDC),
                row_hover_bg: Rgba::rgb(0xF0, 0xF2, 0xF6),
                scrollbar_thumb: Rgba::new(0x9A, 0x9E, 0xA8, 170),
                flag_rim: Rgba::new(0x00, 0x00, 0x00, 32),
            }
        }
    }
}

/// Window-local rectangle in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl LocalRect {
    /// Construct a new local rectangle.
    #[must_use]
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Right edge (`x + width`).
    #[must_use]
    pub const fn right(self) -> i32 {
        self.x + self.width as i32
    }

    /// Bottom edge (`y + height`).
    #[must_use]
    pub const fn bottom(self) -> i32 {
        self.y + self.height as i32
    }

    /// Return `true` when `(px, py)` lies inside this rectangle (half-open `[x, right) × [y, bottom)`).
    #[must_use]
    pub const fn contains(self, px: i32, py: i32) -> bool {
        px >= self.x && py >= self.y && px < self.right() && py < self.bottom()
    }
}

/// Which vertical side of the pill the under-pill bar was placed on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnderbarSide {
    Below,
    Above,
}

/// Full screen-space placement of the under-pill bar and the hover bridge connecting it to the pill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnderbarPlacement {
    pub bar_rect: OverlayRect,
    pub bridge_rect: OverlayRect,
    pub side: UnderbarSide,
}

/// Calculate the screen-space placement of the `26px`-tall under-pill bar and its `6px` hover bridge.
///
/// Places the bar horizontally centered under `pill` (clamped inside `area`), `6px` below `pill`
/// whenever bottom clearance allows (`below_y + 26 <= area.bottom`). If bottom clearance is
/// insufficient, cleanly flips to `6px` above `pill`.
#[must_use]
pub fn calc_underbar_placement(
    pill: OverlayRect,
    bar_w: u32,
    area: WorkArea,
) -> UnderbarPlacement {
    let bw = bar_w as i32;
    let bh = UNDERBAR_HEIGHT as i32;
    let pill_cx = pill.x + (pill.width as i32) / 2;
    let raw_x = pill_cx - bw / 2;

    let (min_x, max_x) = if area.width() >= bw + 2 * UNDERBAR_EDGE_MARGIN {
        (
            area.left + UNDERBAR_EDGE_MARGIN,
            area.right - UNDERBAR_EDGE_MARGIN - bw,
        )
    } else {
        (area.left, (area.right - bw).max(area.left))
    };
    let x = raw_x.clamp(min_x, max_x);

    let below_y = pill.y + pill.height as i32 + UNDERBAR_GAP;
    let above_y = pill.y - UNDERBAR_GAP - bh;
    let clamp_min_y = area.top;
    let clamp_max_y = (area.bottom - bh).max(area.top);

    let (y, side) = if below_y + bh <= area.bottom {
        (below_y.clamp(clamp_min_y, clamp_max_y), UnderbarSide::Below)
    } else if above_y >= area.top {
        (above_y.clamp(clamp_min_y, clamp_max_y), UnderbarSide::Above)
    } else {
        // Extremely constrained work area: choose whichever side has more room.
        let space_below = area.bottom - (pill.y + pill.height as i32);
        let space_above = pill.y - area.top;
        if space_above > space_below {
            (above_y.clamp(clamp_min_y, clamp_max_y), UnderbarSide::Above)
        } else {
            (below_y.clamp(clamp_min_y, clamp_max_y), UnderbarSide::Below)
        }
    };

    let bar_rect = OverlayRect {
        x,
        y,
        width: bar_w,
        height: UNDERBAR_HEIGHT,
    };

    let bridge_left = pill.x.min(bar_rect.x);
    let bridge_right = (pill.x + pill.width as i32).max(bar_rect.x + bw);
    let bridge_w = (bridge_right - bridge_left).max(0) as u32;
    let (bridge_y, bridge_h) = match side {
        UnderbarSide::Below => {
            let top = pill.y + pill.height as i32;
            (top, (bar_rect.y - top).max(0) as u32)
        }
        UnderbarSide::Above => {
            let top = bar_rect.y + bh;
            (top, (pill.y - top).max(0) as u32)
        }
    };
    let bridge_rect = OverlayRect {
        x: bridge_left,
        y: bridge_y,
        width: bridge_w,
        height: bridge_h,
    };

    UnderbarPlacement {
        bar_rect,
        bridge_rect,
        side,
    }
}

/// Calculate the `26px`-tall under-pill bar rectangle relative to `pill` and `area`.
#[must_use]
pub fn calc_underbar_rect(pill: OverlayRect, bar_w: u32, area: WorkArea) -> OverlayRect {
    calc_underbar_placement(pill, bar_w, area).bar_rect
}

/// Adjust a bottom-anchored pill position upward when controls are enabled so the `26px` bar
/// sits `6px` below the pill inside `area` without flipping above or touching the taskbar.
#[must_use]
pub fn calc_pill_position_with_controls(
    position: &str,
    pill_w: u32,
    pill_h: u32,
    area: WorkArea,
    controls_enabled: bool,
) -> (i32, i32) {
    let (px, py) = pure::calc_pill_position(position, pill_w, pill_h, area);
    if !controls_enabled || pure::is_anchored_top(position) {
        return (px, py);
    }
    let desired_y = area.bottom - pill_h as i32 - UNDERBAR_BOTTOM_RESERVE;
    if desired_y >= area.top + pure::MARGIN {
        (px, desired_y)
    } else {
        (px, py)
    }
}

/// Calculate the live transcription panel rectangle while guaranteeing zero overlap with an
/// active/visible under-pill bar.
///
/// - For top-anchored pills, when the underbar sits below the pill, the panel is shifted
///   `GAP` below the underbar.
/// - For bottom-anchored pills, if the underbar had to flip above the pill, the panel is
///   shifted `GAP` above the underbar.
#[must_use]
pub fn calc_panel_rect_with_underbar(
    position: &str,
    pill: OverlayRect,
    underbar: Option<OverlayRect>,
    panel_w: u32,
    panel_h: u32,
    area: WorkArea,
) -> OverlayRect {
    let mut rect = pure::calc_panel_rect(position, pill, panel_w, panel_h, area);
    if let Some(bar) = underbar {
        if pure::is_anchored_top(position) {
            if bar.y >= pill.y + pill.height as i32 {
                rect.y = bar.y + bar.height as i32 + pure::GAP;
            }
        } else if bar.y + bar.height as i32 <= pill.y {
            rect.y = bar.y - pure::GAP - panel_h as i32;
        }
    }
    rect
}

/// Return `true` if screen point `(x, y)` lies inside `rect`.
#[must_use]
pub const fn point_in_overlay_rect(x: i32, y: i32, rect: OverlayRect) -> bool {
    x >= rect.x
        && y >= rect.y
        && x < rect.x + rect.width as i32
        && y < rect.y + rect.height as i32
}

/// Return `true` when screen cursor `(cx, cy)` is inside the active hover zone.
///
/// Always includes the pill and the `6px` bridge rect; once the bar or language popup is shown,
/// includes those rects as well so gliding across controls never flickers.
#[must_use]
pub const fn is_cursor_in_hover_zone(
    cx: i32,
    cy: i32,
    pill: OverlayRect,
    bridge: OverlayRect,
    bar: Option<OverlayRect>,
    popup: Option<OverlayRect>,
) -> bool {
    if point_in_overlay_rect(cx, cy, pill) || point_in_overlay_rect(cx, cy, bridge) {
        return true;
    }
    if let Some(b) = bar
        && point_in_overlay_rect(cx, cy, b)
    {
        return true;
    }
    if let Some(p) = popup
        && point_in_overlay_rect(cx, cy, p)
    {
        return true;
    }
    false
}

/// Deterministic hover-reveal + grace-linger + lock state machine for the under-pill bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnderbarVisibility {
    pub controls_enabled: bool,
    pub pinned: bool,
    pub lang_menu_open: bool,
    pub finishing: bool,
    pub hovered: bool,
    pub visible: bool,
    pub linger_elapsed_ms: u64,
}

impl UnderbarVisibility {
    /// Create a fresh visibility state for a dictation session.
    ///
    /// Starts hidden (`visible == false`) when `pinned == false` (Variant C hover-reveal),
    /// and immediately visible when `controls_enabled && pinned`.
    #[must_use]
    pub const fn new(controls_enabled: bool, pinned: bool) -> Self {
        Self {
            controls_enabled,
            pinned,
            lang_menu_open: false,
            finishing: false,
            hovered: false,
            visible: controls_enabled && pinned,
            linger_elapsed_ms: 0,
        }
    }

    /// Reset transient hover/menu/finishing state at the start of a new dictation session.
    pub fn reset_for_session(&mut self, controls_enabled: bool, pinned: bool) {
        self.controls_enabled = controls_enabled;
        self.pinned = pinned;
        self.lang_menu_open = false;
        self.finishing = false;
        self.hovered = false;
        self.visible = controls_enabled && pinned;
        self.linger_elapsed_ms = 0;
    }

    /// Reset visibility when the overlay hides completely.
    pub fn reset_for_hide(&mut self) {
        self.lang_menu_open = false;
        self.finishing = false;
        self.hovered = false;
        self.visible = false;
        self.linger_elapsed_ms = 0;
    }

    /// Return `true` when any lock (`pinned`, `lang_menu_open`, or `finishing`) keeps the bar shown.
    #[must_use]
    pub const fn is_locked_visible(&self) -> bool {
        self.controls_enabled && (self.pinned || self.lang_menu_open || self.finishing)
    }

    /// Update the pinned flag; pinning immediately shows the bar and clears any linger countdown.
    pub fn set_pinned(&mut self, pinned: bool) {
        self.pinned = pinned;
        if self.controls_enabled && pinned {
            self.visible = true;
            self.linger_elapsed_ms = 0;
        }
    }

    /// Update whether the searchable language menu popup is currently open.
    pub fn set_lang_menu_open(&mut self, open: bool) {
        self.lang_menu_open = open;
        if self.controls_enabled && open {
            self.visible = true;
            self.linger_elapsed_ms = 0;
        }
    }

    /// Update whether the overlay is in the `"Writing…"` finishing state.
    pub fn set_finishing(&mut self, finishing: bool) {
        self.finishing = finishing;
        if self.controls_enabled && finishing {
            self.visible = true;
            self.linger_elapsed_ms = 0;
        }
    }

    /// Update whether controls are globally enabled.
    pub fn set_controls_enabled(&mut self, enabled: bool) {
        self.controls_enabled = enabled;
        if !enabled {
            self.visible = false;
            self.hovered = false;
            self.lang_menu_open = false;
            self.linger_elapsed_ms = 0;
        } else if self.is_locked_visible() {
            self.visible = true;
            self.linger_elapsed_ms = 0;
        }
    }

    /// Advance the state machine by `dt_ms` milliseconds given whether the cursor is in the hover zone.
    ///
    /// Returns whether the under-pill bar should be visible after this tick.
    pub fn tick(&mut self, dt_ms: u64, cursor_in_zone: bool) -> bool {
        if !self.controls_enabled {
            self.hovered = false;
            self.visible = false;
            self.linger_elapsed_ms = 0;
            return false;
        }

        self.hovered = cursor_in_zone;

        if cursor_in_zone || self.is_locked_visible() {
            self.visible = true;
            self.linger_elapsed_ms = 0;
            return true;
        }

        if !self.visible {
            self.linger_elapsed_ms = 0;
            return false;
        }

        self.linger_elapsed_ms = self.linger_elapsed_ms.saturating_add(dt_ms);
        if self.linger_elapsed_ms >= UNDERBAR_LINGER_MS {
            self.visible = false;
            self.linger_elapsed_ms = 0;
        }
        self.visible
    }
}

/// Interactive controls inside the under-pill capsule (left → right).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnderbarControl {
    Cancel,
    Language,
    DeliveryMode,
    Pin,
    Done,
}

/// Computed local-coordinate layout of every element inside the `26px` under-pill capsule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnderbarLayout {
    pub width: u32,
    pub height: u32,
    pub cancel: LocalRect,
    pub divider_x: i32,
    pub timer: LocalRect,
    pub language: LocalRect,
    pub delivery_mode: LocalRect,
    pub pin: LocalRect,
    pub done: LocalRect,
}

impl UnderbarLayout {
    /// Compute the deterministic left-to-right control geometry for the compact `26px` capsule.
    ///
    /// Keeps the bar narrower than the dictation pill (`177px` vs `224px` pill at 80% scale) by
    /// rendering crisp regular-weight language text (`EN ▾`, no flag on the bar itself — flags
    /// stay inside the 99-language popup).
    #[must_use]
    pub const fn compute(_has_flag: bool) -> Self {
        let h = UNDERBAR_HEIGHT;
        let cancel = LocalRect::new(3, 3, 18, 20);
        let divider_x = 23;
        let timer = LocalRect::new(27, 3, 30, 20);
        let language = LocalRect::new(59, 3, 32, 20);
        let delivery_x = language.right() + 2;
        let delivery_mode = LocalRect::new(delivery_x, 3, 36, 20);
        let pin_x = delivery_mode.right() + 2;
        let pin = LocalRect::new(pin_x, 3, 19, 20);
        let done_x = pin.right() + 2;
        let done = LocalRect::new(done_x, 3, 22, 20);
        let width = (done.right() + 3) as u32;

        Self {
            width,
            height: h,
            cancel,
            divider_x,
            timer,
            language,
            delivery_mode,
            pin,
            done,
        }
    }

    /// Hit-test a local coordinate `(px, py)` against the interactive controls.
    #[must_use]
    pub const fn hit_test(&self, px: i32, py: i32) -> Option<UnderbarControl> {
        if self.cancel.contains(px, py) {
            Some(UnderbarControl::Cancel)
        } else if self.language.contains(px, py) {
            Some(UnderbarControl::Language)
        } else if self.delivery_mode.contains(px, py) {
            Some(UnderbarControl::DeliveryMode)
        } else if self.pin.contains(px, py) {
            Some(UnderbarControl::Pin)
        } else if self.done.contains(px, py) {
            Some(UnderbarControl::Done)
        } else {
            None
        }
    }
}

/// Format elapsed session seconds as `M:SS` (e.g., `0:00`, `0:14`, `1:05`, `12:34`).
#[must_use]
pub fn format_elapsed(elapsed_secs: u64) -> String {
    let mins = elapsed_secs / 60;
    let secs = elapsed_secs % 60;
    format!("{mins}:{secs:02}")
}

/// Normalize a delivery mode string to canonical `"batch"` or `"live"`.
#[must_use]
pub fn normalize_delivery_mode(mode: &str) -> &'static str {
    if mode.trim().eq_ignore_ascii_case("live") {
        "live"
    } else {
        "batch"
    }
}

/// Return the toggled delivery mode (`"batch"` <-> `"live"`).
#[must_use]
pub fn next_delivery_mode(current: &str) -> &'static str {
    if normalize_delivery_mode(current) == "live" {
        "batch"
    } else {
        "live"
    }
}

/// Human-readable label for the delivery mode chip (`"Batch"` or `"Live"`).
#[must_use]
pub fn delivery_mode_label(mode: &str) -> &'static str {
    if normalize_delivery_mode(mode) == "live" {
        "Live"
    } else {
        "Batch"
    }
}

/// Format a Whisper language code as a crisp uppercase badge (`"en"` -> `"EN"`).
#[must_use]
pub fn format_language_code(code: &str) -> String {
    let trimmed = code.trim();
    if trimmed.is_empty() {
        "EN".to_owned()
    } else {
        trimmed.to_ascii_uppercase()
    }
}

fn fold_for_search(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| match c {
            'ā' | 'á' | 'à' | 'â' | 'ä' => 'a',
            'ē' | 'é' | 'è' | 'ê' | 'ë' => 'e',
            'ī' | 'í' | 'ì' | 'î' | 'ï' => 'i',
            'ō' | 'ó' | 'ò' | 'ô' | 'ö' => 'o',
            'ū' | 'ú' | 'ù' | 'û' | 'ü' => 'u',
            other => other,
        })
        .collect()
}

/// Filter all 99 supported Whisper languages from [`LANGUAGE_TIERS`] by case-insensitive substring
/// on code or display name.
///
/// - An empty/whitespace query returns all 99 entries in canonical tier order.
/// - Non-empty queries rank exact code matches first, then name prefix matches, then code prefix
///   matches, then general substring matches (preserving canonical tier order within each rank).
#[must_use]
pub fn filter_languages(query: &str) -> Vec<&'static LanguageEntry> {
    let q = fold_for_search(query.trim());
    if q.is_empty() {
        return LANGUAGE_TIERS.iter().collect();
    }

    let mut exact_code = Vec::new();
    let mut name_prefix = Vec::new();
    let mut code_prefix = Vec::new();
    let mut substring = Vec::new();

    for entry in LANGUAGE_TIERS {
        let code_l = entry.code.to_ascii_lowercase();
        let name_l = fold_for_search(entry.name);
        if code_l == q {
            exact_code.push(entry);
        } else if name_l.starts_with(&q) {
            name_prefix.push(entry);
        } else if code_l.starts_with(&q) {
            code_prefix.push(entry);
        } else if name_l.contains(&q) || code_l.contains(&q) {
            substring.push(entry);
        }
    }

    exact_code.extend(name_prefix);
    exact_code.extend(code_prefix);
    exact_code.extend(substring);
    exact_code
}

/// Geometry of a single chip inside the compact bottom recent-languages strip.
///
/// `x` is relative to the strip's inner left edge (`0..row_width`) after applying horizontal
/// scroll offset, so peeking/off-screen chips may have `x < 0` or `x + width > row_width`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentChip {
    pub code: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Layout result for the compact bottom recent-languages chip strip (no heading label).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecentStripLayout {
    pub height: u32,
    pub row_width: u32,
    pub chip_width: u32,
    pub scrollable: bool,
    pub scroll_index: usize,
    pub max_scroll: usize,
    pub scroll_offset_px: i32,
    pub show_left_fade: bool,
    pub show_right_fade: bool,
    pub chips: Vec<RecentChip>,
}

/// Deduplicate recent language codes cleanly (trimmed lowercase, non-empty, order preserved).
#[must_use]
pub fn sanitize_recent_languages(recents: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(recents.len());
    for raw in recents {
        let code = raw.trim().to_ascii_lowercase();
        if !code.is_empty() && !out.contains(&code) {
            out.push(code);
        }
    }
    out
}

/// Compute the compact bottom recent-languages chip row layout.
///
/// Rules:
/// - `0` or `1` recent languages → completely hidden (`height == 0`, `chips.is_empty()`).
/// - `2` recent languages → 2 equal chips (~50% width each), `scrollable == false`.
/// - `3` recent languages → 3 equal chips (~33.3% width each), `scrollable == false`.
/// - `4+` recent languages → chips sized so ~3.5 chips fit across `row_width` (4th chip peeks at
///   the right edge under a fade hint), `scrollable == true`, `max_scroll == count - 3`, with
///   1-chip-per-wheel-notch stepping.
#[must_use]
pub fn calc_recent_strip_layout(
    recents: &[String],
    row_width: u32,
    scroll_index: usize,
) -> RecentStripLayout {
    let codes = sanitize_recent_languages(recents);
    let count = codes.len();
    if count < 2 || row_width == 0 {
        return RecentStripLayout {
            height: 0,
            row_width,
            chip_width: 0,
            scrollable: false,
            scroll_index: 0,
            max_scroll: 0,
            scroll_offset_px: 0,
            show_left_fade: false,
            show_right_fade: false,
            chips: Vec::new(),
        };
    }

    let gap = RECENT_CHIP_GAP;
    let chip_y = 6; // 1px top hairline + 5px gap inside 32px strip

    if count == 2 {
        let usable = row_width.saturating_sub(gap);
        let w0 = usable / 2;
        let w1 = usable.saturating_sub(w0);
        let chips = vec![
            RecentChip {
                code: codes[0].clone(),
                x: 0,
                y: chip_y,
                width: w0,
                height: RECENT_CHIP_HEIGHT,
            },
            RecentChip {
                code: codes[1].clone(),
                x: (w0 + gap) as i32,
                y: chip_y,
                width: w1,
                height: RECENT_CHIP_HEIGHT,
            },
        ];
        return RecentStripLayout {
            height: RECENT_STRIP_HEIGHT,
            row_width,
            chip_width: w0,
            scrollable: false,
            scroll_index: 0,
            max_scroll: 0,
            scroll_offset_px: 0,
            show_left_fade: false,
            show_right_fade: false,
            chips,
        };
    }

    if count == 3 {
        let usable = row_width.saturating_sub(2 * gap);
        let base_w = usable / 3;
        let rem = usable % 3;
        let mut x = 0i32;
        let mut chips = Vec::with_capacity(3);
        for (i, code) in codes.into_iter().enumerate() {
            let w = base_w + u32::from((i as u32) < rem);
            chips.push(RecentChip {
                code,
                x,
                y: chip_y,
                width: w,
                height: RECENT_CHIP_HEIGHT,
            });
            x += (w + gap) as i32;
        }
        return RecentStripLayout {
            height: RECENT_STRIP_HEIGHT,
            row_width,
            chip_width: base_w,
            scrollable: false,
            scroll_index: 0,
            max_scroll: 0,
            scroll_offset_px: 0,
            show_left_fade: false,
            show_right_fade: false,
            chips,
        };
    }

    // 4+ recent languages: size chips so ~3.5 chips (3 full chips + 3 gaps + 0.5 peeking chip)
    // span `row_width`.
    let chip_w = (((row_width as f32) - 3.0 * (gap as f32)) / 3.5)
        .floor()
        .max(24.0) as u32;
    let step_px = (chip_w + gap) as i32;
    let total_w = (count as u32) * chip_w + ((count as u32).saturating_sub(1)) * gap;
    let max_offset_px = total_w.saturating_sub(row_width) as i32;
    let max_scroll = count.saturating_sub(3);
    let clamped_scroll = scroll_index.min(max_scroll);
    let scroll_offset_px = ((clamped_scroll as i32) * step_px).min(max_offset_px);

    let mut chips = Vec::with_capacity(count);
    for (i, code) in codes.into_iter().enumerate() {
        let x = (i as i32) * step_px - scroll_offset_px;
        chips.push(RecentChip {
            code,
            x,
            y: chip_y,
            width: chip_w,
            height: RECENT_CHIP_HEIGHT,
        });
    }

    RecentStripLayout {
        height: RECENT_STRIP_HEIGHT,
        row_width,
        chip_width: chip_w,
        scrollable: true,
        scroll_index: clamped_scroll,
        max_scroll,
        scroll_offset_px,
        show_left_fade: scroll_offset_px > 0,
        show_right_fade: scroll_offset_px < max_offset_px,
        chips,
    }
}

/// Step the recent-language horizontal scroll index by `notch_steps` (1 chip per wheel notch),
/// clamped strictly to `0..=max_scroll`.
#[must_use]
pub fn step_recent_scroll(current: usize, notch_steps: i32, max_scroll: usize) -> usize {
    if max_scroll == 0 {
        return 0;
    }
    let next = (current as isize).saturating_add(notch_steps as isize);
    next.clamp(0, max_scroll as isize) as usize
}

/// Convert a Win32 `WM_MOUSEWHEEL` delta (`WHEEL_DELTA = 120`) into signed step notches.
///
/// Scrolling down (`delta < 0`) yields positive notches (scroll forward / right);
/// scrolling up (`delta > 0`) yields negative notches (scroll backward / left).
#[must_use]
pub const fn wheel_delta_to_notches(wheel_delta: i16) -> i32 {
    if wheel_delta == 0 {
        0
    } else if wheel_delta < 0 {
        let abs = (-(wheel_delta as i32)) / 120;
        if abs > 0 { abs } else { 1 }
    } else {
        let abs = (wheel_delta as i32) / 120;
        -(if abs > 0 { abs } else { 1 })
    }
}

/// Hit-test result for the searchable 99-language popup menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PopupHit {
    SearchInput,
    ClearSearch,
    LanguageRow(usize),
    RecentChip(usize),
}

/// Complete local layout for the searchable 99-language popup window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LangPopupLayout {
    pub width: u32,
    pub height: u32,
    pub search_rect: LocalRect,
    pub clear_rect: LocalRect,
    pub list_rect: LocalRect,
    pub visible_rows: usize,
    pub total_rows: usize,
    pub recent_strip_y: i32,
    pub recent_strip: RecentStripLayout,
}

impl LangPopupLayout {
    /// Compute the popup geometry for the current filtered row count and recent languages.
    #[must_use]
    pub fn compute(
        total_rows: usize,
        recents: &[String],
        recent_scroll_index: usize,
    ) -> Self {
        let width = POPUP_WIDTH;
        let inner_w = width.saturating_sub((2 * POPUP_PADDING_X) as u32);
        let search_rect = LocalRect::new(
            POPUP_PADDING_X,
            POPUP_PADDING_TOP,
            inner_w,
            POPUP_SEARCH_HEIGHT,
        );
        let clear_rect = LocalRect::new(
            search_rect.right() - 22,
            search_rect.y + 3,
            20,
            20,
        );
        let visible_rows = total_rows.clamp(1, LANG_VISIBLE_ROWS);
        let list_h = (visible_rows as u32) * LANG_ROW_HEIGHT;
        let list_y = search_rect.bottom() + POPUP_SEARCH_GAP;
        let list_rect = LocalRect::new(POPUP_PADDING_X, list_y, inner_w, list_h);

        let recent_strip = calc_recent_strip_layout(recents, inner_w, recent_scroll_index);
        let recent_strip_y = list_rect.bottom() + 4;
        let height = if recent_strip.height > 0 {
            (recent_strip_y as u32) + recent_strip.height
        } else {
            (list_rect.bottom() as u32) + POPUP_BOTTOM_PAD
        };

        Self {
            width,
            height,
            search_rect,
            clear_rect,
            list_rect,
            visible_rows,
            total_rows,
            recent_strip_y,
            recent_strip,
        }
    }

    /// Maximum list scroll offset (first visible row index).
    #[must_use]
    pub fn max_list_scroll(&self) -> usize {
        self.total_rows.saturating_sub(self.visible_rows)
    }

    /// Return the local bounding rectangle of the recent-languages strip, if visible.
    #[must_use]
    pub const fn recent_strip_rect(&self) -> Option<LocalRect> {
        if self.recent_strip.height == 0 {
            None
        } else {
            Some(LocalRect::new(
                POPUP_PADDING_X,
                self.recent_strip_y,
                self.recent_strip.row_width,
                self.recent_strip.height,
            ))
        }
    }

    /// Hit-test a popup-local point `(px, py)` given whether a search query is present and the
    /// current list scroll index.
    #[must_use]
    pub fn hit_test(
        &self,
        px: i32,
        py: i32,
        has_query: bool,
        list_scroll: usize,
    ) -> Option<PopupHit> {
        if has_query && self.clear_rect.contains(px, py) {
            return Some(PopupHit::ClearSearch);
        }
        if self.search_rect.contains(px, py) {
            return Some(PopupHit::SearchInput);
        }
        if self.list_rect.contains(px, py) && self.total_rows > 0 {
            let rel_y = (py - self.list_rect.y) as u32;
            let row_offset = (rel_y / LANG_ROW_HEIGHT) as usize;
            let idx = list_scroll.saturating_add(row_offset);
            if idx < self.total_rows {
                return Some(PopupHit::LanguageRow(idx));
            }
        }
        if let Some(strip_rect) = self.recent_strip_rect()
            && strip_rect.contains(px, py)
        {
            let local_x = px - strip_rect.x;
            let local_y = py - strip_rect.y;
            for (idx, chip) in self.recent_strip.chips.iter().enumerate() {
                let chip_rect = LocalRect::new(chip.x, chip.y, chip.width, chip.height);
                if chip_rect.contains(local_x, local_y) {
                    return Some(PopupHit::RecentChip(idx));
                }
            }
        }
        None
    }
}

/// Calculate the screen-space rectangle of the language popup anchored cleanly above the
/// under-pill `EN ▾` chip (or below the underbar if near the top of the screen), clamped inside `area`.
#[must_use]
pub fn calc_lang_popup_rect(
    bar_rect: OverlayRect,
    lang_chip: LocalRect,
    popup_w: u32,
    popup_h: u32,
    area: WorkArea,
) -> OverlayRect {
    let pw = popup_w as i32;
    let ph = popup_h as i32;
    let chip_cx = bar_rect.x + lang_chip.x + (lang_chip.width as i32) / 2;
    let raw_x = chip_cx - pw / 2;

    let (min_x, max_x) = if area.width() >= pw + 2 * UNDERBAR_EDGE_MARGIN {
        (
            area.left + UNDERBAR_EDGE_MARGIN,
            area.right - UNDERBAR_EDGE_MARGIN - pw,
        )
    } else {
        (area.left, (area.right - pw).max(area.left))
    };
    let x = raw_x.clamp(min_x, max_x);

    let above_y = bar_rect.y - POPUP_GAP - ph;
    let below_y = bar_rect.y + bar_rect.height as i32 + POPUP_GAP;
    let min_y = area.top + UNDERBAR_EDGE_MARGIN;
    let max_y = (area.bottom - UNDERBAR_EDGE_MARGIN - ph).max(area.top);

    let y = if above_y >= min_y {
        above_y
    } else if below_y + ph <= area.bottom - UNDERBAR_EDGE_MARGIN {
        below_y
    } else {
        above_y.clamp(area.top, max_y)
    };

    OverlayRect {
        x,
        y,
        width: popup_w,
        height: popup_h,
    }
}

/// Interactive buttons on the floating failed-paste recovery card anchored at the pill position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryCardButton {
    PastePrevious,
    ShowText,
    Copy,
    Discard,
}

/// Computed local geometry of the floating failed-paste recovery card.
///
/// When `has_paste_target` is `true`, renders a primary `Paste to <App>` button alongside `Copy`
/// and `Discard` (`304×76`); otherwise renders `Show text`, `Copy`, and `Discard` (`264×76`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecoveryCardLayout {
    pub width: u32,
    pub height: u32,
    pub dot_cx: i32,
    pub dot_cy: i32,
    pub title_rect: LocalRect,
    pub has_paste_target: bool,
    pub paste_previous: LocalRect,
    pub show_text: LocalRect,
    pub copy: LocalRect,
    pub discard: LocalRect,
}

impl RecoveryCardLayout {
    /// Deterministic compact layout for the `264×76` floating failed-paste card (no previous app).
    #[must_use]
    pub const fn compute() -> Self {
        Self::compute_for(false)
    }

    /// Deterministic compact layout with or without a primary `Paste to <App>` target button.
    #[must_use]
    pub const fn compute_for(has_paste_target: bool) -> Self {
        let height = RECOVERY_CARD_HEIGHT;
        let pad_x = 11;
        let btn_y = 40;
        let btn_h = 26;
        let gap = 6;
        if has_paste_target {
            let width = 304;
            let paste_w = 144;
            let copy_w = 62;
            let discard_w = 64;
            let paste_previous = LocalRect::new(pad_x, btn_y, paste_w, btn_h);
            let copy = LocalRect::new(paste_previous.right() + gap, btn_y, copy_w, btn_h);
            let discard = LocalRect::new(copy.right() + gap, btn_y, discard_w, btn_h);
            Self {
                width,
                height,
                dot_cx: 17,
                dot_cy: 21,
                title_rect: LocalRect::new(26, 11, width - 36, 20),
                has_paste_target: true,
                paste_previous,
                show_text: LocalRect::new(0, 0, 0, 0),
                copy,
                discard,
            }
        } else {
            let width = RECOVERY_CARD_WIDTH;
            let show_w = 82;
            let copy_w = 74;
            let discard_w = 74;
            let show_text = LocalRect::new(pad_x, btn_y, show_w, btn_h);
            let copy = LocalRect::new(show_text.right() + gap, btn_y, copy_w, btn_h);
            let discard = LocalRect::new(copy.right() + gap, btn_y, discard_w, btn_h);
            Self {
                width,
                height,
                dot_cx: 17,
                dot_cy: 21,
                title_rect: LocalRect::new(26, 11, width - 36, 20),
                has_paste_target: false,
                paste_previous: LocalRect::new(0, 0, 0, 0),
                show_text,
                copy,
                discard,
            }
        }
    }

    /// Hit-test local coordinates `(px, py)` against the recovery card action buttons.
    #[must_use]
    pub const fn hit_test(&self, px: i32, py: i32) -> Option<RecoveryCardButton> {
        if self.has_paste_target && self.paste_previous.contains(px, py) {
            Some(RecoveryCardButton::PastePrevious)
        } else if !self.has_paste_target && self.show_text.contains(px, py) {
            Some(RecoveryCardButton::ShowText)
        } else if self.copy.contains(px, py) {
            Some(RecoveryCardButton::Copy)
        } else if self.discard.contains(px, py) {
            Some(RecoveryCardButton::Discard)
        } else {
            None
        }
    }
}

/// Format a compact primary button label for pasting into the previous external app
/// (e.g. `"Paste to Terminal"`, truncated cleanly to fit the `144px` button).
#[must_use]
pub fn format_paste_previous_button_label(app_name: &str) -> String {
    let clean = app_name.trim();
    if clean.is_empty() {
        return "Paste to previous".to_owned();
    }
    let mut short: String = clean.chars().take(12).collect();
    if clean.chars().count() > 12 {
        short.push('…');
    }
    format!("Paste to {short}")
}

/// Position the floating failed-paste recovery card centered at the pill's screen position,
/// clamped cleanly inside `area`.
#[must_use]
pub fn calc_recovery_card_rect(
    pill: OverlayRect,
    card_w: u32,
    card_h: u32,
    area: WorkArea,
) -> OverlayRect {
    let cw = card_w as i32;
    let ch = card_h as i32;
    let pill_cx = pill.x + (pill.width as i32) / 2;
    let pill_cy = pill.y + (pill.height as i32) / 2;
    let raw_x = pill_cx - cw / 2;
    let raw_y = pill_cy - ch / 2;

    let min_x = area.left + UNDERBAR_EDGE_MARGIN;
    let max_x = (area.right - UNDERBAR_EDGE_MARGIN - cw).max(min_x);
    let min_y = area.top + UNDERBAR_EDGE_MARGIN;
    let max_y = (area.bottom - UNDERBAR_EDGE_MARGIN - ch).max(min_y);

    OverlayRect {
        x: raw_x.clamp(min_x, max_x),
        y: raw_y.clamp(min_y, max_y),
        width: card_w,
        height: card_h,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    const AREA: WorkArea = WorkArea {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1040,
    };

    #[test]
    fn calc_underbar_rect_centers_below_clamps_and_flips_on_tight_bottom() {
        // Sufficient bottom clearance: centered 6px below pill.
        let pill = OverlayRect {
            x: 848,
            y: 900,
            width: 224,
            height: 41,
        };
        let layout = UnderbarLayout::compute(true);
        let placement = calc_underbar_placement(pill, layout.width, AREA);
        assert_eq!(placement.side, UnderbarSide::Below);
        assert_eq!(placement.bar_rect.height, UNDERBAR_HEIGHT);
        assert_eq!(placement.bar_rect.y, pill.y + pill.height as i32 + UNDERBAR_GAP);
        assert_eq!(
            placement.bar_rect.x,
            pill.x + (pill.width as i32) / 2 - (layout.width as i32) / 2
        );
        assert_eq!(placement.bridge_rect.y, pill.y + pill.height as i32);
        assert_eq!(placement.bridge_rect.height, UNDERBAR_GAP as u32);
        assert_eq!(
            calc_underbar_rect(pill, layout.width, AREA),
            placement.bar_rect
        );

        // Insufficient bottom clearance: flips cleanly 6px above pill.
        let low_pill = OverlayRect {
            x: 848,
            y: 990,
            width: 224,
            height: 41,
        };
        let flipped = calc_underbar_placement(low_pill, layout.width, AREA);
        assert_eq!(flipped.side, UnderbarSide::Above);
        assert_eq!(
            flipped.bar_rect.y,
            low_pill.y - UNDERBAR_GAP - UNDERBAR_HEIGHT as i32
        );
        assert_eq!(
            flipped.bridge_rect.y,
            flipped.bar_rect.y + UNDERBAR_HEIGHT as i32
        );
        assert_eq!(flipped.bridge_rect.height, UNDERBAR_GAP as u32);

        // Negative multi-monitor coordinates with edge clamping.
        let left_monitor = WorkArea {
            left: -3840,
            top: -100,
            right: -1920,
            bottom: 980,
        };
        let edge_pill = OverlayRect {
            x: -3835,
            y: 400,
            width: 140,
            height: 36,
        };
        let clamped = calc_underbar_rect(edge_pill, layout.width, left_monitor);
        assert_eq!(clamped.x, left_monitor.left + UNDERBAR_EDGE_MARGIN);
        assert!(clamped.x + clamped.width as i32 <= left_monitor.right - UNDERBAR_EDGE_MARGIN);
        assert_eq!(clamped.y, edge_pill.y + edge_pill.height as i32 + UNDERBAR_GAP);

        let right_pill = OverlayRect {
            x: -1990,
            y: 400,
            width: 140,
            height: 36,
        };
        let right_clamped = calc_underbar_rect(right_pill, layout.width, left_monitor);
        assert_eq!(
            right_clamped.x + right_clamped.width as i32,
            left_monitor.right - UNDERBAR_EDGE_MARGIN
        );
    }

    #[test]
    fn calc_panel_rect_with_underbar_avoids_overlap_on_top_and_flipped_bottom() {
        let pill_top = OverlayRect {
            x: 848,
            y: 24,
            width: 224,
            height: 41,
        };
        let bar_below = calc_underbar_rect(pill_top, 232, AREA);
        let panel_top = calc_panel_rect_with_underbar(
            "top-center",
            pill_top,
            Some(bar_below),
            360,
            48,
            AREA,
        );
        assert_eq!(panel_top.y, bar_below.y + bar_below.height as i32 + pure::GAP);

        let pill_bottom_low = OverlayRect {
            x: 848,
            y: 990,
            width: 224,
            height: 41,
        };
        let bar_above = calc_underbar_rect(pill_bottom_low, 232, AREA);
        assert!(bar_above.y + bar_above.height as i32 <= pill_bottom_low.y);
        let panel_bottom = calc_panel_rect_with_underbar(
            "bottom-center",
            pill_bottom_low,
            Some(bar_above),
            360,
            48,
            AREA,
        );
        assert_eq!(panel_bottom.y, bar_above.y - pure::GAP - 48);
    }

    #[test]
    fn hover_and_grace_visibility_state_machine() {
        let pill = OverlayRect {
            x: 848,
            y: 900,
            width: 224,
            height: 41,
        };
        let placement = calc_underbar_placement(pill, 232, AREA);

        // Bare pill starts hidden when unpinned.
        let mut vis = UnderbarVisibility::new(true, false);
        assert!(!vis.visible);
        assert!(!vis.tick(33, false));

        // Cursor over pill reveals bar.
        let in_pill = is_cursor_in_hover_zone(900, 920, pill, placement.bridge_rect, None, None);
        assert!(in_pill);
        assert!(vis.tick(33, in_pill));
        assert!(vis.visible);

        // Cursor leaves -> 250ms grace linger keeps bar visible until threshold reached.
        assert!(vis.tick(100, false));
        assert!(vis.tick(149, false));
        assert!(!vis.tick(1, false));
        assert!(!vis.visible);

        // Cursor over 6px bridge reveals bar.
        let in_bridge = is_cursor_in_hover_zone(
            900,
            pill.y + pill.height as i32 + 2,
            pill,
            placement.bridge_rect,
            None,
            None,
        );
        assert!(in_bridge);
        assert!(vis.tick(33, in_bridge));
        assert!(vis.visible);

        // Re-entering before 250ms resets linger countdown.
        assert!(vis.tick(200, false));
        assert!(vis.tick(33, true));
        assert!(vis.tick(200, false));
        assert!(!vis.tick(50, false));

        // Pinned locks bar visible even with cursor outside.
        vis.set_pinned(true);
        assert!(vis.visible);
        assert!(vis.tick(500, false));
        vis.set_pinned(false);
        assert!(!vis.tick(300, false));

        // Language menu open locks bar visible.
        vis.set_lang_menu_open(true);
        assert!(vis.visible);
        assert!(vis.tick(500, false));
        vis.set_lang_menu_open(false);
        assert!(!vis.tick(300, false));

        // Finishing ("Writing…") locks bar visible.
        vis.set_finishing(true);
        assert!(vis.visible);
        assert!(vis.tick(500, false));
        vis.set_finishing(false);
        assert!(!vis.tick(300, false));

        // Disabled controls never reveal.
        vis.set_controls_enabled(false);
        vis.set_pinned(true);
        assert!(!vis.tick(33, true));
    }

    #[test]
    fn recent_languages_chip_layout_0_1_2_3_and_5_with_wheel_stepping() {
        let row_w = 220;

        // 0 recents -> hidden.
        let r0 = calc_recent_strip_layout(&[], row_w, 0);
        assert_eq!(r0.height, 0);
        assert!(r0.chips.is_empty());
        assert!(!r0.scrollable);

        // 1 recent -> hidden.
        let r1 = calc_recent_strip_layout(&["en".to_owned()], row_w, 0);
        assert_eq!(r1.height, 0);
        assert!(r1.chips.is_empty());
        assert!(!r1.scrollable);

        // Duplicate entries collapse to 1 -> hidden.
        let r1_dup = calc_recent_strip_layout(&["en".to_owned(), "EN".to_owned()], row_w, 0);
        assert_eq!(r1_dup.height, 0);
        assert!(r1_dup.chips.is_empty());

        // 2 recents -> 2 equal ~50% width chips, non-scrollable.
        let r2 = calc_recent_strip_layout(&["en".to_owned(), "bg".to_owned()], row_w, 0);
        assert_eq!(r2.height, RECENT_STRIP_HEIGHT);
        assert_eq!(r2.chips.len(), 2);
        assert!(!r2.scrollable);
        assert_eq!(r2.chips[0].width, 108);
        assert_eq!(r2.chips[1].width, 108);
        assert_eq!(
            r2.chips[1].x + r2.chips[1].width as i32,
            row_w as i32,
            "2 chips fill full inner width"
        );

        // 3 recents -> 3 equal ~33.3% width chips, non-scrollable.
        let r3 = calc_recent_strip_layout(
            &["en".to_owned(), "de".to_owned(), "bg".to_owned()],
            row_w,
            0,
        );
        assert_eq!(r3.height, RECENT_STRIP_HEIGHT);
        assert_eq!(r3.chips.len(), 3);
        assert!(!r3.scrollable);
        for chip in &r3.chips {
            assert!((70..=71).contains(&chip.width));
        }
        assert_eq!(
            r3.chips[2].x + r3.chips[2].width as i32,
            row_w as i32,
            "3 chips fill full inner width"
        );

        // 5 recents -> ~3.5 chips fit across row_w, 4th chip peeks at right edge, wheel steps 1 chip per notch.
        let five: Vec<String> = ["en", "de", "bg", "fr", "es"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        let r5 = calc_recent_strip_layout(&five, row_w, 0);
        assert_eq!(r5.height, RECENT_STRIP_HEIGHT);
        assert_eq!(r5.chips.len(), 5);
        assert!(r5.scrollable);
        assert_eq!(r5.max_scroll, 2);
        assert!(!r5.show_left_fade);
        assert!(r5.show_right_fade);
        // First 3 chips fully visible inside 0..220; 4th chip partially peeking at right edge.
        assert!(r5.chips[2].x + r5.chips[2].width as i32 <= row_w as i32);
        assert!(r5.chips[3].x < row_w as i32);
        assert!(r5.chips[3].x + r5.chips[3].width as i32 > row_w as i32);
        // Verify ~3.5 chips fit across row_w.
        let visible_chips =
            (row_w as f32 + RECENT_CHIP_GAP as f32) / (r5.chip_width as f32 + RECENT_CHIP_GAP as f32);
        assert!(
            (3.4..=3.7).contains(&visible_chips),
            "expected ~3.5 chips across row, got {visible_chips}"
        );

        // 1-chip-per-wheel-notch stepping clamps cleanly in 0..=max_scroll.
        let s0 = 0;
        let s_under = step_recent_scroll(s0, wheel_delta_to_notches(120), r5.max_scroll);
        assert_eq!(s_under, 0);
        let s1 = step_recent_scroll(s0, wheel_delta_to_notches(-120), r5.max_scroll);
        assert_eq!(s1, 1);
        let s2 = step_recent_scroll(s1, wheel_delta_to_notches(-120), r5.max_scroll);
        assert_eq!(s2, 2);
        let s_over = step_recent_scroll(s2, wheel_delta_to_notches(-120), r5.max_scroll);
        assert_eq!(s_over, 2);

        let r5_end = calc_recent_strip_layout(&five, row_w, s2);
        assert!(r5_end.show_left_fade);
        assert!(!r5_end.show_right_fade);
        assert_eq!(
            r5_end.chips[4].x + r5_end.chips[4].width as i32,
            row_w as i32,
            "last chip aligns flush with right edge at max_scroll"
        );
    }

    #[test]
    fn filter_languages_covers_all_99_and_matches_code_and_name() {
        let all = filter_languages("");
        assert_eq!(all.len(), 99);
        assert_eq!(filter_languages("   ").len(), 99);

        let by_code = filter_languages("bg");
        assert!(!by_code.is_empty());
        assert_eq!(by_code[0].code, "bg");
        assert_eq!(by_code[0].name, "Bulgarian");

        let by_name = filter_languages("Bulgarian");
        assert_eq!(by_name.len(), 1);
        assert_eq!(by_name[0].code, "bg");

        let by_substr = filter_languages("BULGAR");
        assert_eq!(by_substr.len(), 1);
        assert_eq!(by_substr[0].code, "bg");

        let garia = filter_languages("GARIA");
        assert!(garia.iter().any(|e| e.code == "bg"));
        assert!(garia.iter().any(|e| e.code == "hu"));

        let maori = filter_languages("maori");
        assert_eq!(maori.len(), 1);
        assert_eq!(maori[0].code, "mi");

        assert!(filter_languages("zzzz-nonexistent").is_empty());
    }

    #[test]
    fn underbar_hit_testing_and_elapsed_timer_formatting() {
        let layout = UnderbarLayout::compute(true);

        let center = |r: LocalRect| (r.x + (r.width as i32) / 2, r.y + (r.height as i32) / 2);

        let (cx, cy) = center(layout.cancel);
        assert_eq!(layout.hit_test(cx, cy), Some(UnderbarControl::Cancel));

        let (lx, ly) = center(layout.language);
        assert_eq!(layout.hit_test(lx, ly), Some(UnderbarControl::Language));

        let (dx, dy) = center(layout.delivery_mode);
        assert_eq!(layout.hit_test(dx, dy), Some(UnderbarControl::DeliveryMode));

        let (px, py) = center(layout.pin);
        assert_eq!(layout.hit_test(px, py), Some(UnderbarControl::Pin));

        let (ox, oy) = center(layout.done);
        assert_eq!(layout.hit_test(ox, oy), Some(UnderbarControl::Done));

        // Timer area is display-only (not an interactive button).
        let (tx, ty) = center(layout.timer);
        assert_eq!(layout.hit_test(tx, ty), None);

        assert_eq!(format_elapsed(0), "0:00");
        assert_eq!(format_elapsed(14), "0:14");
        assert_eq!(format_elapsed(65), "1:05");
        assert_eq!(format_elapsed(754), "12:34");
    }

    #[test]
    fn lang_popup_layout_hit_testing_and_screen_clamping() {
        let recents = vec!["en".to_owned(), "de".to_owned(), "bg".to_owned()];
        let popup = LangPopupLayout::compute(99, &recents, 0);
        assert_eq!(popup.visible_rows, LANG_VISIBLE_ROWS);
        assert_eq!(popup.max_list_scroll(), 99 - LANG_VISIBLE_ROWS);

        assert_eq!(
            popup.hit_test(popup.clear_rect.x + 5, popup.clear_rect.y + 5, true, 0),
            Some(PopupHit::ClearSearch)
        );
        assert_eq!(
            popup.hit_test(popup.search_rect.x + 10, popup.search_rect.y + 10, false, 0),
            Some(PopupHit::SearchInput)
        );
        assert_eq!(
            popup.hit_test(
                popup.list_rect.x + 20,
                popup.list_rect.y + (LANG_ROW_HEIGHT as i32) * 2 + 5,
                false,
                5,
            ),
            Some(PopupHit::LanguageRow(7))
        );
        let strip_y = popup.recent_strip_y + popup.recent_strip.chips[1].y + 5;
        let strip_x = POPUP_PADDING_X + popup.recent_strip.chips[1].x + 10;
        assert_eq!(
            popup.hit_test(strip_x, strip_y, false, 0),
            Some(PopupHit::RecentChip(1))
        );

        // Anchoring near bottom -> opens above underbar.
        let bar_layout = UnderbarLayout::compute(true);
        let bar_bottom = OverlayRect {
            x: 844,
            y: 960,
            width: bar_layout.width,
            height: UNDERBAR_HEIGHT,
        };
        let pr_above = calc_lang_popup_rect(
            bar_bottom,
            bar_layout.language,
            popup.width,
            popup.height,
            AREA,
        );
        assert_eq!(pr_above.y, bar_bottom.y - POPUP_GAP - popup.height as i32);

        // Anchoring near top -> flips below underbar cleanly.
        let bar_top = OverlayRect {
            x: 844,
            y: 71,
            width: bar_layout.width,
            height: UNDERBAR_HEIGHT,
        };
        let pr_below = calc_lang_popup_rect(
            bar_top,
            bar_layout.language,
            popup.width,
            popup.height,
            AREA,
        );
        assert_eq!(pr_below.y, bar_top.y + UNDERBAR_HEIGHT as i32 + POPUP_GAP);
    }

    #[test]
    fn compact_underbar_narrower_than_pill_and_recovery_card_hits_all_buttons() {
        let layout = UnderbarLayout::compute(true);
        // At default 80% scale, pill width is ~224px; under-pill bar must stay narrower.
        assert!(
            layout.width < 200,
            "under-pill bar ({}px) should stay narrower than pill",
            layout.width
        );

        let rec = RecoveryCardLayout::compute();
        assert_eq!(rec.width, RECOVERY_CARD_WIDTH);
        assert_eq!(rec.height, RECOVERY_CARD_HEIGHT);
        assert_eq!(
            rec.hit_test(rec.show_text.x + 5, rec.show_text.y + 5),
            Some(RecoveryCardButton::ShowText)
        );
        assert_eq!(
            rec.hit_test(rec.copy.x + 5, rec.copy.y + 5),
            Some(RecoveryCardButton::Copy)
        );
        assert_eq!(
            rec.hit_test(rec.discard.x + 5, rec.discard.y + 5),
            Some(RecoveryCardButton::Discard)
        );
        assert_eq!(rec.hit_test(4, 4), None);

        let pill = OverlayRect {
            x: 848,
            y: 960,
            width: 224,
            height: 45,
        };
        let card_rect = calc_recovery_card_rect(pill, rec.width, rec.height, AREA);
        assert_eq!(card_rect.width, RECOVERY_CARD_WIDTH);
        assert_eq!(card_rect.height, RECOVERY_CARD_HEIGHT);
        assert!(card_rect.x >= AREA.left && card_rect.x + card_rect.width as i32 <= AREA.right);
        assert!(card_rect.y >= AREA.top && card_rect.y + card_rect.height as i32 <= AREA.bottom);

        let rec_paste = RecoveryCardLayout::compute_for(true);
        assert!(rec_paste.has_paste_target);
        assert_eq!(
            rec_paste.hit_test(
                rec_paste.paste_previous.x + 5,
                rec_paste.paste_previous.y + 5
            ),
            Some(RecoveryCardButton::PastePrevious)
        );
        assert_eq!(
            rec_paste.hit_test(rec_paste.copy.x + 5, rec_paste.copy.y + 5),
            Some(RecoveryCardButton::Copy)
        );
        assert_eq!(
            rec_paste.hit_test(rec_paste.discard.x + 5, rec_paste.discard.y + 5),
            Some(RecoveryCardButton::Discard)
        );
        assert_eq!(
            format_paste_previous_button_label("Terminal"),
            "Paste to Terminal"
        );
        assert_eq!(
            format_paste_previous_button_label(""),
            "Paste to previous"
        );
    }
}
