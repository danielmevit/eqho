//! Unix single-instance tests: the lock-file path only.

#![cfg(not(windows))]

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use damt_app::{Acquire, SingleInstance};

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
fn second_acquire_is_already_running_and_drop_releases() {
    let dir = temp_dir("single-instance");
    let app_id = "xyz.damt.ocy";

    let first = SingleInstance::acquire_in_dir(&dir, app_id);
    let guard = match first {
        Acquire::First(guard) => guard,
        Acquire::AlreadyRunning => {
            let _ = fs::remove_dir_all(&dir);
            panic!("the first acquire should win");
        }
    };

    assert!(
        dir.join(".ocy.lock").exists(),
        "the lock file should use the C# Ocy name"
    );
    assert!(matches!(
        SingleInstance::acquire_in_dir(&dir, app_id),
        Acquire::AlreadyRunning
    ));

    drop(guard);

    assert!(matches!(
        SingleInstance::acquire_in_dir(&dir, app_id),
        Acquire::First(_)
    ));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn different_app_ids_do_not_exclude_each_other() {
    let dir = temp_dir("single-instance-apps");

    let first = SingleInstance::acquire_in_dir(&dir, "xyz.damt.ocy.one");
    let second = SingleInstance::acquire_in_dir(&dir, "xyz.damt.ocy.two");

    assert!(matches!(first, Acquire::First(_)));
    assert!(matches!(second, Acquire::First(_)));

    drop(first);
    drop(second);
    let _ = fs::remove_dir_all(&dir);
}
