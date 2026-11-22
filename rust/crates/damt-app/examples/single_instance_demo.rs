//! Run this twice. The first process waits for signals; the second signals it and exits.
//!
//! On Windows the first process prints `signal received` when the second launch happens. On other
//! platforms the single-instance exclusion works through the lock file, but the signal channel is not
//! implemented yet, so the first process keeps waiting.

use std::time::Duration;

use damt_app::{Acquire, SingleInstance};

const APP_ID: &str = "xyz.damt.ocy.demo";

fn main() {
    match SingleInstance::acquire(APP_ID) {
        Acquire::AlreadyRunning => {
            println!("another instance is running; signal sent");
        }
        Acquire::First(mut guard) => {
            println!("first instance; waiting for a second launch (Ctrl+C exits)");
            guard.listen(|| println!("signal received"));

            loop {
                std::thread::sleep(Duration::from_secs(1));
            }
        }
    }
}
