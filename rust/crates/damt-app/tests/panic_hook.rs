//! Panic-hook test in its own integration binary so the global hook does not disturb other tests.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use damt_app::{ErrorLog, install_panic_hook};

fn temp_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("damt-app-panic-{}-{nanos}", std::process::id()));
    let _ = fs::create_dir_all(&dir);
    dir
}

#[test]
fn panic_hook_writes_message_and_location() {
    let dir = temp_dir();
    install_panic_hook(ErrorLog::new(&dir, "ocy"));

    let result = std::panic::catch_unwind(|| panic!("boom from the panic-hook test"));
    assert!(result.is_err(), "catch_unwind should see the panic");

    let text = fs::read_to_string(dir.join("ocy-errors.log")).unwrap_or_default();
    assert!(text.contains("panic"), "context missing: {text}");
    assert!(
        text.contains("boom from the panic-hook test"),
        "payload missing: {text}"
    );
    assert!(text.contains(".rs:"), "location missing: {text}");

    let _ = fs::remove_dir_all(&dir);
}
