//! Slint 1.19 software-renderer startup latency and memory measurement probe.

use std::time::{Duration, Instant};

use serde::Serialize;

slint::slint! {
    #[edition = "1.19"]
    export component SpikeProbeWindow inherits Window {
        width: 1px;
        height: 1px;
    }
}

/// Configuration for `tray-bench`.
#[derive(Debug)]
pub struct TrayBenchArgs {
    pub hold_ms: u64,
}

#[derive(Debug, Serialize)]
pub struct TrayBenchOutput {
    pub ok: bool,
    pub startup_ms: f64,
    pub private_bytes: u64,
    pub working_set_bytes: u64,
}

pub fn run_tray_bench(args: &TrayBenchArgs, process_start: Instant) -> Result<TrayBenchOutput, String> {
    // Force Slint's software renderer only (never femtovg/skia).
    slint::BackendSelector::new()
        .renderer_name("software".into())
        .select()
        .map_err(|e| format!("Slint software backend selection failed: {e}"))?;

    let _win = SpikeProbeWindow::new()
        .map_err(|e| format!("Slint window init failed: {e}"))?;
    let startup_ms = process_start.elapsed().as_secs_f64() * 1000.0;

    if args.hold_ms > 0 {
        std::thread::sleep(Duration::from_millis(args.hold_ms));
    }

    let (private_bytes, working_set_bytes) = query_process_memory();

    Ok(TrayBenchOutput {
        ok: true,
        startup_ms,
        private_bytes,
        working_set_bytes,
    })
}

#[cfg(windows)]
fn query_process_memory() -> (u64, u64) {
    use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX};
    use windows::Win32::System::Threading::GetCurrentProcess;

    let mut counters = PROCESS_MEMORY_COUNTERS_EX::default();
    let cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
    // SAFETY: `GetCurrentProcess()` is a pseudo-handle and `counters` is properly sized.
    let ok = unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            (&mut counters as *mut PROCESS_MEMORY_COUNTERS_EX).cast(),
            cb,
        )
    };
    if ok.is_ok() {
        (counters.PrivateUsage as u64, counters.WorkingSetSize as u64)
    } else {
        (0, 0)
    }
}

#[cfg(not(windows))]
fn query_process_memory() -> (u64, u64) {
    (0, 0)
}
