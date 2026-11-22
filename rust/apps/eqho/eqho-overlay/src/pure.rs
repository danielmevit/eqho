//! Cross-platform pure math, layout, and animation state for Eqho's overlay.

use damt_platform::ScreenRect;
use eqho_core::pillfx::VoiceFollower;
use eqho_core::settings::Settings;

/// Distance in pixels from the work-area edge.
pub const MARGIN: i32 = 24;
/// Vertical gap in pixels between the pill and the transcription panel.
pub const GAP: i32 = 10;
/// Maximum width in pixels of the transcription panel.
pub const PANEL_MAX_W: u32 = 640;
/// Horizontal padding in pixels inside the transcription panel.
pub const PANEL_PADDING_X: i32 = 18;
/// Vertical padding in pixels inside the transcription panel.
pub const PANEL_PADDING_Y: i32 = 12;
/// Maximum number of characters shown in the transcription panel tail.
pub const MAX_TEXT_CHARS: usize = 220;
/// Placeholder text shown while listening before real words arrive.
pub const PLACEHOLDER: &str = "Listening...";
/// Additional alpha multiplier applied to the pill in light theme.
pub const LIGHT_PILL_ALPHA: f32 = 0.90;
/// Number of discrete fade steps (matches `src/overlay.py`).
pub const FADE_STEPS: u32 = 5;
/// Milliseconds per fade step (matches `src/overlay.py`).
pub const FADE_INTERVAL_MS: u64 = 25;
/// Target animation frame interval in milliseconds (~30 fps).
pub const FRAME_MS: u64 = 33;

/// Runtime configuration for the floating overlay.
#[derive(Debug, Clone, PartialEq)]
pub struct OverlayConfig {
    pub enabled: bool,
    pub show_text: bool,
    pub monitor: String,
    pub position: String,
    pub pill_scale: f64,
    pub pill_opacity: f64,
    pub panel_opacity: f64,
    pub font_size: i32,
    pub show_level: bool,
    pub dark_theme: bool,
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            show_text: true,
            monitor: "active".to_owned(),
            position: "bottom-center".to_owned(),
            pill_scale: 0.8,
            pill_opacity: 0.85,
            panel_opacity: 0.85,
            font_size: 14,
            show_level: true,
            dark_theme: true,
        }
    }
}

impl OverlayConfig {
    /// Build an [`OverlayConfig`] from Eqho [`Settings`] and the resolved theme polarity.
    #[must_use]
    pub fn from_settings(settings: &Settings, resolved_dark: bool) -> Self {
        let pill_scale = if settings.overlay_pill_scale.is_finite() {
            settings.overlay_pill_scale.clamp(0.6, 1.4)
        } else {
            0.8
        };
        let pill_opacity = if settings.overlay_opacity.is_finite() {
            settings.overlay_opacity.clamp(0.3, 1.0)
        } else {
            0.85
        };
        let panel_opacity = if settings.overlay_panel_opacity.is_finite() {
            settings.overlay_panel_opacity.clamp(0.3, 1.0)
        } else {
            0.85
        };
        let monitor = match settings.overlay_monitor.trim().to_ascii_lowercase().as_str() {
            "cursor" => "cursor".to_owned(),
            "primary" => "primary".to_owned(),
            _ => "active".to_owned(),
        };
        Self {
            enabled: settings.overlay_enabled,
            show_text: settings.overlay_show_text,
            monitor,
            position: settings.overlay_position.clone(),
            pill_scale,
            pill_opacity,
            panel_opacity,
            font_size: settings.overlay_font_size.clamp(10, 28),
            show_level: settings.overlay_show_level,
            dark_theme: resolved_dark,
        }
    }
}

/// Monitor work-area bounds (excluding taskbar/dock) in physical screen pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkArea {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl WorkArea {
    /// Width of the work area in pixels (clamped to >= 0).
    #[must_use]
    pub fn width(self) -> i32 {
        (self.right - self.left).max(0)
    }

    /// Height of the work area in pixels (clamped to >= 0).
    #[must_use]
    pub fn height(self) -> i32 {
        (self.bottom - self.top).max(0)
    }
}

