//! FROZEN Eqho OS-service contracts, lead-owned.
//!
//! Workers implement these traits in `eqho-platform-win` (and later Linux/macOS crates); they must
//! not break these signatures. Every probe method degrades gracefully rather than panicking.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

pub use damt_platform::PlatformError;

/// Hardware graphics adapter returned by [`GpuProbe`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GpuAdapter {
    pub name: String,
    pub vendor_id: u32,
    pub dedicated_video_memory_bytes: u64,
}

/// Hardware GPU and CPU-instruction probes used for backend/model selection.
pub trait GpuProbe: Send + Sync {
    /// Non-software hardware graphics adapters on this system. Empty on failure or unsupported OS.
    fn list_adapters(&self) -> Vec<GpuAdapter>;
    /// Whether the CPU supports AVX2 (required by AVX2-compiled whisper.cpp CPU backends).
    fn cpu_has_avx2(&self) -> bool;
}

/// Physical free-memory probe (for low-memory load warnings).
pub trait MemoryProbe: Send + Sync {
    /// Currently available physical RAM in MiB, or `None` if unavailable.
    fn free_memory_mb(&self) -> Option<u64>;
}

/// OS microphone privacy permission probe and settings opener.
pub trait MicPrivacyProbe: Send + Sync {
    /// `Some(true)` = explicitly blocked by OS privacy switch, `Some(false)` = explicitly allowed,
    /// `None` = unknown or unsupported. Read-only: must never write OS settings.
    fn mic_access_blocked(&self) -> Option<bool>;
    /// Opens the OS microphone privacy settings page. Returns `true` if launched.
    fn open_mic_privacy_settings(&self) -> bool;
}

/// Master output volume control for dictation ducking.
/// Implementations must initialize/uninitialize COM per thread cleanly on Windows.
pub trait VolumeControl: Send + Sync {
    fn is_supported(&self) -> bool;
    /// Scalar master volume in `0.0..=1.0`, or `None` on failure/unsupported.
    fn get_volume(&self) -> Option<f32>;
    /// Set scalar master volume in `0.0..=1.0`.
    fn set_volume(&self, level: f32) -> bool;
    /// Set master mute state.
    fn set_mute(&self, muted: bool) -> bool;
}

/// Opaque native window handle (HWND cast to `usize` on Windows; `0` = none).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct WindowHandle(pub usize);

impl WindowHandle {
    pub const NONE: Self = Self(0);

    #[must_use]
    pub const fn is_valid(self) -> bool {
        self.0 != 0
    }
}

/// Foreground window capture, verified focus restoration, and process basename lookup.
pub trait FocusTracker: Send + Sync {
    fn get_foreground_window(&self) -> WindowHandle;
    /// Attempt to focus `window` and verify it became the active foreground window.
    /// Returns `true` only if activation succeeded and `get_foreground_window() == window`.
    fn restore_foreground_window(&self, window: WindowHandle) -> bool;
    /// Basename of the executable owning `window` (e.g. `"notepad.exe"`), or `None`.
    fn get_window_process_name(&self, window: WindowHandle) -> Option<String>;
    /// Returns `true` when `window` is a known non-text surface (Eqho's own window, Desktop
    /// wallpaper, or Taskbar) where sending `Ctrl+V` would silently vanish instead of pasting.
    fn is_non_paste_target(&self, _window: WindowHandle) -> bool {
        false
    }
}

/// Modifier key identifier for selective release before paste (`EQ-MODS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ModifierKey {
    CtrlLeft,
    CtrlRight,
    ShiftLeft,
    ShiftRight,
    AltLeft,
    AltRight,
    WinLeft,
    WinRight,
}

/// Clipboard read/write with privacy opt-out support (`EQ-CLIP`).
pub trait ClipboardService: Send + Sync {
    /// Read current Unicode clipboard text, or `None` if unavailable or non-text.
    fn get_text(&self) -> Option<String>;
    /// Write Unicode text to the clipboard. When `private` is `true`, marks the clipboard payload
    /// to exclude it from clipboard history, cloud clipboard, and clipboard monitors where supported.
    fn set_text(&self, text: &str, private: bool) -> Result<(), PlatformError>;
}

/// Keyboard input simulation for paste, character typing, backspaces, and held-modifier queries.
pub trait InputInjector: Send + Sync {
    /// Returns the set of modifiers physically held down right now, or `None` if unsupported.
    fn held_modifiers(&self) -> Option<Vec<ModifierKey>>;
    /// Emit key-up events ONLY for the listed modifiers.
    fn release_modifiers(&self, mods: &[ModifierKey]) -> Result<(), PlatformError>;
    /// Send the platform paste shortcut (`Ctrl+V` on Windows/Linux, `Cmd+V` on macOS).
    fn send_paste_shortcut(&self) -> Result<(), PlatformError>;
    /// Simulate typing Unicode characters directly.
    fn type_unicode_text(&self, text: &str) -> Result<(), PlatformError>;
    /// Send `count` Backspace keystrokes (used by `"delete that"`).
    fn send_backspaces(&self, count: usize) -> Result<(), PlatformError>;
}

/// Activation behavior for the global dictation hotkey.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum HotkeyMode {
    #[default]
    Toggle,
    Hold,
}

/// Parsed hotkey combination (e.g., `"alt+q"` or `"ctrl+shift+f9"`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HotkeySpec {
    pub raw: String,
    pub mode: HotkeyMode,
}

