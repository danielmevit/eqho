//! Windows implementations of the frozen [`eqho_platform`] OS-service contracts.
//!
//! Pure helpers in [`pure`] compile and run unit tests on all targets; Win32/COM/WASAPI/DXGI
//! implementations are gated behind `#[cfg(windows)]` and exposed via [`WinEqhoPlatform`].

#![deny(clippy::unwrap_used, clippy::expect_used)]

pub mod pure;

pub use pure::{
    HotkeyModifiers, ParsedHotkey, classify_mic_consent, downmix_f32_to_mono, downmix_i16_to_mono,
    from_utf16_nul, has_discrete_gpu, is_software_adapter, linear_resample_mono,
    parse_hotkey_combo, utf16_with_nul,
};

#[cfg(windows)]
mod audio;
#[cfg(windows)]
mod clipboard;
#[cfg(windows)]
mod com;
#[cfg(windows)]
mod focus;
#[cfg(windows)]
mod gpu;
#[cfg(windows)]
mod hotkey;
#[cfg(windows)]
mod injector;
#[cfg(windows)]
mod memory;
#[cfg(windows)]
mod mic_privacy;
#[cfg(windows)]
mod volume;

#[cfg(windows)]
pub use audio::WinAudioBackend;
#[cfg(windows)]
pub use clipboard::WinClipboardService;
#[cfg(windows)]
pub use focus::WinFocusTracker;
#[cfg(windows)]
pub use gpu::WinGpuProbe;
#[cfg(windows)]
pub use hotkey::WinHotkeyService;
#[cfg(windows)]
pub use injector::WinInputInjector;
#[cfg(windows)]
pub use memory::WinMemoryProbe;
#[cfg(windows)]
pub use mic_privacy::WinMicPrivacyProbe;
#[cfg(windows)]
pub use volume::WinVolumeControl;

/// Combined Windows platform facade implementing all frozen [`eqho_platform`] service traits.
#[cfg(windows)]
#[derive(Debug, Default)]
pub struct WinEqhoPlatform {
    gpu: WinGpuProbe,
    memory: WinMemoryProbe,
    mic_privacy: WinMicPrivacyProbe,
    volume: WinVolumeControl,
    focus: WinFocusTracker,
    clipboard: WinClipboardService,
    injector: WinInputInjector,
    hotkey: WinHotkeyService,
    audio: WinAudioBackend,
}

#[cfg(windows)]
impl WinEqhoPlatform {
    /// Creates a new Windows platform service bundle.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn gpu(&self) -> &WinGpuProbe {
        &self.gpu
    }

    #[must_use]
    pub fn memory(&self) -> &WinMemoryProbe {
        &self.memory
    }

    #[must_use]
    pub fn mic_privacy(&self) -> &WinMicPrivacyProbe {
        &self.mic_privacy
    }

    #[must_use]
    pub fn volume(&self) -> &WinVolumeControl {
        &self.volume
    }

    #[must_use]
    pub fn focus(&self) -> &WinFocusTracker {
        &self.focus
    }

    #[must_use]
    pub fn clipboard(&self) -> &WinClipboardService {
        &self.clipboard
    }

    #[must_use]
    pub fn injector(&self) -> &WinInputInjector {
        &self.injector
    }

    #[must_use]
    pub fn hotkey(&self) -> &WinHotkeyService {
        &self.hotkey
    }

    #[must_use]
    pub fn audio(&self) -> &WinAudioBackend {
        &self.audio
    }
}

#[cfg(windows)]
impl eqho_platform::GpuProbe for WinEqhoPlatform {
    fn list_adapters(&self) -> Vec<eqho_platform::GpuAdapter> {
        self.gpu.list_adapters()
    }

    fn cpu_has_avx2(&self) -> bool {
        self.gpu.cpu_has_avx2()
    }
}

#[cfg(windows)]
impl eqho_platform::MemoryProbe for WinEqhoPlatform {
    fn free_memory_mb(&self) -> Option<u64> {
        self.memory.free_memory_mb()
    }
}

#[cfg(windows)]
impl eqho_platform::MicPrivacyProbe for WinEqhoPlatform {
    fn mic_access_blocked(&self) -> Option<bool> {
        self.mic_privacy.mic_access_blocked()
    }

    fn open_mic_privacy_settings(&self) -> bool {
        self.mic_privacy.open_mic_privacy_settings()
    }
}

#[cfg(windows)]
impl eqho_platform::VolumeControl for WinEqhoPlatform {
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

#[cfg(windows)]
impl eqho_platform::FocusTracker for WinEqhoPlatform {
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

#[cfg(windows)]
impl eqho_platform::ClipboardService for WinEqhoPlatform {
    fn get_text(&self) -> Option<String> {
        self.clipboard.get_text()
    }

    fn set_text(&self, text: &str, private: bool) -> Result<(), eqho_platform::PlatformError> {
        self.clipboard.set_text(text, private)
    }
}

#[cfg(windows)]
impl eqho_platform::InputInjector for WinEqhoPlatform {
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

#[cfg(windows)]
impl eqho_platform::HotkeyService for WinEqhoPlatform {
    fn register(
        &self,
        spec: &eqho_platform::HotkeySpec,
        callback: eqho_platform::HotkeyCallback,
    ) -> Result<Box<dyn eqho_platform::HotkeyRegistration>, eqho_platform::PlatformError> {
        self.hotkey.register(spec, callback)
    }
}

#[cfg(windows)]
impl eqho_platform::AudioBackend for WinEqhoPlatform {
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
