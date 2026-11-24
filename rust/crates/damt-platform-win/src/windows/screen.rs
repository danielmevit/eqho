use std::mem::size_of;

use damt_platform::{ActiveScreenProvider, ScreenRect};
use windows::Win32::Foundation::POINT;
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, HMONITOR, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTONULL, MONITORINFO,
    MonitorFromPoint, MonitorFromWindow,
};
use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, GetForegroundWindow, GetShellWindow};

use crate::RectMath;

/// The monitor holding the foreground window, else the one under the cursor (C# `WindowsActiveScreenProvider`).
#[derive(Debug, Default, Clone, Copy)]
pub struct WinActiveScreen;

impl ActiveScreenProvider for WinActiveScreen {
    fn is_supported(&self) -> bool {
        true
    }

    fn active_screen_bounds(&self) -> Option<ScreenRect> {
        // SAFETY: these functions have no arguments and only read the current desktop state.
        let (foreground, shell) = unsafe { (GetForegroundWindow(), GetShellWindow()) };

        let mut monitor = HMONITOR::default();
        if !foreground.is_invalid() && foreground != shell {
            // SAFETY: `foreground` is a window handle returned by the OS.
            monitor = unsafe { MonitorFromWindow(foreground, MONITOR_DEFAULTTONULL) };
        }

        if monitor.is_invalid() {
            let mut point = POINT::default();
            // SAFETY: `point` is a valid, aligned out-parameter.
            if unsafe { GetCursorPos(&mut point) }.is_err() {
                return None;
            }

            // SAFETY: `point` was filled in by GetCursorPos and is passed by value.
            monitor = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST) };
        }

        if monitor.is_invalid() {
            return None;
        }

        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };

        // SAFETY: `monitor` is valid and `info.cbSize` is set as the API requires.
        if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
            return None;
        }

        Some(RectMath::to_screen_rect(
            info.rcMonitor.left,
            info.rcMonitor.top,
            info.rcMonitor.right,
            info.rcMonitor.bottom,
        ))
    }
}
