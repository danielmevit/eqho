//! Local transcript history — JSONL file in the config dir, pruned to a cap.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use jiff::Timestamp;
use jiff::tz::TimeZone;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const MAX_ENTRIES: usize = 1000;

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranscriptEntry {
    pub ts: f64,
    pub text: String,
    pub duration: f64,
    pub model: String,
    pub lang: String,
}

#[derive(Debug, Error)]
pub enum HistoryError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug)]
pub struct TranscriptHistory {
    path: PathBuf,
    max_entries: usize,
    lock: Mutex<()>,
}

impl TranscriptHistory {
    pub fn new(path: impl Into<PathBuf>, max_entries: usize) -> Self {
        Self {
            path: path.into(),
            max_entries,
            lock: Mutex::new(()),
        }
    }

    pub fn with_default_cap(path: impl Into<PathBuf>) -> Self {
        Self::new(path, MAX_ENTRIES)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn append(
        &self,
        text: &str,
        duration: f64,
        model: &str,
        lang: &str,
    ) -> Result<(), HistoryError> {
        let now = Timestamp::now();
        let ts = now.as_second() as f64 + f64::from(now.subsec_nanosecond()) / 1_000_000_000.0;
        self.append_with_ts(ts, text, duration, model, lang)
    }

    pub fn append_with_ts(
        &self,
        ts: f64,
        text: &str,
        duration: f64,
        model: &str,
        lang: &str,
    ) -> Result<(), HistoryError> {
        if text.is_empty() {
            return Ok(());
        }
        let entry = TranscriptEntry {
            ts,
            text: text.to_owned(),
            duration: (duration * 100.0).round() / 100.0,
            model: model.to_owned(),
            lang: lang.to_owned(),
        };

        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut line = serde_json::to_string(&entry)?;
        line.push('\n');
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        file.write_all(line.as_bytes())?;
        file.flush()?;
        self.prune_locked()
    }

    /// Return all entries, newest first.
    pub fn read_all(&self) -> Vec<TranscriptEntry> {
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut entries = self.read_locked();
        entries.reverse();
        entries
    }

    /// Case-insensitive substring search, newest first.
    pub fn search(&self, query: &str) -> Vec<TranscriptEntry> {
        let q = query.trim().to_lowercase();
        let entries = self.read_all();
        if q.is_empty() {
            return entries;
        }
        entries
            .into_iter()
            .filter(|e| e.text.to_lowercase().contains(&q))
            .collect()
    }

    pub fn delete(&self, ts: f64) -> Result<(), HistoryError> {
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        let entries: Vec<TranscriptEntry> = self
            .read_locked()
            .into_iter()
            .filter(|e| e.ts != ts)
            .collect();
        self.write_locked(&entries)
    }

    pub fn clear(&self) -> Result<(), HistoryError> {
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        match fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(HistoryError::Io(e)),
        }
    }

    /// Write a human-readable `.txt` (newest first). Returns entry count.
    pub fn export_txt(&self, dest: &Path) -> Result<usize, HistoryError> {
        let entries = self.read_all();
        let mut lines = Vec::with_capacity(entries.len());
        let tz = TimeZone::system();
        for e in &entries {
            let secs = e.ts.trunc() as i64;
            let nanos = ((e.ts - e.ts.trunc()) * 1_000_000_000.0) as i32;
            let stamp = Timestamp::new(secs, nanos)
                .unwrap_or(Timestamp::UNIX_EPOCH)
                .to_zoned_datetime(tz.clone())
                .strftime("%Y-%m-%d %H:%M")
                .to_string();
            lines.push(format!("[{stamp}] {}", e.text));
        }
        let content = if lines.is_empty() {
            String::new()
        } else {
            format!("{}\n", lines.join("\n"))
        };
        if let Some(parent) = dest.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        fs::write(dest, content)?;
        Ok(entries.len())
    }

