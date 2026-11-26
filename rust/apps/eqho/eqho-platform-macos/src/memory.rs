//! macOS available physical memory probe via Mach `host_statistics64`.

use std::ffi::{c_int, c_uint};

use eqho_platform::MemoryProbe;

const HOST_VM_INFO64: c_int = 4;
const HOST_VM_INFO64_COUNT: c_uint = 38;

#[repr(C)]
#[derive(Default)]
struct VmStatistics64 {
    free_count: u32,
    active_count: u32,
    inactive_count: u32,
    wire_count: u32,
    zero_fill_count: u64,
    reactivations: u64,
    pageins: u64,
    pageouts: u64,
    faults: u64,
    cow_faults: u64,
    lookups: u64,
    hits: u64,
    purges: u64,
    purgeable_count: u32,
    speculative_count: u32,
    decompressions: u64,
    compressions: u64,
    swapins: u64,
    swapouts: u64,
    compressor_page_count: u32,
    throttled_count: u32,
    external_page_count: u32,
    internal_page_count: u32,
    total_uncompressed_pages_in_compressor: u64,
}

#[link(name = "System", kind = "framework")]
unsafe extern "C" {
    fn mach_host_self() -> c_uint;
    fn host_page_size(host: c_uint, page_size: *mut usize) -> c_int;
    fn host_statistics64(
        host_priv: c_uint,
        flavor: c_int,
        host_info64_out: *mut i32,
        host_info64_out_cnt: *mut c_uint,
    ) -> c_int;
}

/// Queries available RAM (free + inactive + speculative + purgeable pages) via Mach VM stats.
#[derive(Debug, Default, Clone, Copy)]
pub struct MacMemoryProbe;

impl MemoryProbe for MacMemoryProbe {
    fn free_memory_mb(&self) -> Option<u64> {
        // SAFETY: Standard Mach kernel VM statistics query.
        unsafe {
            let host = mach_host_self();
            let mut page_size: usize = 0;
            if host_page_size(host, &mut page_size) != 0 || page_size == 0 {
                return None;
            }
            let mut stats = VmStatistics64::default();
            let mut count = HOST_VM_INFO64_COUNT;
            let rc = host_statistics64(
                host,
                HOST_VM_INFO64,
                (&mut stats as *mut VmStatistics64).cast(),
                &mut count,
            );
            if rc != 0 {
                return None;
            }
            let avail_pages = u64::from(stats.free_count)
                .saturating_add(u64::from(stats.inactive_count))
                .saturating_add(u64::from(stats.speculative_count))
                .saturating_add(u64::from(stats.purgeable_count));
            Some(avail_pages.saturating_mul(page_size as u64) / (1024 * 1024))
        }
    }
}
