//! Physical free-memory probe via `GlobalMemoryStatusEx`.

use std::mem::size_of;

use eqho_platform::MemoryProbe;
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

/// Windows physical memory probe.
#[derive(Debug, Default, Clone, Copy)]
pub struct WinMemoryProbe;

impl MemoryProbe for WinMemoryProbe {
    fn free_memory_mb(&self) -> Option<u64> {
        let mut status = MEMORYSTATUSEX {
            dwLength: size_of::<MEMORYSTATUSEX>() as u32,
            ..Default::default()
        };
        // SAFETY: `status.dwLength` is initialized to `size_of::<MEMORYSTATUSEX>()` and `status`
        // is a valid mutable stack struct.
        unsafe { GlobalMemoryStatusEx(&mut status) }.ok()?;
        Some(status.ullAvailPhys / (1024 * 1024))
    }
}
