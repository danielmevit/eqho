use std::ffi::c_void;

use damt_platform::MotionPreference;
use windows::Win32::UI::WindowsAndMessaging::{
    SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
};

use crate::MotionMath;

/// The OS "Animation effects" preference (C# `WindowsMotionPreference`).
#[derive(Debug, Default, Clone, Copy)]
pub struct WinMotion;

impl MotionPreference for WinMotion {
    fn is_supported(&self) -> bool {
        true
    }

    fn prefers_reduced_motion(&self) -> bool {
        let mut client_area_animation: i32 = 0;

        // SAFETY: the pointer is a valid, aligned BOOL-sized out-parameter for SPI_GETCLIENTAREAANIMATION.
        let call = unsafe {
            SystemParametersInfoW(
                SPI_GETCLIENTAREAANIMATION,
                0,
                Some(&mut client_area_animation as *mut i32 as *mut c_void),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )
        };

        MotionMath::reduced(call.is_ok(), client_area_animation)
    }
}
