//! Eqho floating iridescent pill and live transcription panel overlay.
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![warn(missing_debug_implementations)]

pub mod pure;
#[cfg(windows)]
mod win;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub use pure::{
    FADE_INTERVAL_MS, FADE_STEPS, FRAME_MS, FrameParams, GAP, LIGHT_PILL_ALPHA, MARGIN,
    MAX_TEXT_CHARS, OverlayConfig, OverlayRect, PANEL_MAX_W, PANEL_PADDING_X, PANEL_PADDING_Y,
    PLACEHOLDER, PillAnimator, WorkArea, calc_panel_rect, calc_pill_position, is_anchored_top,
    should_show_panel, tail_text,
};

#[cfg(not(windows))]
#[derive(Debug)]
struct NonWinState {
    config: OverlayConfig,
    text: String,
    animator: PillAnimator,
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
        let enabled = Arc::new(AtomicBool::new(config.enabled));
        let visible = Arc::new(AtomicBool::new(false));
        #[cfg(windows)]
        {
            let client = win::WinOverlayClient::start(config);
            Self {
                visible,
                enabled,
                client: Mutex::new(client),
            }
        }
        #[cfg(not(windows))]
        {
            Self {
                visible,
                enabled,
                state: Mutex::new(NonWinState {
                    config,
                    text: String::new(),
                    animator: PillAnimator::new(),
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
            guard.config = config;
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
}
