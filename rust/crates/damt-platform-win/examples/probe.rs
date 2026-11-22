//! Prints every provider's current value once a second for ten seconds, then listens for session
//! events for twenty seconds. Windows only; the lead runs it on the owner's PC.

#[cfg(not(windows))]
fn main() {
    eprintln!("damt-platform-win probe: this example runs on Windows only.");
}

#[cfg(windows)]
fn main() {
    use damt_platform::{PlatformServices, SessionEventSource};
    use damt_platform_win::WinPlatform;
    use std::sync::mpsc::{RecvTimeoutError, channel};
    use std::time::{Duration, Instant};

    let mut platform = WinPlatform::new("damt-platform-win-probe");

    for _ in 0..10 {
        println!(
            "idle={:?} focus={:?} autostart={} screen={:?} reduced_motion={} light_taskbar={:?}",
            platform.idle().idle_time(),
            platform.focus().state(),
            platform.autostart().is_enabled(),
            platform.active_screen().active_screen_bounds(),
            platform.motion().prefers_reduced_motion(),
            platform.taskbar_theme().is_light_taskbar(),
        );
        std::thread::sleep(Duration::from_secs(1));
    }

    let (events_tx, events_rx) = channel();
    platform.session().start(Box::new(move |event| {
        let _ = events_tx.send(event);
    }));

    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        match events_rx.recv_timeout(Duration::from_millis(200)) {
            Ok(event) => println!("session={event:?}"),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}