/// Events emitted by a registered global hotkey hook.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HotkeyEvent {
    Toggle,
    HoldPressed,
    HoldReleased,
}

/// Active hotkey registration handle; dropping unregisters cleanly.
pub trait HotkeyRegistration: Send {}

/// Callback invoked by a registered global hotkey hook.
pub type HotkeyCallback = Box<dyn Fn(HotkeyEvent) + Send + Sync>;

/// Global hotkey listener service.
pub trait HotkeyService: Send + Sync {
    fn register(
        &self,
        spec: &HotkeySpec,
        callback: HotkeyCallback,
    ) -> Result<Box<dyn HotkeyRegistration>, PlatformError>;
}

/// Microphone input device descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioDeviceInfo {
    pub index: usize,
    pub name: String,
    pub is_default: bool,
}

/// Active audio capture stream handle; dropping stops and closes the microphone cleanly.
pub trait AudioStreamHandle: Send {}

/// Callback receiving 16 kHz mono `f32` audio blocks (~50 ms / 800 samples).
pub type AudioBlockCallback = Box<dyn Fn(&[f32]) + Send + Sync>;

/// 16 kHz mono float32 microphone capture and blocking chime playback.
pub trait AudioBackend: Send + Sync {
    fn list_input_devices(&self) -> Vec<AudioDeviceInfo>;
    /// Open a 16 kHz mono `f32` capture stream delivering ~50 ms blocks (`~800` samples) to `on_block`.
    fn start_capture(
        &self,
        device_index: Option<usize>,
        on_block: AudioBlockCallback,
    ) -> Result<Box<dyn AudioStreamHandle>, PlatformError>;
    /// Play mono `f32` PCM samples at `sample_rate` Hz (used for start/stop chime).
    /// When `blocking` is true, returns only after playback completes.
    fn play_pcm(&self, samples: &[f32], sample_rate: u32, blocking: bool) -> Result<(), PlatformError>;
}

/// Safe no-op fallback implementation for unsupported platforms and headless tests.
#[derive(Debug, Default, Clone, Copy)]
pub struct UnsupportedEqhoPlatform;

impl GpuProbe for UnsupportedEqhoPlatform {
    fn list_adapters(&self) -> Vec<GpuAdapter> {
        Vec::new()
    }
    fn cpu_has_avx2(&self) -> bool {
        true
    }
}

impl MemoryProbe for UnsupportedEqhoPlatform {
    fn free_memory_mb(&self) -> Option<u64> {
        None
    }
}

impl MicPrivacyProbe for UnsupportedEqhoPlatform {
    fn mic_access_blocked(&self) -> Option<bool> {
        None
    }
    fn open_mic_privacy_settings(&self) -> bool {
        false
    }
}

impl VolumeControl for UnsupportedEqhoPlatform {
    fn is_supported(&self) -> bool {
        false
    }
    fn get_volume(&self) -> Option<f32> {
        None
    }
    fn set_volume(&self, _level: f32) -> bool {
        false
    }
    fn set_mute(&self, _muted: bool) -> bool {
        false
    }
}

impl FocusTracker for UnsupportedEqhoPlatform {
    fn get_foreground_window(&self) -> WindowHandle {
        WindowHandle::NONE
    }
    fn restore_foreground_window(&self, _window: WindowHandle) -> bool {
        false
    }
    fn get_window_process_name(&self, _window: WindowHandle) -> Option<String> {
        None
    }
}

impl ClipboardService for UnsupportedEqhoPlatform {
    fn get_text(&self) -> Option<String> {
        None
    }
    fn set_text(&self, _text: &str, _private: bool) -> Result<(), PlatformError> {
        Err(PlatformError("clipboard unsupported".to_owned()))
    }
}

impl InputInjector for UnsupportedEqhoPlatform {
    fn held_modifiers(&self) -> Option<Vec<ModifierKey>> {
        None
    }
    fn release_modifiers(&self, _mods: &[ModifierKey]) -> Result<(), PlatformError> {
        Ok(())
    }
    fn send_paste_shortcut(&self) -> Result<(), PlatformError> {
        Err(PlatformError("input injection unsupported".to_owned()))
    }
    fn type_unicode_text(&self, _text: &str) -> Result<(), PlatformError> {
        Err(PlatformError("input injection unsupported".to_owned()))
    }
    fn send_backspaces(&self, _count: usize) -> Result<(), PlatformError> {
        Err(PlatformError("input injection unsupported".to_owned()))
    }
}

impl HotkeyService for UnsupportedEqhoPlatform {
    fn register(
        &self,
        _spec: &HotkeySpec,
        _callback: HotkeyCallback,
    ) -> Result<Box<dyn HotkeyRegistration>, PlatformError> {
        Err(PlatformError("global hotkeys unsupported".to_owned()))
    }
}

impl AudioBackend for UnsupportedEqhoPlatform {
    fn list_input_devices(&self) -> Vec<AudioDeviceInfo> {
        Vec::new()
    }
    fn start_capture(
        &self,
        _device_index: Option<usize>,
        _on_block: AudioBlockCallback,
    ) -> Result<Box<dyn AudioStreamHandle>, PlatformError> {
        Err(PlatformError("audio capture unsupported".to_owned()))
    }
    fn play_pcm(&self, _samples: &[f32], _sample_rate: u32, _blocking: bool) -> Result<(), PlatformError> {
        Ok(())
    }
}