    fn read_locked(&self) -> Vec<TranscriptEntry> {
        let Ok(raw) = fs::read_to_string(&self.path) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for line in raw.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Ok(entry) = serde_json::from_str::<TranscriptEntry>(trimmed) {
                out.push(entry);
            }
        }
        out
    }

    fn prune_locked(&self) -> Result<(), HistoryError> {
        let entries = self.read_locked();
        if entries.len() > self.max_entries {
            let start = entries.len() - self.max_entries;
            self.write_locked(&entries[start..])?;
        }
        Ok(())
    }

    fn write_locked(&self, entries: &[TranscriptEntry]) -> Result<(), HistoryError> {
        if entries.is_empty() {
            match fs::remove_file(&self.path) {
                Ok(()) => return Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                Err(e) => return Err(HistoryError::Io(e)),
            }
        }

        let dir = self.path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(dir)?;
        let seq = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp_path = dir.join(format!("history_{}_{}.tmp", std::process::id(), seq));

        let mut buf = String::new();
        for e in entries {
            buf.push_str(&serde_json::to_string(e)?);
            buf.push('\n');
        }

        let write_res = (|| -> Result<(), std::io::Error> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp_path)?;
            file.write_all(buf.as_bytes())?;
            file.flush()?;
            file.sync_all()?;
            Ok(())
        })();

        if let Err(err) = write_res {
            let _ = fs::remove_file(&tmp_path);
            return Err(HistoryError::Io(err));
        }

        if let Err(err) = fs::rename(&tmp_path, &self.path) {
            let _ = fs::remove_file(&tmp_path);
            return Err(HistoryError::Io(err));
        }
        Ok(())
    }
}

/// Format a Unix epoch timestamp (seconds) in the user's local timezone as `YYYY-MM-DD HH:MM`.
#[must_use]
pub fn format_timestamp_local(ts_secs: f64) -> String {
    let secs = if ts_secs.is_finite() && ts_secs >= 0.0 {
        ts_secs.floor() as i64
    } else {
        0
    };
    Timestamp::new(secs, 0)
        .unwrap_or(Timestamp::UNIX_EPOCH)
        .to_zoned_datetime(TimeZone::system())
        .strftime("%Y-%m-%d %H:%M")
        .to_string()
}

/// Return a monotonic local calendar day ordinal (days since Unix epoch in local timezone)
/// so "Today", "Yesterday", and "Last 7/30 days" align with the user's local midnight.
#[must_use]
pub fn local_day_ordinal(ts_secs: f64) -> i64 {
    let secs = if ts_secs.is_finite() {
        ts_secs.floor() as i64
    } else {
        0
    };
    let zdt = Timestamp::new(secs, 0)
        .unwrap_or(Timestamp::UNIX_EPOCH)
        .to_zoned_datetime(TimeZone::system());
    let offset_secs = i64::from(zdt.offset().seconds());
    secs.saturating_add(offset_secs).div_euclid(86_400)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("eqho_core_hist_{name}_{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn append_prune_search_delete_clear_and_export() {
        let dir = temp_dir("ops");
        let path = dir.join("history.jsonl");
        let hist = TranscriptHistory::new(&path, 3);

        for i in 0..5 {
            hist.append_with_ts(1000.0 + f64::from(i), &format!("entry {i}"), 1.234, "tiny", "en")
                .unwrap();
        }

        let entries = hist.read_all();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].text, "entry 4");
        assert_eq!(entries[2].text, "entry 2");
        assert!((entries[0].duration - 1.23).abs() < 1e-9);

        let found = hist.search("ENTRY 3");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].text, "entry 3");

        hist.delete(1003.0).unwrap();
        let after_del = hist.read_all();
        assert_eq!(after_del.len(), 2);
        assert!(after_del.iter().all(|e| e.ts != 1003.0));

        let export_path = dir.join("export.txt");
        let exported = hist.export_txt(&export_path).unwrap();
        assert_eq!(exported, 2);
        let txt = fs::read_to_string(&export_path).unwrap();
        assert!(txt.contains("entry 4"));
        assert!(txt.contains("entry 2"));

        hist.clear().unwrap();
        assert!(hist.read_all().is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn skips_blank_and_corrupt_lines() {
        let dir = temp_dir("corrupt_lines");
        let path = dir.join("history.jsonl");
        fs::write(
            &path,
            "\n{\"ts\": 1.0, \"text\": \"ok\", \"duration\": 0.5, \"model\": \"base\", \"lang\": \"en\"}\n{bad\n\n",
        )
        .unwrap();
        let hist = TranscriptHistory::with_default_cap(&path);
        let entries = hist.read_all();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].text, "ok");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn local_timestamp_and_day_ordinal_behave_consistently() {
        let s = format_timestamp_local(1_763_816_400.0);
        assert_eq!(s.len(), 16);
        assert_eq!(&s[4..5], "-");
        assert_eq!(&s[10..11], " ");
        let d0 = local_day_ordinal(1_763_816_400.0);
        let d1 = local_day_ordinal(1_763_816_400.0 + 86_400.0);
        assert_eq!(d1 - d0, 1);
    }
}
