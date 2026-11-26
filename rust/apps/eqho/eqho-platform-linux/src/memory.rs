//! Physical free-memory probe via `/proc/meminfo`.

use eqho_platform::MemoryProbe;

use crate::pure::parse_proc_meminfo_available_mb;

/// Queries currently available physical RAM from `/proc/meminfo`.
#[derive(Debug, Default, Clone, Copy)]
pub struct LinuxMemoryProbe;

impl MemoryProbe for LinuxMemoryProbe {
    fn free_memory_mb(&self) -> Option<u64> {
        let contents = std::fs::read_to_string("/proc/meminfo").ok()?;
        parse_proc_meminfo_available_mb(&contents)
    }
}
