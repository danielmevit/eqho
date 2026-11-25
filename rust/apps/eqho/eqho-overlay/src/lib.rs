//! Eqho floating iridescent pill, hover-reveal under-pill action strip, searchable 99-language
//! menu, and live transcription panel overlay.
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![warn(missing_debug_implementations)]

#[cfg_attr(not(windows), allow(dead_code))]
mod canvas;
pub mod pure;
pub mod underbar;
#[cfg(windows)]
mod win;

#[cfg(not(windows))]
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub use pure::{
    FADE_INTERVAL_MS, FADE_STEPS, FRAME_MS, FrameParams, GAP, LIGHT_PILL_ALPHA, MARGIN,
    MAX_TEXT_CHARS, OverlayConfig, OverlayRect, PANEL_MAX_W, PANEL_PADDING_X, PANEL_PADDING_Y,
    PLACEHOLDER, PillAnimator, WorkArea, calc_panel_rect, calc_pill_position, is_anchored_top,
    should_show_panel, tail_text,
};
pub use underbar::{
    FINISHING_LABEL, LANG_ROW_HEIGHT, LANG_VISIBLE_ROWS, LangPopupLayout, LocalRect,
    POPUP_BOTTOM_PAD, POPUP_GAP, POPUP_PADDING_TOP, POPUP_PADDING_X, POPUP_SEARCH_GAP,
    POPUP_SEARCH_HEIGHT, POPUP_WIDTH, PopupHit, RECENT_CHIP_GAP, RECENT_CHIP_HEIGHT,
    RECENT_FADE_WIDTH, RECENT_STRIP_HEIGHT, RECOVERY_CARD_HEIGHT, RECOVERY_CARD_RADIUS,
    RECOVERY_CARD_WIDTH, RecentChip, RecentStripLayout, RecoveryCardButton, RecoveryCardLayout,
    Rgba, UNDERBAR_BOTTOM_RESERVE, UNDERBAR_EDGE_MARGIN, UNDERBAR_GAP, UNDERBAR_HEIGHT,
    UNDERBAR_HOVER_ALPHA_RATIO, UNDERBAR_LINGER_MS, UNDERBAR_RADIUS, UNDERBAR_REST_ALPHA_RATIO,
    UnderbarControl, UnderbarLayout, UnderbarPlacement, UnderbarSide, UnderbarTheme,
    UnderbarVisibility, calc_lang_popup_rect, calc_panel_rect_with_underbar,
    calc_pill_position_with_controls, calc_recent_strip_layout, calc_recovery_card_rect,
    calc_underbar_placement, calc_underbar_rect, delivery_mode_label, filter_languages,
    format_elapsed, format_language_code, is_cursor_in_hover_zone, next_delivery_mode,
    normalize_delivery_mode, point_in_overlay_rect, sanitize_recent_languages, step_recent_scroll,
    wheel_delta_to_notches,
};

/// User actions emitted from the floating under-pill control bar, language menu, and
/// failed-paste recovery card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverlayAction {
    Cancel,
    Done,
    TogglePin(bool),
    ToggleDeliveryMode(String),
    SelectLanguage(String),
    RecoveryShowText,
    RecoveryCopy,
    RecoveryDiscard,
}

/// Callback invoked when the user clicks an interactive control in the overlay.
pub type OverlayActionCallback = Arc<dyn Fn(OverlayAction) + Send + Sync>;

/// Optional provider returning straight RGBA8 `(width, height, bytes)` flag bitmaps for language codes.
pub type FlagBitmapProvider = Arc<dyn Fn(&str) -> Option<(u32, u32, Vec<u8>)> + Send + Sync>;

#[cfg(not(windows))]
struct NonWinState {
    config: OverlayConfig,
    text: String,
    animator: PillAnimator,
    finishing: bool,
    visibility: UnderbarVisibility,
    on_action: Option<OverlayActionCallback>,
    flag_provider: Option<FlagBitmapProvider>,
}