impl From<ScreenRect> for WorkArea {
    fn from(r: ScreenRect) -> Self {
        Self {
            left: r.x,
            top: r.y,
            right: r.x + r.width,
            bottom: r.y + r.height,
        }
    }
}

/// Screen-space rectangle for the pill or transcription panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlayRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Calculate the top-left `(x, y)` position of the pill for `position` within `area`.
///
/// Matches `src/overlay.py::_calc_position` across all 6 anchor strings, defaulting
/// unknown values to `"bottom-center"`.
#[must_use]
pub fn calc_pill_position(position: &str, pill_w: u32, pill_h: u32, area: WorkArea) -> (i32, i32) {
    let w = pill_w as i32;
    let h = pill_h as i32;
    let cx = area.left + (area.right - area.left - w).div_euclid(2);
    match position {
        "top-center" => (cx, area.top + MARGIN),
        "top-left" => (area.left + MARGIN, area.top + MARGIN),
        "top-right" => (area.right - w - MARGIN, area.top + MARGIN),
        "bottom-left" => (area.left + MARGIN, area.bottom - h - MARGIN),
        "bottom-right" => (area.right - w - MARGIN, area.bottom - h - MARGIN),
        _ => (cx, area.bottom - h - MARGIN),
    }
}

/// Return `true` when the overlay is anchored to the top edge of the work area.
#[must_use]
pub fn is_anchored_top(position: &str) -> bool {
    position.starts_with("top")
}

/// Calculate the transcription panel rectangle relative to the pill and work area.
///
/// Horizontally centered on the pill and clamped within `[area.left + 8, area.right - 8 - panel_w]`.
/// Vertically placed `GAP` below the pill for top anchors, or `GAP` above the pill
/// (`pill.y - GAP - panel_h`) for bottom anchors so extra lines expand upward.
#[must_use]
pub fn calc_panel_rect(
    position: &str,
    pill: OverlayRect,
    panel_w: u32,
    panel_h: u32,
    area: WorkArea,
) -> OverlayRect {
    let pw = panel_w as i32;
    let ph = panel_h as i32;
    let pill_cx = pill.x + (pill.width as i32) / 2;
    let raw_x = pill_cx - pw / 2;
    let min_x = area.left + 8;
    let max_x = (area.right - 8 - pw).max(min_x);
    let x = raw_x.clamp(min_x, max_x);
    let y = if is_anchored_top(position) {
        pill.y + pill.height as i32 + GAP
    } else {
        pill.y - GAP - ph
    };
    OverlayRect {
        x,
        y,
        width: panel_w,
        height: panel_h,
    }
}

/// UTF-8 char-boundary-safe truncation to [`MAX_TEXT_CHARS`] characters with leading `'…'`.
#[must_use]
pub fn tail_text(text: &str) -> String {
    let count = text.chars().count();
    if count <= MAX_TEXT_CHARS {
        return text.to_owned();
    }
    let skip = count - (MAX_TEXT_CHARS - 1);
    let byte_idx = text
        .char_indices()
        .nth(skip)
        .map_or(text.len(), |(idx, _)| idx);
    let mut out = String::with_capacity(3 + text.len() - byte_idx);
    out.push('…');
    out.push_str(&text[byte_idx..]);
    out
}

/// Return `true` when `text` contains real words (not empty/whitespace or [`PLACEHOLDER`]).
#[must_use]
pub fn should_show_panel(text: &str) -> bool {
    let trimmed = text.trim();
    !trimmed.is_empty() && trimmed != PLACEHOLDER
}

/// Per-frame animation inputs produced by [`PillAnimator::step`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameParams {
    pub phase: f32,
    pub level: f32,
    pub wave: Option<(f32, f32, f32)>,
}

