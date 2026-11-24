//! A size-capped error log and a panic hook, matching the C# `ErrorLogger`.

use std::any::Any;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use crate::pure::format_line;

/// A rotating error log: `<data>/<prefix>-errors.log`, rotated at 256 KB to
/// `<data>/<prefix>-errors.old.log`.
///
/// Logging never panics and never propagates I/O errors. For Ocy the prefix is `ocy`.
#[derive(Debug, Clone)]
pub struct ErrorLog {
    data_dir: PathBuf,
    prefix: String,
    gate: Arc<Mutex<()>>,
}

impl ErrorLog {
    /// Rotation threshold, exactly the C# `ErrorLogger.MaxLogSizeBytes`.
    pub const MAX_SIZE_BYTES: u64 = 256 * 1024;

    /// A log writing to `data_dir/<prefix>-errors.log`.
    pub fn new(data_dir: impl Into<PathBuf>, prefix: impl Into<String>) -> Self {
        Self {
            data_dir: data_dir.into(),
            prefix: prefix.into(),
            gate: Arc::new(Mutex::new(())),
        }
    }

    /// The log file path.
    pub fn path(&self) -> PathBuf {
        self.data_dir.join(format!("{}-errors.log", self.prefix))
    }

    /// The rotated log file path.
    pub fn old_path(&self) -> PathBuf {
        self.data_dir
            .join(format!("{}-errors.old.log", self.prefix))
    }

    /// Append a timestamped line. Failures are swallowed.
    pub fn log(&self, context: Option<&str>, message: &str) {
        let line = format_line(SystemTime::now(), context, message);
        self.append(&line);
    }

    /// Append a timestamped line for a `Display` error. Failures are swallowed.
    pub fn log_error(&self, context: &str, error: &dyn fmt::Display) {
        self.log(Some(context), &error.to_string());
    }

    fn append(&self, line: &str) {
        let _guard = self
            .gate
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if fs::create_dir_all(&self.data_dir).is_err() {
            return;
        }

        let log_path = self.path();
        let old_path = self.old_path();

        if let Ok(metadata) = fs::metadata(&log_path)
            && metadata.len() >= Self::MAX_SIZE_BYTES
        {
            rotate(&log_path, &old_path);
        }

        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&log_path) {
            let _ = writeln!(file, "{line}");
        }
    }
}

/// Move `log` over `old`, replacing an existing old file when the platform allows it.
///
/// Rotation is best effort: the caller still appends to the original log if this fails.
fn rotate(log: &Path, old: &Path) {
    if fs::rename(log, old).is_ok() {
        return;
    }

    // Windows can refuse a rename over an existing file through some filesystems or third-party filters.
    // Mirror the C# fallback: delete the old file (best effort), then try again.
    let _ = fs::remove_file(old);
    let _ = fs::rename(log, old);
}

/// Install a panic hook that writes the panic payload and location to `log` before calling the previous
/// hook.
///
/// The release profile uses `panic = "abort"`, so this log entry is the only record of the panic.
pub fn install_panic_hook(log: ErrorLog) {
    let previous = std::panic::take_hook();

    std::panic::set_hook(Box::new(move |info| {
        let payload = payload_text(info.payload());
        let location = match info.location() {
            Some(location) => format!(
                "{}:{}:{}",
                location.file(),
                location.line(),
                location.column()
            ),
            None => String::from("unknown location"),
        };

        log.log(Some("panic"), &format!("{payload} at {location}"));
        previous(info);
    }));
}

fn payload_text(payload: &(dyn Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_string()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        String::from("non-string panic payload")
    }
}