#[cfg(not(windows))]
impl fmt::Debug for NonWinState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NonWinState")
            .field("config", &self.config)
            .field("text", &self.text)
            .field("animator", &self.animator)
            .field("finishing", &self.finishing)
            .field("visibility", &self.visibility)
            .field("has_on_action", &self.on_action.is_some())
            .field("has_flag_provider", &self.flag_provider.is_some())
            .finish()
    }
}

/// Thread-safe handle to Eqho's floating dictation overlay.
#[derive(Debug)]
pub struct OverlayHandle {
    visible: Arc<AtomicBool>,
    enabled: Arc<AtomicBool>,
    #[cfg(windows)]
    client: Mutex<Option<win::WinOverlayClient>>,
    #[cfg(not(windows))]
    state: Mutex<NonWinState>,
}

impl OverlayHandle {
    /// Start the overlay backend (dedicated Win32 thread on Windows; state recorder elsewhere).
    #[must_use]
    pub fn start(config: OverlayConfig) -> Self {
        Self::start_with_callbacks(config, None, None)
    }

    /// Start the overlay backend with optional interactive action and flag bitmap callbacks.
    #[must_use]
    pub fn start_with_callbacks(
        config: OverlayConfig,
        on_action: Option<OverlayActionCallback>,
        flag_provider: Option<FlagBitmapProvider>,
    ) -> Self {
        let enabled = Arc::new(AtomicBool::new(config.enabled));
        let visible = Arc::new(AtomicBool::new(false));
        #[cfg(windows)]
        {
            let client = win::WinOverlayClient::start(config, on_action, flag_provider);
            Self {
                visible,
                enabled,
                client: Mutex::new(client),
            }
        }
        #[cfg(not(windows))]
        {
            let visibility = UnderbarVisibility::new(config.controls_enabled, config.controls_pinned);
            Self {
                visible,
                enabled,
                state: Mutex::new(NonWinState {
                    config,
                    text: String::new(),
                    animator: PillAnimator::new(),
                    finishing: false,
                    visibility,
                    on_action,
                    flag_provider,
                }),
            }
        }
    }

    /// Show the pill (and transcription panel if `text` has real words).
    pub fn show(&self, text: &str) {
        if !self.enabled.load(Ordering::Acquire) {
            return;
        }
        self.visible.store(true, Ordering::Release);
        #[cfg(windows)]
        if let Ok(guard) = self.client.lock()
            && let Some(client) = guard.as_ref()
        {
            client.send(win::OverlayCmd::Show(text.to_owned()));
        }
        #[cfg(not(windows))]
        if let Ok(mut guard) = self.state.lock() {
            guard.animator.fade_target = 1.0;
            guard.finishing = false;
            let controls_enabled = guard.config.controls_enabled;
            let controls_pinned = guard.config.controls_pinned;
            guard
                .visibility
                .reset_for_session(controls_enabled, controls_pinned);
            guard.text = if pure::should_show_panel(text) {
                pure::tail_text(text.trim())
            } else {
                String::new()
            };
        }
    }

    /// Update the live transcription panel text.
    pub fn update_text(&self, text: &str) {
        if !self.visible.load(Ordering::Acquire) {
            return;
        }
        #[cfg(windows)]
        if let Ok(guard) = self.client.lock()
            && let Some(client) = guard.as_ref()
        {
            client.send(win::OverlayCmd::UpdateText(text.to_owned()));
        }
        #[cfg(not(windows))]
        if let Ok(mut guard) = self.state.lock() {
            guard.text = if pure::should_show_panel(text) {
                pure::tail_text(text.trim())
            } else {
                String::new()
            };
        }
    }

    /// Feed the live microphone level (`0.0..=1.0`).
    pub fn set_level(&self, level: f32) {
        #[cfg(windows)]
        if let Ok(guard) = self.client.lock()
            && let Some(client) = guard.as_ref()
        {
            client.send(win::OverlayCmd::SetLevel(level));
        }
        #[cfg(not(windows))]
        if let Ok(mut guard) = self.state.lock() {
            guard.animator.set_level(level);
        }
    }

