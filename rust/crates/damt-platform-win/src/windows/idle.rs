use std::mem::size_of;
use std::time::Duration;

use damt_platform::IdleProvider;
use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

use crate::IdleMath;

/// Idle time from `GetLastInputInfo`, measured against `GetTickCount64` (C# `WindowsIdleProvider`).
#[derive(Debug, Default, Clone, Copy)]
pub struct WinIdle;

impl IdleProvider for WinIdle {
    fn is_supported(&self) -> bool {
        true
    }

    fn idle_time(&self) -> Duration {
        let mut info = LASTINPUTINFO {
            cbSize: size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };

        // SAFETY: `info` is a properly sized LASTINPUTINFO and stays alive for the call.
        if !unsafe { GetLastInputInfo(&mut info) }.as_bool() {
            return Duration::ZERO;
        }

        // SAFETY: no pointers or invariants; the function takes no arguments.
        let now = unsafe { GetTickCount64() };
        IdleMath::compute(now as u32, info.dwTime)
    }
}
