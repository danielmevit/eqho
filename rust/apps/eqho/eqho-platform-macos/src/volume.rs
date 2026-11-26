//! macOS CoreAudio master output volume control for dictation audio ducking.

use std::ffi::c_void;
use std::ptr::null;

use eqho_platform::VolumeControl;

const K_AUDIO_OBJECT_SYSTEM_OBJECT: u32 = 1;
const K_AUDIO_HARDWARE_PROPERTY_DEFAULT_OUTPUT_DEVICE: u32 = u32::from_be_bytes(*b"dOut");
const K_AUDIO_HARDWARE_SERVICE_VIRTUAL_MAIN_VOLUME: u32 = u32::from_be_bytes(*b"vmvc");
const K_AUDIO_DEVICE_PROPERTY_MUTE: u32 = u32::from_be_bytes(*b"mute");
const K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL: u32 = u32::from_be_bytes(*b"glob");
const K_AUDIO_OBJECT_PROPERTY_SCOPE_OUTPUT: u32 = u32::from_be_bytes(*b"outp");
const K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN: u32 = 0;

#[repr(C)]
struct AudioObjectPropertyAddress {
    selector: u32,
    scope: u32,
    element: u32,
}

#[link(name = "CoreAudio", kind = "framework")]
unsafe extern "C" {
    fn AudioObjectGetPropertyData(
        object_id: u32,
        address: *const AudioObjectPropertyAddress,
        qualifier_data_size: u32,
        qualifier_data: *const c_void,
        data_size: *mut u32,
        data: *mut c_void,
    ) -> i32;
    fn AudioObjectSetPropertyData(
        object_id: u32,
        address: *const AudioObjectPropertyAddress,
        qualifier_data_size: u32,
        qualifier_data: *const c_void,
        data_size: u32,
        data: *const c_void,
    ) -> i32;
    fn AudioObjectHasProperty(object_id: u32, address: *const AudioObjectPropertyAddress) -> u8;
}

/// CoreAudio virtual main volume and mute controller for the default output device.
#[derive(Debug, Default, Clone, Copy)]
pub struct MacVolumeControl;

impl MacVolumeControl {
    fn default_output_device() -> Option<u32> {
        let addr = AudioObjectPropertyAddress {
            selector: K_AUDIO_HARDWARE_PROPERTY_DEFAULT_OUTPUT_DEVICE,
            scope: K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL,
            element: K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN,
        };
        let mut dev_id: u32 = 0;
        let mut size = std::mem::size_of::<u32>() as u32;
        // SAFETY: Standard CoreAudio query for the default output AudioDeviceID.
        let status = unsafe {
            AudioObjectGetPropertyData(
                K_AUDIO_OBJECT_SYSTEM_OBJECT,
                &addr,
                0,
                null(),
                &mut size,
                (&mut dev_id as *mut u32).cast(),
            )
        };
        if status == 0 && dev_id != 0 {
            Some(dev_id)
        } else {
            None
        }
    }
}

impl VolumeControl for MacVolumeControl {
    fn is_supported(&self) -> bool {
        let Some(dev) = Self::default_output_device() else {
            return false;
        };
        let addr = AudioObjectPropertyAddress {
            selector: K_AUDIO_HARDWARE_SERVICE_VIRTUAL_MAIN_VOLUME,
            scope: K_AUDIO_OBJECT_PROPERTY_SCOPE_OUTPUT,
            element: K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN,
        };
        // SAFETY: Checking property availability on valid CoreAudio device.
        unsafe { AudioObjectHasProperty(dev, &addr) != 0 }
    }

    fn get_volume(&self) -> Option<f32> {
        let dev = Self::default_output_device()?;
        let addr = AudioObjectPropertyAddress {
            selector: K_AUDIO_HARDWARE_SERVICE_VIRTUAL_MAIN_VOLUME,
            scope: K_AUDIO_OBJECT_PROPERTY_SCOPE_OUTPUT,
            element: K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN,
        };
        let mut vol: f32 = 0.0;
        let mut size = std::mem::size_of::<f32>() as u32;
        // SAFETY: Reading `Float32` virtual main volume from CoreAudio.
        let status = unsafe {
            AudioObjectGetPropertyData(
                dev,
                &addr,
                0,
                null(),
                &mut size,
                (&mut vol as *mut f32).cast(),
            )
        };
        if status == 0 && vol.is_finite() {
            Some(vol.clamp(0.0, 1.0))
        } else {
            None
        }
    }

    fn set_volume(&self, level: f32) -> bool {
        let Some(dev) = Self::default_output_device() else {
            return false;
        };
        let addr = AudioObjectPropertyAddress {
            selector: K_AUDIO_HARDWARE_SERVICE_VIRTUAL_MAIN_VOLUME,
            scope: K_AUDIO_OBJECT_PROPERTY_SCOPE_OUTPUT,
            element: K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN,
        };
        let vol = level.clamp(0.0, 1.0);
        // SAFETY: Writing `Float32` virtual main volume to CoreAudio.
        unsafe {
            AudioObjectSetPropertyData(
                dev,
                &addr,
                0,
                null(),
                std::mem::size_of::<f32>() as u32,
                (&vol as *const f32).cast(),
            ) == 0
        }
    }

    fn set_mute(&self, muted: bool) -> bool {
        let Some(dev) = Self::default_output_device() else {
            return false;
        };
        let addr = AudioObjectPropertyAddress {
            selector: K_AUDIO_DEVICE_PROPERTY_MUTE,
            scope: K_AUDIO_OBJECT_PROPERTY_SCOPE_OUTPUT,
            element: K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN,
        };
        let val: u32 = u32::from(muted);
        // SAFETY: Writing `UInt32` mute flag to CoreAudio.
        unsafe {
            AudioObjectSetPropertyData(
                dev,
                &addr,
                0,
                null(),
                std::mem::size_of::<u32>() as u32,
                (&val as *const u32).cast(),
            ) == 0
        }
    }
}