    /// Feed live voice waveform features `(loud, tone)`.
    pub fn set_waveform(&self, loud: f32, tone: f32) {
        #[cfg(windows)]
        if let Ok(guard) = self.client.lock()
            && let Some(client) = guard.as_ref()
        {
            client.send(win::OverlayCmd::SetWaveform(loud, tone));
        }
        #[cfg(not(windows))]
        if let Ok(mut guard) = self.state.lock() {
            guard.animator.feed_voice(loud, tone);
        }
    }

    /// Keep the under-pill bar locked visible with `"Writing…"` and disable duplicate clicks while
    /// a dictation session is finalizing.
    pub fn set_finishing(&self, finishing: bool) {
        #[cfg(windows)]
        if let Ok(guard) = self.client.lock()
            && let Some(client) = guard.as_ref()
        {
            client.send(win::OverlayCmd::SetFinishing(finishing));
        }
        #[cfg(not(windows))]
        if let Ok(mut guard) = self.state.lock() {
            guard.finishing = finishing;
            guard.visibility.set_finishing(finishing);
        }
    }

    /// Lightweight live update for language, delivery mode, pin state, and recent languages
    /// without tearing down or recreating the pill.
    pub fn update_session_meta(
        &self,
        language: &str,
        delivery_mode: &str,
        pinned: bool,
        recent_languages: Vec<String>,
    ) {
        let lang = if language.trim().is_empty() {
            "en".to_owned()
        } else {
            language.trim().to_ascii_lowercase()
        };
        let mode = underbar::normalize_delivery_mode(delivery_mode).to_owned();
        let recents = underbar::sanitize_recent_languages(&recent_languages);

        #[cfg(windows)]
        if let Ok(guard) = self.client.lock()
            && let Some(client) = guard.as_ref()
        {
            client.send(win::OverlayCmd::UpdateSessionMeta {
                language: lang,
                delivery_mode: mode,
                pinned,
                recent_languages: recents,
            });
        }
        #[cfg(not(windows))]
        if let Ok(mut guard) = self.state.lock() {
            guard.config.language = lang;
            guard.config.delivery_mode = mode;
            guard.config.controls_pinned = pinned;
            guard.config.recent_languages = recents;
            guard.visibility.set_pinned(pinned);
        }
    }

    /// Apply updated overlay settings and theme polarity.
    pub fn update_config(&self, config: OverlayConfig) {
        let is_enabled = config.enabled;
        self.enabled.store(is_enabled, Ordering::Release);
        if !is_enabled {
            self.visible.store(false, Ordering::Release);
        }
        #[cfg(windows)]
        if let Ok(guard) = self.client.lock()
            && let Some(client) = guard.as_ref()
        {
            client.send(win::OverlayCmd::UpdateConfig(config));
        }
        #[cfg(not(windows))]
        if let Ok(mut guard) = self.state.lock() {
            let controls_enabled = config.controls_enabled;
            let controls_pinned = config.controls_pinned;
            guard.config = config;
            guard.visibility.set_controls_enabled(controls_enabled);
            guard.visibility.set_pinned(controls_pinned);
        }
    }

    /// Show the compact floating failed-paste action card (`Show text` / `Copy` / `Discard`)
    /// anchored at the pill's screen position when automatic pasting fails.
    pub fn show_recovery(&self, text: &str) {
        if !self.enabled.load(Ordering::Acquire) || text.trim().is_empty() {
            return;
        }
        #[cfg(windows)]
        if let Ok(guard) = self.client.lock()
            && let Some(client) = guard.as_ref()
        {
            client.send(win::OverlayCmd::ShowRecovery(text.to_owned()));
        }
        #[cfg(not(windows))]
        let _ = text;
    }

    /// Hide the floating failed-paste recovery card if visible.
    pub fn hide_recovery(&self) {
        #[cfg(windows)]
        if let Ok(guard) = self.client.lock()
            && let Some(client) = guard.as_ref()
        {
            client.send(win::OverlayCmd::HideRecovery);
        }
    }

