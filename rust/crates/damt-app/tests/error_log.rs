//! ErrorLog formatting, rotation and failure handling (temp dirs, all platforms).

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use damt_app::ErrorLog;

fn temp_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("damt-app-{name}-{}-{nanos}", std::process::id()));
    let _ = fs::create_dir_all(&dir);
    dir
}

#[test]
fn logs_a_timestamped_line() {
    let dir = temp_dir("error-log-line");
    let log = ErrorLog::new(&dir, "ocy");

    log.log(Some("startup"), "ready");
    let text = fs::read_to_string(log.path()).unwrap_or_default();

    assert!(text.starts_with('['), "line should start with a timestamp");
    assert!(
        text.contains("startup: ready"),
        "context and message missing"
    );
    assert!(text.contains('T'), "timestamp should contain a T");
    assert!(text.ends_with('\n'), "line should end with a newline");
    assert_eq!(log.path(), dir.join("ocy-errors.log"));
    assert_eq!(log.old_path(), dir.join("ocy-errors.old.log"));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn logs_without_context() {
    let dir = temp_dir("error-log-no-context");
    let log = ErrorLog::new(&dir, "ocy");

    log.log(None, "bare message");
    let text = fs::read_to_string(log.path()).unwrap_or_default();

    assert!(text.contains("] bare message"));
    assert!(!text.contains(": bare message"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn rotates_at_256_kb_and_replaces_the_old_file() {
    let dir = temp_dir("error-log-rotate");
    let log = ErrorLog::new(&dir, "ocy");

    let big = vec![b'x'; ErrorLog::MAX_SIZE_BYTES as usize];
    let _ = fs::write(log.path(), &big);
    let _ = fs::write(log.old_path(), b"previous old log");

    log.log(Some("rotate"), "after rotation");

    let old = fs::read(log.old_path()).unwrap_or_default();
    let new = fs::read_to_string(log.path()).unwrap_or_default();

    assert_eq!(old, big, "the oversized log should move to the old path");
    assert!(new.contains("rotate: after rotation"));
    assert!(new.len() < 1_024, "the new log should be small");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn does_not_rotate_below_the_limit() {
    let dir = temp_dir("error-log-no-rotate");
    let log = ErrorLog::new(&dir, "ocy");

    let _ = fs::write(log.path(), b"small");
    log.log(Some("keep"), "appended");

    assert!(!log.old_path().exists());
    let text = fs::read_to_string(log.path()).unwrap_or_default();
    assert!(text.starts_with("small"));
    assert!(text.contains("keep: appended"));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn io_failures_are_swallowed() {
    let dir = temp_dir("error-log-failure");
    let log = ErrorLog::new(&dir, "ocy");

    // A directory at the log path makes appending fail; the logger must not panic.
    let _ = fs::create_dir_all(log.path());
    log.log(Some("io"), "cannot append");
    log.log_error("io", &std::io::Error::other("example"));

    assert!(log.path().is_dir());
    let _ = fs::remove_dir_all(&dir);
}
