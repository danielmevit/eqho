//! COM `IAudioEndpointVolume` master output volume control (never mutates `UserDuckingPreference`).

use eqho_platform::VolumeControl;
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{IMMDeviceEnumerator, MMDeviceEnumerator, eMultimedia, eRender};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
use windows::core::GUID;

use crate::com::{ComGuard, activate_mm_device};

/// Windows master output volume control for dictation ducking.
#[derive(Debug, Default, Clone, Copy)]
pub struct WinVolumeControl;

fn with_endpoint_volume<T>(f: impl FnOnce(&IAudioEndpointVolume) -> Option<T>) -> Option<T> {
    let _com = ComGuard::init().ok()?;
    // SAFETY: COM is initialized on this thread; `MMDeviceEnumerator` is a standard system CLSID.
    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.ok()?;
    // SAFETY: `enumerator` is a valid COM pointer; `GetDefaultAudioEndpoint` returns the active render device.
    let device = unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia) }.ok()?;
    let volume: IAudioEndpointVolume = activate_mm_device(&device, CLSCTX_ALL).ok()?;
    let result = f(&volume);
    // Drop COM interfaces explicitly before `_com` runs `CoUninitialize`.
    drop(volume);
    drop(device);
    drop(enumerator);
    result
}

impl VolumeControl for WinVolumeControl {
    fn is_supported(&self) -> bool {
        with_endpoint_volume(|_| Some(())).is_some()
    }

    fn get_volume(&self) -> Option<f32> {
        with_endpoint_volume(|volume| {
            // SAFETY: `volume` is a valid `IAudioEndpointVolume` COM pointer.
            unsafe { volume.GetMasterVolumeLevelScalar() }.ok()
        })
    }

    fn set_volume(&self, level: f32) -> bool {
        let clamped = level.clamp(0.0, 1.0);
        with_endpoint_volume(|volume| {
            let ctx = GUID::zeroed();
            // SAFETY: `volume` is a valid COM pointer and `&ctx` points to a valid `GUID`.
            unsafe { volume.SetMasterVolumeLevelScalar(clamped, &ctx) }.ok()
        })
        .is_some()
    }

    fn set_mute(&self, muted: bool) -> bool {
        with_endpoint_volume(|volume| {
            let ctx = GUID::zeroed();
            // SAFETY: `volume` is a valid COM pointer and `&ctx` points to a valid `GUID`.
            unsafe { volume.SetMute(muted, &ctx) }.ok()
        })
        .is_some()
    }
}