    /// Fade out and hide the overlay.
    pub fn hide(&self) {
        self.visible.store(false, Ordering::Release);
        #[cfg(windows)]
        if let Ok(guard) = self.client.lock()
            && let Some(client) = guard.as_ref()
        {
            client.send(win::OverlayCmd::Hide);
        }
        #[cfg(not(windows))]
        if let Ok(mut guard) = self.state.lock() {
            guard.animator.fade_target = 0.0;
            guard.text.clear();
            guard.finishing = false;
            guard.visibility.reset_for_hide();
        }
    }

    /// Stop the overlay thread and release resources.
    pub fn shutdown(&self) {
        self.visible.store(false, Ordering::Release);
        #[cfg(windows)]
        if let Ok(mut guard) = self.client.lock()
            && let Some(mut client) = guard.take()
        {
            client.shutdown();
        }
        #[cfg(not(windows))]
        if let Ok(mut guard) = self.state.lock() {
            guard.animator.reset();
            guard.text.clear();
            guard.finishing = false;
            guard.visibility.reset_for_hide();
        }
    }

    /// Return whether the overlay is currently shown.
    #[must_use]
    pub fn is_visible(&self) -> bool {
        self.visible.load(Ordering::Acquire)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn overlay_handle_lifecycle_on_current_platform() {
        let handle = OverlayHandle::start(OverlayConfig::default());
        assert!(!handle.is_visible());

        handle.show(PLACEHOLDER);
        assert!(handle.is_visible());

        handle.update_text("Hello Eqho world");
        handle.set_level(0.75);
        handle.set_waveform(0.6, 0.4);
        assert!(handle.is_visible());

        handle.hide();
        assert!(!handle.is_visible());

        handle.shutdown();
        assert!(!handle.is_visible());
    }

    #[test]
    fn overlay_handle_start_with_callbacks_meta_and_finishing() {
        let recorded: Arc<Mutex<Vec<OverlayAction>>> = Arc::new(Mutex::new(Vec::new()));
        let recorded_cb = Arc::clone(&recorded);
        let on_action: OverlayActionCallback = Arc::new(move |act| {
            if let Ok(mut list) = recorded_cb.lock() {
                list.push(act);
            }
        });
        let flag_provider: FlagBitmapProvider = Arc::new(|code| {
            if code == "bg" {
                Some((2, 2, vec![255; 16]))
            } else {
                None
            }
        });

        let handle = OverlayHandle::start_with_callbacks(
            OverlayConfig::default(),
            Some(on_action),
            Some(flag_provider),
        );
        assert!(!handle.is_visible());

        handle.show(PLACEHOLDER);
        assert!(handle.is_visible());

        handle.update_session_meta(
            "BG",
            "live",
            true,
            vec!["bg".to_owned(), "en".to_owned(), "de".to_owned()],
        );
        handle.set_finishing(true);

        #[cfg(not(windows))]
        {
            let guard = handle.state.lock().unwrap();
            assert_eq!(guard.config.language, "bg");
            assert_eq!(guard.config.delivery_mode, "live");
            assert!(guard.config.controls_pinned);
            assert_eq!(guard.config.recent_languages, vec!["bg", "en", "de"]);
            assert!(guard.finishing);
            assert!(guard.visibility.visible);
            assert!(guard.visibility.finishing);
            assert!(guard.on_action.is_some());
            assert!(guard.flag_provider.as_ref().unwrap()("bg").is_some());
            // Verify callback closure works end-to-end.
            (guard.on_action.as_ref().unwrap())(OverlayAction::TogglePin(true));
        }

        #[cfg(not(windows))]
        assert_eq!(
            *recorded.lock().unwrap(),
            vec![OverlayAction::TogglePin(true)]
        );

        handle.set_finishing(false);
        handle.hide();
        assert!(!handle.is_visible());
        handle.shutdown();
    }
}
