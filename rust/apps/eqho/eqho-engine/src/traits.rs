//! Platform bundle, UI event sink, and speech backend traits for `eqho-engine`.

use std::path::Path;

use eqho_host::{HostSegment, ModelHost, ModelHostError, ModelInfo};
use eqho_platform::{
    AudioBackend, ClipboardService, FocusTracker, GpuProbe, HotkeyService, InputInjector,
    MemoryProbe, MicPrivacyProbe, VolumeControl,
};

/// Unified OS-service bundle required by [`crate::DictationCoordinator`].
pub trait EqhoPlatform:
    GpuProbe
    + MemoryProbe
    + MicPrivacyProbe
    + VolumeControl
    + FocusTracker
    + ClipboardService
    + InputInjector
    + HotkeyService
    + AudioBackend
    + Send
    + Sync
    + 'static
{
}

impl<T> EqhoPlatform for T where
    T: GpuProbe
        + MemoryProbe
        + MicPrivacyProbe
        + VolumeControl
        + FocusTracker
        + ClipboardService
        + InputInjector
        + HotkeyService
        + AudioBackend
        + Send
        + Sync
        + 'static
{
}

/// UI event callbacks emitted by [`crate::DictationCoordinator`] for the overlay,
/// system tray, notifications, recovery banner, and history list.
pub trait UiEventSink: Send + Sync + 'static {
    fn show_overlay(&self, text: &str);
    fn update_overlay_text(&self, text: &str);
    fn set_overlay_level(&self, level: f32);
    fn set_overlay_waveform(&self, loud: f32, tone: f32);
    fn hide_overlay(&self);
    fn set_tray_active(&self, active: bool);
    fn notify(&self, message: &str);
    fn on_model_progress(&self, model_key: &str, message: &str, fraction: Option<f32>);
    fn on_recovery_available(&self, text: Option<&str>);
    fn on_dictation_committed(&self, _text: &str) {}
    fn on_history_changed(&self);
    fn on_settings_changed(&self, _settings: &eqho_core::settings::Settings) {}
}

/// No-op [`UiEventSink`] for `--smoke` and headless tests.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopUiSink;

impl UiEventSink for NoopUiSink {
    fn show_overlay(&self, _text: &str) {}
    fn update_overlay_text(&self, _text: &str) {}
    fn set_overlay_level(&self, _level: f32) {}
    fn set_overlay_waveform(&self, _loud: f32, _tone: f32) {}
    fn hide_overlay(&self) {}
    fn set_tray_active(&self, _active: bool) {}
    fn notify(&self, _message: &str) {}
    fn on_model_progress(&self, _model_key: &str, _message: &str, _fraction: Option<f32>) {}
    fn on_recovery_available(&self, _text: Option<&str>) {}
    fn on_history_changed(&self) {}
}

/// Abstract speech-to-text backend so production uses [`eqho_host::ModelHost`]
/// while tests can inject deterministic fake or fault-injecting backends.
pub trait SpeechBackend: Send {
    fn is_ready(&mut self, model_key: &str, use_gpu: bool) -> bool;
    fn load_model(
        &mut self,
        model_key: &str,
        model_dir: &Path,
        use_gpu: bool,
        threads: u32,
        on_progress: &mut dyn FnMut(&str, Option<f32>),
    ) -> Result<ModelInfo, ModelHostError>;
    fn transcribe(
        &mut self,
        audio: &[f32],
        language: &str,
        beam_size: u32,
        initial_prompt: &str,
    ) -> Result<Vec<HostSegment>, ModelHostError>;
    fn info(&self) -> Option<&ModelInfo>;
    fn shutdown(&mut self);
}

impl SpeechBackend for ModelHost {
    fn is_ready(&mut self, model_key: &str, use_gpu: bool) -> bool {
        ModelHost::is_ready(self, model_key, use_gpu)
    }

    fn load_model(
        &mut self,
        model_key: &str,
        model_dir: &Path,
        use_gpu: bool,
        threads: u32,
        on_progress: &mut dyn FnMut(&str, Option<f32>),
    ) -> Result<ModelInfo, ModelHostError> {
        ModelHost::load_model(self, model_key, model_dir, use_gpu, threads, on_progress)
    }

    fn transcribe(
        &mut self,
        audio: &[f32],
        language: &str,
        beam_size: u32,
        initial_prompt: &str,
    ) -> Result<Vec<HostSegment>, ModelHostError> {
        ModelHost::transcribe(self, audio, language, beam_size, initial_prompt)
    }

    fn info(&self) -> Option<&ModelInfo> {
        ModelHost::info(self)
    }

    fn shutdown(&mut self) {
        ModelHost::shutdown(self);
    }
}
