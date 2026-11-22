use damt_platform::{FocusState, FocusStateProvider};
use windows::Win32::UI::Shell::SHQueryUserNotificationState;

use crate::FocusStateMapper;

/// Focus state from `SHQueryUserNotificationState` (C# `WindowsFocusStateProvider`).
#[derive(Debug, Default, Clone, Copy)]
pub struct WinFocus;

impl FocusStateProvider for WinFocus {
    fn is_supported(&self) -> bool {
        true
    }

    fn state(&self) -> FocusState {
        // SAFETY: the function takes no arguments and writes only into its own out-parameter.
        match unsafe { SHQueryUserNotificationState() } {
            Ok(state) => FocusStateMapper::map(0, state.0),
            Err(error) => FocusStateMapper::map(error.code().0, 0),
        }
    }
}
