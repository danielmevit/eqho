//! Linux implementations of the frozen [`eqho_platform`] OS-service contracts.
//!
//! Pure helpers in [`pure`] compile and run unit tests on all targets; native Linux
//! implementations (dynamic Vulkan, PipeWire/PulseAudio, X11/XTest, Wayland fallbacks) live
//! behind `#[cfg(target_os = "linux")]` and are exposed via [`LinuxEqhoPlatform`].

#![deny(clippy::unwrap_used, clippy::expect_used)]
#![warn(missing_debug_implementations)]

pub mod pure;

pub use pure::{
    LinuxHotkeyModifiers, ParsedLinuxHotkey, is_software_vulkan_device, is_wayland_session,
    parse_hotkey_combo, parse_pactl_sources, parse_pactl_volume, parse_proc_meminfo_available_mb,
    parse_wpctl_volume,
};

#[cfg(target_os = "linux")]
mod audio;
#[cfg(target_os = "linux")]
mod clipboard;
#[cfg(target_os = "linux")]
mod focus;
#[cfg(target_os = "linux")]
mod gpu;
#[cfg(target_os = "linux")]
mod hotkey;
#[cfg(target_os = "linux")]
mod injector;
#[cfg(target_os = "linux")]
mod memory;
#[cfg(target_os = "linux")]
mod mic_privacy;
#[cfg(target_os = "linux")]
mod volume;
#[cfg(target_os = "linux")]
mod x11_ffi;

#[cfg(target_os = "linux")]
pub use audio::LinuxAudioBackend;
#[cfg(target_os = "linux")]
pub use clipboard::LinuxClipboardService;
#[cfg(target_os = "linux")]
pub use focus::LinuxFocusTracker;
#[cfg(target_os = "linux")]
pub use gpu::LinuxGpuProbe;
#[cfg(target_os = "linux")]
pub use hotkey::LinuxHotkeyService;
#[cfg(target_os = "linux")]
pub use injector::LinuxInputInjector;
#[cfg(target_os = "linux")]
pub use memory::LinuxMemoryProbe;
#[cfg(target_os = "linux")]
pub use mic_privacy::LinuxMicPrivacyProbe;
#[cfg(target_os = "linux")]
pub use volume::LinuxVolumeControl;

/// Combined Linux platform facade implementing all frozen [`eqho_platform`] service traits.
#[cfg(target_os = "linux")]
#[derive(Debug, Default)]
pub struct LinuxEqhoPlatform {
    gpu: LinuxGpuProbe,
    memory: LinuxMemoryProbe,
    mic_privacy: LinuxMicPrivacyProbe,
    volume: LinuxVolumeControl,
    focus: LinuxFocusTracker,
    clipboard: LinuxClipboardService,
    injector: LinuxInputInjector,
    hotkey: LinuxHotkeyService,
    audio: LinuxAudioBackend,
}

#[cfg(target_os = "linux")]
impl LinuxEqhoPlatform {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

#[cfg(target_os = "linux")]
impl eqho_platform::GpuProbe for LinuxEqhoPlatform {
    fn list_adapters(&self) -> Vec<eqho_platform::GpuAdapter> {
        self.gpu.list_adapters()
    }

    fn cpu_has_avx2(&self) -> bool {
        self.gpu.cpu_has_avx2()
    }
}

#[cfg(target_os = "linux")]
impl eqho_platform::MemoryProbe for LinuxEqhoPlatform {
    fn free_memory_mb(&self) -> Option<u64> {
        self.memory.free_memory_mb()
    }
}

#[cfg(target_os = "linux")]
impl eqho_platform::MicPrivacyProbe for LinuxEqhoPlatform {
    fn mic_access_blocked(&self) -> Option<bool> {
        self.mic_privacy.mic_access_blocked()
    }

    fn open_mic_privacy_settings(&self) -> bool {
        self.mic_privacy.open_mic_privacy_settings()
    }
}

#[cfg(target_os = "linux")]
impl eqho_platform::VolumeControl for LinuxEqhoPlatform {
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

#[cfg(target_os = "linux")]
impl eqho_platform::FocusTracker for LinuxEqhoPlatform {
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

#[cfg(target_os = "linux")]
impl eqho_platform::ClipboardService for LinuxEqhoPlatform {
    fn get_text(&self) -> Option<String> {
        self.clipboard.get_text()
    }

    fn set_text(&self, text: &str, private: bool) -> Result<(), eqho_platform::PlatformError> {
        self.clipboard.set_text(text, private)
    }
}

#[cfg(target_os = "linux")]
impl eqho_platform::InputInjector for LinuxEqhoPlatform {
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

#[cfg(target_os = "linux")]
impl eqho_platform::HotkeyService for LinuxEqhoPlatform {
    fn register(
        &self,
        spec: &eqho_platform::HotkeySpec,
        callback: eqho_platform::HotkeyCallback,
    ) -> Result<Box<dyn eqho_platform::HotkeyRegistration>, eqho_platform::PlatformError> {
        self.hotkey.register(spec, callback)
    }
}

#[cfg(target_os = "linux")]
impl eqho_platform::AudioBackend for LinuxEqhoPlatform {
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