/// Syllable-following pill animation and fade state.
#[derive(Debug, Clone)]
pub struct PillAnimator {
    pub phase: f32,
    pub level_raw: f32,
    pub lvl_fast: f32,
    pub lvl_slow: f32,
    pub voice: VoiceFollower,
    pub voice_seen: bool,
    pub alpha_logical: f32,
    pub fade_target: f32,
}

impl Default for PillAnimator {
    fn default() -> Self {
        Self::new()
    }
}

impl PillAnimator {
    /// Create a new animator in the hidden, zeroed state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            phase: 0.0,
            level_raw: 0.0,
            lvl_fast: 0.0,
            lvl_slow: 0.0,
            voice: VoiceFollower::default(),
            voice_seen: false,
            alpha_logical: 0.0,
            fade_target: 0.0,
        }
    }

    /// Reset phase, audio level followers, voice waves, and fade state.
    pub fn reset(&mut self) {
        self.phase = 0.0;
        self.level_raw = 0.0;
        self.lvl_fast = 0.0;
        self.lvl_slow = 0.0;
        self.voice.reset();
        self.voice_seen = false;
        self.alpha_logical = 0.0;
        self.fade_target = 0.0;
    }

    /// Update the raw mic level in `0.0..=1.0`.
    pub fn set_level(&mut self, level: f32) {
        self.level_raw = if level.is_finite() {
            level.clamp(0.0, 1.0)
        } else {
            0.0
        };
    }

    /// Feed live voice waveform features `(loud, tone)`.
    pub fn feed_voice(&mut self, loud: f32, tone: f32) {
        let l = if loud.is_finite() {
            loud.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let t = if tone.is_finite() {
            tone.clamp(0.0, 1.0)
        } else {
            0.5
        };
        self.voice.feed(l, t);
        self.voice_seen = true;
    }

    /// Advance the syllable follower, phase integration, and voice follower by `dt_s`.
    pub fn step(&mut self, dt_s: f32, show_level: bool) -> FrameParams {
        let dt = if dt_s.is_finite() {
            dt_s.clamp(0.0, 0.1)
        } else {
            0.0
        };
        let raw = if show_level { self.level_raw } else { 0.0 };
        let k_fast = if raw > self.lvl_fast { 0.6 } else { 0.3 };
        self.lvl_fast += (raw - self.lvl_fast) * k_fast;
        self.lvl_slow += (self.lvl_fast - self.lvl_slow) * 0.06;
        let dyn_lvl = ((self.lvl_fast - self.lvl_slow) * 3.0).clamp(0.0, 1.0);
        let base = ((self.lvl_fast - 0.35) * 1.1).clamp(0.0, 1.0);
        let level = (base * 0.55 + dyn_lvl * 0.6).clamp(0.0, 1.0);
        self.phase += dt * (1.0 + 0.9 * level);

        let wave = if self.voice_seen && show_level {
            Some(self.voice.step(dt))
        } else {
            None
        };

        FrameParams {
            phase: self.phase,
            level,
            wave,
        }
    }

    /// Advance logical fade alpha toward `fade_target` over `FADE_STEPS * FADE_INTERVAL_MS`.
    pub fn step_fade(&mut self, dt_s: f32) -> f32 {
        let dt = if dt_s.is_finite() {
            dt_s.max(0.0)
        } else {
            0.0
        };
        let fade_dur = (FADE_STEPS as f32) * (FADE_INTERVAL_MS as f32) / 1000.0;
        let max_delta = if fade_dur > 0.0 { dt / fade_dur } else { 1.0 };
        let diff = self.fade_target - self.alpha_logical;
        if diff.abs() <= max_delta {
            self.alpha_logical = self.fade_target;
        } else {
            self.alpha_logical += diff.signum() * max_delta;
        }
        self.alpha_logical = self.alpha_logical.clamp(0.0, 1.0);
        self.alpha_logical
    }

    /// Compute the effective 0..=255 pill alpha (applies `LIGHT_PILL_ALPHA` in light theme).
    #[must_use]
    pub fn effective_pill_alpha(&self, cfg: &OverlayConfig) -> u8 {
        let theme_scale = if cfg.dark_theme { 1.0 } else { LIGHT_PILL_ALPHA };
        let a = (self.alpha_logical * (cfg.pill_opacity as f32) * theme_scale).clamp(0.0, 1.0);
        (a * 255.0).round() as u8
    }

    /// Compute the effective 0..=255 panel alpha.
    #[must_use]
    pub fn effective_panel_alpha(&self, cfg: &OverlayConfig) -> u8 {
        let a = (self.alpha_logical * (cfg.panel_opacity as f32)).clamp(0.0, 1.0);
        (a * 255.0).round() as u8
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
    fn calc_pill_position_all_six_anchors() {
        let w = 224;
        let h = 41;
        let cx = (1920 - 224) / 2;

        assert_eq!(
            calc_pill_position("bottom-center", w, h, AREA),
            (cx, 1040 - 41 - MARGIN)
        );
        assert_eq!(
            calc_pill_position("top-center", w, h, AREA),
            (cx, MARGIN)
        );
        assert_eq!(
            calc_pill_position("top-left", w, h, AREA),
            (MARGIN, MARGIN)
        );
        assert_eq!(
            calc_pill_position("top-right", w, h, AREA),
            (1920 - 224 - MARGIN, MARGIN)
        );
        assert_eq!(
            calc_pill_position("bottom-left", w, h, AREA),
            (MARGIN, 1040 - 41 - MARGIN)
        );
        assert_eq!(
            calc_pill_position("bottom-right", w, h, AREA),
            (1920 - 224 - MARGIN, 1040 - 41 - MARGIN)
        );
    }

    #[test]
    fn calc_panel_rect_upward_growth_and_clamping() {
        let pill_bottom = OverlayRect {
            x: 848,
            y: 975,
            width: 224,
            height: 41,
        };
        let r1 = calc_panel_rect("bottom-center", pill_bottom, 300, 44, AREA);
        let r2 = calc_panel_rect("bottom-center", pill_bottom, 300, 88, AREA);
        assert_eq!(r1.y, 975 - GAP - 44);
        assert_eq!(r2.y, 975 - GAP - 88);
        assert!(r2.y < r1.y, "growing panel must expand upward on bottom anchor");

        let pill_top = OverlayRect {
            x: 848,
            y: 24,
            width: 224,
            height: 41,
        };
        let t1 = calc_panel_rect("top-center", pill_top, 300, 44, AREA);
        let t2 = calc_panel_rect("top-center", pill_top, 300, 88, AREA);
        assert_eq!(t1.y, 24 + 41 + GAP);
        assert_eq!(t2.y, t1.y, "top anchor keeps panel.y fixed below pill");

        // Horizontal clamping on left and right edges.
        let pill_left = OverlayRect {
            x: 24,
            y: 975,
            width: 224,
            height: 41,
        };
        let cl = calc_panel_rect("bottom-left", pill_left, 500, 50, AREA);
        assert_eq!(cl.x, AREA.left + 8);

        let pill_right = OverlayRect {
            x: 1920 - 224 - 24,
            y: 975,
            width: 224,
            height: 41,
        };
        let cr = calc_panel_rect("bottom-right", pill_right, 500, 50, AREA);
        assert_eq!(cr.x, AREA.right - 8 - 500);
    }

    #[test]
    fn tail_text_is_utf8_safe_with_multibyte_chars() {
        let short = "Здравей 🌍";
        assert_eq!(tail_text(short), short);

        // Mix Bulgarian (2-byte), CJK (3-byte), and emoji (4-byte) > 220 chars.
        let unit = "Дума日本🚀";
        let long: String = std::iter::repeat_n(unit, 40).collect(); // 280 chars
        assert_eq!(long.chars().count(), 280);

        let tailed = tail_text(&long);
        assert_eq!(tailed.chars().count(), MAX_TEXT_CHARS);
        assert!(tailed.starts_with('…'));
        assert!(tailed.ends_with("🚀"));
    }

    #[test]
    fn should_show_panel_filters_placeholder_and_blank() {
        assert!(!should_show_panel(""));
        assert!(!should_show_panel("   \n\t "));
        assert!(!should_show_panel("Listening..."));
        assert!(!should_show_panel("  Listening...  "));

        assert!(should_show_panel("Loading model…"));
        assert!(should_show_panel("Writing it down…"));
        assert!(should_show_panel("Hello world"));
    }

    #[test]
    fn pill_animator_syllable_phase_alpha_and_fade() {
        let mut anim = PillAnimator::new();
        anim.set_level(0.9);
        anim.feed_voice(0.8, 0.6);

        let f1 = anim.step(0.033, true);
        let f2 = anim.step(0.033, true);
        assert!(f1.level > 0.0);
        assert!(f2.phase > f1.phase, "phase must increase monotonically");
        assert!(f2.wave.is_some());

        // Fade 0 -> 1 -> 0 bounds.
        anim.fade_target = 1.0;
        for _ in 0..10 {
            let a = anim.step_fade(0.025);
            assert!((0.0..=1.0).contains(&a));
        }
        assert!((anim.alpha_logical - 1.0).abs() < 1e-6);

        let dark_cfg = OverlayConfig {
            dark_theme: true,
            pill_opacity: 1.0,
            ..OverlayConfig::default()
        };
        let light_cfg = OverlayConfig {
            dark_theme: false,
            pill_opacity: 1.0,
            ..OverlayConfig::default()
        };
        assert_eq!(anim.effective_pill_alpha(&dark_cfg), 255);
        assert_eq!(
            anim.effective_pill_alpha(&light_cfg),
            (LIGHT_PILL_ALPHA * 255.0).round() as u8
        );

        anim.fade_target = 0.0;
        for _ in 0..10 {
            let a = anim.step_fade(0.025);
            assert!((0.0..=1.0).contains(&a));
        }
        assert!((anim.alpha_logical - 0.0).abs() < 1e-6);
    }

    #[test]
    fn overlay_config_from_settings_clamps_and_maps() {
        let s = Settings {
            overlay_enabled: false,
            overlay_position: "top-right".to_owned(),
            overlay_pill_scale: 5.0,
            overlay_opacity: 0.1,
            overlay_panel_opacity: 2.0,
            overlay_font_size: 99,
            overlay_show_level: false,
            ..Settings::default()
        };

        let cfg = OverlayConfig::from_settings(&s, false);
        assert!(!cfg.enabled);
        assert_eq!(cfg.position, "top-right");
        assert!((cfg.pill_scale - 1.4).abs() < 1e-9);
        assert!((cfg.pill_opacity - 0.3).abs() < 1e-9);
        assert!((cfg.panel_opacity - 1.0).abs() < 1e-9);
        assert_eq!(cfg.font_size, 28);
        assert!(!cfg.show_level);
        assert!(!cfg.dark_theme);
        assert_eq!(cfg.monitor, "active");
        assert!(cfg.show_text);
    }

    #[test]
    fn multi_monitor_negative_work_area_places_pill_and_panel_inside_active_display() {
        let left_monitor = WorkArea {
            left: -3840,
            top: 0,
            right: -1920,
            bottom: 1152,
        };
        let (px, py) = calc_pill_position("bottom-center", 224, 41, left_monitor);
        assert!(px >= left_monitor.left && px + 224 <= left_monitor.right);
        assert_eq!(px, -3840 + (1920 - 224) / 2);
        assert_eq!(py, 1152 - 41 - MARGIN);

        let pill = OverlayRect {
            x: px,
            y: py,
            width: 224,
            height: 41,
        };
        let panel = calc_panel_rect("bottom-center", pill, 420, 48, left_monitor);
        assert!(panel.x >= left_monitor.left + 8);
        assert!(panel.x + 420 <= left_monitor.right - 8);
        assert_eq!(panel.y, py - GAP - 48);
    }
}
