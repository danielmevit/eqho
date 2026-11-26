//! macOS implementations of the frozen [`eqho_platform`] OS-service contracts.
//!
//! Pure helpers in [`pure`] compile and run unit tests on all targets; native macOS
//! implementations (CoreAudio, CoreGraphics, AppKit, Mach, Metal) live behind
//! `#[cfg(target_os = "macos")]` and are exposed via [`MacEqhoPlatform`].

#![deny(clippy::unwrap_used, clippy::expect_used)]
#![warn(missing_debug_implementations)]

pub mod pure;

pub use pure::{MacHotkeyModifiers, ParsedMacHotkey, classify_apple_gpu, parse_hotkey_combo};

#[cfg(target_os = "macos")]
mod audio;
#[cfg(target_os = "macos")]
mod clipboard;
#[cfg(target_os = "macos")]
mod focus;
#[cfg(target_os = "macos")]
mod gpu;
#[cfg(target_os = "macos")]
mod hotkey;
#[cfg(target_os = "macos")]
mod injector;
#[cfg(target_os = "macos")]
mod memory;
#[cfg(target_os = "macos")]
mod mic_privacy;
#[cfg(target_os = "macos")]
mod volume;

#[cfg(target_os = "macos")]
pub use audio::MacAudioBackend;
#[cfg(target_os = "macos")]
pub use clipboard::MacClipboardService;
#[cfg(target_os = "macos")]
pub use focus::MacFocusTracker;
#[cfg(target_os = "macos")]
pub use gpu::MacGpuProbe;
#[cfg(target_os = "macos")]
pub use hotkey::MacHotkeyService;
#[cfg(target_os = "macos")]
pub use injector::MacInputInjector;
#[cfg(target_os = "macos")]
pub use memory::MacMemoryProbe;
#[cfg(target_os = "macos")]
pub use mic_privacy::MacMicPrivacyProbe;
#[cfg(target_os = "macos")]
pub use volume::MacVolumeControl;

/// Combined macOS platform facade implementing all frozen [`eqho_platform`] service traits.
#[cfg(target_os = "macos")]
#[derive(Debug, Default)]
pub struct MacEqhoPlatform {
    gpu: MacGpuProbe,
    memory: MacMemoryProbe,
    mic_privacy: MacMicPrivacyProbe,
    volume: MacVolumeControl,
    focus: MacFocusTracker,
    clipboard: MacClipboardService,
    injector: MacInputInjector,
    hotkey: MacHotkeyService,
    audio: MacAudioBackend,
}

#[cfg(target_os = "macos")]
impl MacEqhoPlatform {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

#[cfg(target_os = "macos")]
impl eqho_platform::GpuProbe for MacEqhoPlatform {
    fn list_adapters(&self) -> Vec<eqho_platform::GpuAdapter> {
        self.gpu.list_adapters()
    }

    fn cpu_has_avx2(&self) -> bool {
        self.gpu.cpu_has_avx2()
    }
}

#[cfg(target_os = "macos")]
impl eqho_platform::MemoryProbe for MacEqhoPlatform {
    fn free_memory_mb(&self) -> Option<u64> {
        self.memory.free_memory_mb()
    }
}

#[cfg(target_os = "macos")]
impl eqho_platform::MicPrivacyProbe for MacEqhoPlatform {
    fn mic_access_blocked(&self) -> Option<bool> {
        self.mic_privacy.mic_access_blocked()
    }

    fn open_mic_privacy_settings(&self) -> bool {
        self.mic_privacy.open_mic_privacy_settings()
    }
}

#[cfg(target_os = "macos")]
impl eqho_platform::VolumeControl for MacEqhoPlatform {
    fn is_supported(&self) -> bool {
        self.volume.is_supported()
    }

    fn get_volume(&self) -> Option<f32> {
        self.volume.get_volume()
    }

    fn set_volume(&self, level: f32) -> bool {
        self.volume.set_volume(level)
    }

    fn set_mute(&self, muted: bool) -> bool {
        self.volume.set_mute(muted)
    }
}

#[cfg(target_os = "macos")]
impl eqho_platform::FocusTracker for MacEqhoPlatform {
    fn get_foreground_window(&self) -> eqho_platform::WindowHandle {
        self.focus.get_foreground_window()
    }

    fn restore_foreground_window(&self, window: eqho_platform::WindowHandle) -> bool {
        self.focus.restore_foreground_window(window)
    }

    fn get_window_process_name(&self, window: eqho_platform::WindowHandle) -> Option<String> {
        self.focus.get_window_process_name(window)
    }
}

#[cfg(target_os = "macos")]
impl eqho_platform::ClipboardService for MacEqhoPlatform {
    fn get_text(&self) -> Option<String> {
        self.clipboard.get_text()
    }

    fn set_text(&self, text: &str, private: bool) -> Result<(), eqho_platform::PlatformError> {
        self.clipboard.set_text(text, private)
    }
}

#[cfg(target_os = "macos")]
impl eqho_platform::InputInjector for MacEqhoPlatform {
    fn held_modifiers(&self) -> Option<Vec<eqho_platform::ModifierKey>> {
        self.injector.held_modifiers()
    }

    fn release_modifiers(
        &self,
        mods: &[eqho_platform::ModifierKey],
    ) -> Result<(), eqho_platform::PlatformError> {
        self.injector.release_modifiers(mods)
    }

    fn send_paste_shortcut(&self) -> Result<(), eqho_platform::PlatformError> {
        self.injector.send_paste_shortcut()
    }

    fn type_unicode_text(&self, text: &str) -> Result<(), eqho_platform::PlatformError> {
        self.injector.type_unicode_text(text)
    }

    fn send_backspaces(&self, count: usize) -> Result<(), eqho_platform::PlatformError> {
        self.injector.send_backspaces(count)
    }
}

#[cfg(target_os = "macos")]
impl eqho_platform::HotkeyService for MacEqhoPlatform {
    fn register(
        &self,
        spec: &eqho_platform::HotkeySpec,
        callback: eqho_platform::HotkeyCallback,
    ) -> Result<Box<dyn eqho_platform::HotkeyRegistration>, eqho_platform::PlatformError> {
        self.hotkey.register(spec, callback)
    }
}

#[cfg(target_os = "macos")]
impl eqho_platform::AudioBackend for MacEqhoPlatform {
    fn list_input_devices(&self) -> Vec<eqho_platform::AudioDeviceInfo> {
        self.audio.list_input_devices()
    }

    fn start_capture(
        &self,
        device_index: Option<usize>,
        on_block: eqho_platform::AudioBlockCallback,
    ) -> Result<Box<dyn eqho_platform::AudioStreamHandle>, eqho_platform::PlatformError> {
        self.audio.start_capture(device_index, on_block)
    }

    fn play_pcm(
        &self,
        samples: &[f32],
        sample_rate: u32,
        blocking: bool,
    ) -> Result<(), eqho_platform::PlatformError> {
        self.audio.play_pcm(samples, sample_rate, blocking)
    }
}
