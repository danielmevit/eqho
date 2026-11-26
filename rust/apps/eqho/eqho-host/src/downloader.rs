//! GGML model downloader (native Windows WinHTTP + Unix streaming curl/wget with live progress).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use eqho_core::modelstore::{model_info, whispercpp_model_path};

use crate::error::HostError;

/// Minimum file size (1 MiB) for an existing `.bin` model file to be considered cached.
pub const MIN_CACHED_MODEL_BYTES: u64 = 1_048_576;

/// Format a byte count into a human-readable MB / GB string (e.g. `"580 MB"`, `"1.5 GB"`).
#[must_use]
pub fn format_bytes(bytes: u64) -> String {
    const MIB: f64 = 1_048_576.0;
    const GIB: f64 = 1_073_741_824.0;
    let b = bytes as f64;
    if b >= GIB {
        format!("{:.2} GB", b / GIB)
    } else if b >= MIB {
        format!("{:.0} MB", b / MIB)
    } else {
        format!("{} KB", (bytes / 1024).max(1))
    }
}

/// Format an estimated remaining duration in seconds into a concise ETA string.
#[must_use]
pub fn format_eta(remaining_secs: f64) -> String {
    if !remaining_secs.is_finite() || remaining_secs < 0.0 {
        return String::new();
    }
    let total = remaining_secs.round() as u64;
    if total <= 3 {
        "< 5s left".to_owned()
    } else if total < 60 {
        format!("~{total}s left")
    } else if total < 3600 {
        let mins = total / 60;
        let secs = total % 60;
        if secs == 0 {
            format!("~{mins}m left")
        } else {
            format!("~{mins}m {secs:02}s left")
        }
    } else {
        let hrs = total / 3600;
        let mins = (total % 3600) / 60;
        format!("~{hrs}h {mins:02}m left")
    }
}

/// Build a human-friendly progress line including percent, transferred bytes, speed, and ETA.
#[must_use]
pub fn format_download_progress(
    downloaded: u64,
    total_bytes: Option<u64>,
    elapsed_secs: f64,
) -> String {
    let speed_bps = if elapsed_secs >= 0.4 && downloaded > 0 {
        Some(downloaded as f64 / elapsed_secs)
    } else {
        None
    };
    let speed_str = speed_bps.map(|bps| format!("{:.1} MB/s", bps / 1_048_576.0));

    if let Some(total) = total_bytes.filter(|&t| t > 0) {
        let pct = ((downloaded.saturating_mul(100)) / total).min(100);
        let mut parts = vec![
            format!("Downloading {pct}%"),
            format!("{} / {}", format_bytes(downloaded), format_bytes(total)),
        ];
        if let Some(spd) = speed_str {
            parts.push(spd);
        }
        if let Some(bps) = speed_bps
            && bps > 1.0
            && total > downloaded
        {
            let eta = format_eta((total - downloaded) as f64 / bps);
            if !eta.is_empty() {
                parts.push(eta);
            }
        }
        parts.join(" · ")
    } else {
        let mut parts = vec![format!("Downloading {}", format_bytes(downloaded))];
        if let Some(spd) = speed_str {
            parts.push(spd);
        }
        parts.join(" · ")
    }
}

/// Parse an `https://` URL into `(host, port, path_and_query)`.
pub fn parse_https_url(url: &str) -> Result<(String, u16, String), HostError> {
    let rest = url
        .strip_prefix("https://")
        .ok_or_else(|| HostError::InvalidUrl("URL must start with https://".into()))?;
    if rest.is_empty() || rest.chars().any(char::is_whitespace) {
        return Err(HostError::InvalidUrl("URL is empty or contains whitespace".into()));
    }

    let (authority, path) = match rest.find('/') {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => (rest, "/"),
    };

    if authority.contains('@') || authority.is_empty() {
        return Err(HostError::InvalidUrl("invalid authority in URL".into()));
    }

    let (host, port) = if let Some(colon) = authority.rfind(':') {
        let host_part = &authority[..colon];
        let port_part = &authority[colon + 1..];
        if host_part.is_empty() || port_part.is_empty() {
            return Err(HostError::InvalidUrl("empty host or port".into()));
        }
        let port: u16 = port_part
            .parse()
            .map_err(|_| HostError::InvalidUrl(format!("invalid port: {port_part}")))?;
        if port == 0 {
            return Err(HostError::InvalidUrl("port must be 1..=65535".into()));
        }
        (host_part, port)
    } else {
        (authority, 443)
    };

    if host.is_empty() {
        return Err(HostError::InvalidUrl("empty host".into()));
    }

    Ok((host.to_owned(), port, path.to_owned()))
}

/// Ensure the GGML model for `model_key` exists under `model_dir`, downloading
/// it via WinHTTP on Windows if missing or smaller than 1 MiB.
pub fn download_ggml_model(
    model_key: &str,
    model_dir: &Path,
    on_progress: &mut dyn FnMut(&str, Option<f32>),
) -> Result<PathBuf, HostError> {
    let never_cancel = AtomicBool::new(false);
    download_ggml_model_cancellable(model_key, model_dir, &never_cancel, on_progress)
}

/// Same as [`download_ggml_model`], aborting cleanly and removing partial temp files
/// whenever `cancel` is set to `true`.
pub fn download_ggml_model_cancellable(
    model_key: &str,
    model_dir: &Path,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(&str, Option<f32>),
) -> Result<PathBuf, HostError> {
    if cancel.load(Ordering::Acquire) {
        return Err(HostError::DownloadCancelled);
    }
    let target = whispercpp_model_path(model_dir, model_key);
    if let Ok(meta) = std::fs::metadata(&target)
        && meta.is_file()
        && meta.len() > MIN_CACHED_MODEL_BYTES
    {
        return Ok(target);
    }

    let info = model_info(model_key)
        .ok_or_else(|| HostError::DownloadFailed(format!("unknown model key: {model_key}")))?;

    download_https_to_file(info.download_url, &target, cancel, on_progress)?;
    Ok(target)
}

#[cfg(windows)]
fn download_https_to_file(
    url: &str,
    target: &Path,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(&str, Option<f32>),
) -> Result<(), HostError> {
    use std::ffi::c_void;
    use std::fs::File;
    use std::io::{BufWriter, Write};
    use std::time::{Duration, Instant};

    use windows::Win32::Networking::WinHttp::{
        WINHTTP_ACCESS_TYPE_DEFAULT_PROXY, WINHTTP_FLAG_SECURE, WINHTTP_OPTION_REDIRECT_POLICY,
        WINHTTP_OPTION_REDIRECT_POLICY_DISALLOW_HTTPS_TO_HTTP, WINHTTP_QUERY_CONTENT_LENGTH,
        WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect,
        WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders, WinHttpReadData,
        WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetOption, WinHttpSetTimeouts,
    };
    use windows::core::PCWSTR;

    struct HInternetGuard(*mut c_void);

    impl Drop for HInternetGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: `self.0` is a valid WinHTTP handle owned by this guard.
                unsafe {
                    let _ = WinHttpCloseHandle(self.0);
                }
            }
        }
    }

    struct TempFileGuard {
        path: PathBuf,
        armed: bool,
    }

    impl TempFileGuard {
        fn disarm(&mut self) {
            self.armed = false;
        }
    }

    impl Drop for TempFileGuard {
        fn drop(&mut self) {
            if self.armed {
                let _ = std::fs::remove_file(&self.path);
            }
        }
    }

    fn to_wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    let (host, port, path) = parse_https_url(url)?;

    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let tmp_name = format!(
        "{}.tmp.{}",
        target
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("model.bin"),
        std::process::id()
    );
    let tmp_path = target.with_file_name(tmp_name);
    let mut tmp_guard = TempFileGuard {
        path: tmp_path.clone(),
        armed: true,
    };

    let agent_w = to_wide("Eqho/0.9");
    let host_w = to_wide(&host);
    let path_w = to_wide(&path);
    let verb_w = to_wide("GET");

    // SAFETY: Null-terminated UTF-16 user-agent string; proxy settings use system default.
    let session = HInternetGuard(unsafe {
        WinHttpOpen(
            PCWSTR(agent_w.as_ptr()),
            WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        )
    });
    if session.0.is_null() {
        return Err(HostError::DownloadFailed(format!(
            "WinHttpOpen failed: {}",
            std::io::Error::last_os_error()
        )));
    }

    let redirect_policy: u32 = WINHTTP_OPTION_REDIRECT_POLICY_DISALLOW_HTTPS_TO_HTTP;
    // SAFETY: `session.0` is a valid WinHTTP session handle and `redirect_policy` is a `u32`.
    unsafe {
        WinHttpSetOption(
            Some(session.0),
            WINHTTP_OPTION_REDIRECT_POLICY,
            Some(&redirect_policy.to_ne_bytes()),
        )
        .map_err(|e| HostError::DownloadFailed(format!("WinHttpSetOption redirect failed: {e}")))?;
        let _ = WinHttpSetTimeouts(session.0, 30_000, 30_000, 30_000, 60_000);
    }

    // SAFETY: `session.0` is valid and `host_w` is null-terminated UTF-16.
    let connect = HInternetGuard(unsafe { WinHttpConnect(session.0, PCWSTR(host_w.as_ptr()), port, 0) });
    if connect.0.is_null() {
        return Err(HostError::DownloadFailed(format!(
            "WinHttpConnect failed: {}",
            std::io::Error::last_os_error()
        )));
    }

    // SAFETY: `connect.0` is valid and `verb_w`/`path_w` are null-terminated UTF-16.
    let request = HInternetGuard(unsafe {
        WinHttpOpenRequest(
            connect.0,
            PCWSTR(verb_w.as_ptr()),
            PCWSTR(path_w.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            WINHTTP_FLAG_SECURE,
        )
    });
    if request.0.is_null() {
        return Err(HostError::DownloadFailed(format!(
            "WinHttpOpenRequest failed: {}",
            std::io::Error::last_os_error()
        )));
    }

    // SAFETY: `request.0` is a valid request handle with no extra headers or body.
    unsafe {
        WinHttpSendRequest(request.0, None, None, 0, 0, 0)
            .map_err(|e| HostError::DownloadFailed(format!("WinHttpSendRequest failed: {e}")))?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut())
            .map_err(|e| HostError::DownloadFailed(format!("WinHttpReceiveResponse failed: {e}")))?;
    }

    let mut status_code: u32 = 0;
    let mut status_len: u32 = std::mem::size_of::<u32>() as u32;
    // SAFETY: `status_code` is a `u32` buffer of `status_len` bytes.
    unsafe {
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some((&mut status_code as *mut u32).cast()),
            &mut status_len,
            std::ptr::null_mut(),
        )
        .map_err(|e| HostError::DownloadFailed(format!("WinHttpQueryHeaders status failed: {e}")))?;
    }
    if !(200..=299).contains(&status_code) {
        return Err(HostError::DownloadFailed(format!(
            "HTTP {status_code} downloading {url}"
        )));
    }

    let total_bytes: Option<u64> = {
        let mut len_buf = [0u16; 64];
        let mut byte_len: u32 = (len_buf.len() * 2) as u32;
        // SAFETY: `len_buf` has `byte_len` bytes capacity.
        let ok = unsafe {
            WinHttpQueryHeaders(
                request.0,
                WINHTTP_QUERY_CONTENT_LENGTH,
                PCWSTR::null(),
                Some(len_buf.as_mut_ptr().cast()),
                &mut byte_len,
                std::ptr::null_mut(),
            )
            .is_ok()
        };
        if ok {
            let wide_len = (byte_len as usize) / 2;
            let text = String::from_utf16_lossy(&len_buf[..wide_len.min(len_buf.len())]);
            text.trim_matches('\0').trim().parse::<u64>().ok()
        } else {
            None
        }
    };

    on_progress("Downloading 0% · Connecting…", total_bytes.map(|_| 0.0));

    {
        let file = File::create(&tmp_path)?;
        let mut writer = BufWriter::with_capacity(64 * 1024, file);
        let mut chunk = vec![0u8; 64 * 1024];
        let mut downloaded: u64 = 0;
        let mut last_pct: i32 = -1;
        let download_start = Instant::now();
        let mut last_report = download_start;

        loop {
            if cancel.load(Ordering::Acquire) {
                return Err(HostError::DownloadCancelled);
            }
            let mut bytes_read: u32 = 0;
            // SAFETY: `request.0` is valid and `chunk` is a writable buffer of `chunk.len()` bytes.
            unsafe {
                WinHttpReadData(
                    request.0,
                    chunk.as_mut_ptr().cast(),
                    chunk.len() as u32,
                    &mut bytes_read,
                )
                .map_err(|e| HostError::DownloadFailed(format!("WinHttpReadData failed: {e}")))?;
            }
            if bytes_read == 0 {
                break;
            }
            writer.write_all(&chunk[..bytes_read as usize])?;
            downloaded += u64::from(bytes_read);

            let now = Instant::now();
            let pct_now = total_bytes
                .filter(|&t| t > 0)
                .map(|t| ((downloaded.saturating_mul(100)) / t).min(100) as i32);
            let pct_changed = pct_now.is_some_and(|p| p != last_pct);
            if pct_changed || now.duration_since(last_report) >= Duration::from_millis(350) {
                last_report = now;
                let elapsed_s = now.duration_since(download_start).as_secs_f64();
                let msg = format_download_progress(downloaded, total_bytes, elapsed_s);
                if let (Some(total), Some(pct)) = (total_bytes.filter(|&t| t > 0), pct_now) {
                    last_pct = pct;
                    let frac = (downloaded as f32 / total as f32).clamp(0.0, 1.0);
                    on_progress(&msg, Some(frac));
                } else {
                    on_progress(&msg, None);
                }
            }
        }

        writer.flush()?;
        writer.get_ref().sync_all()?;
    }

    if target.exists() {
        let _ = std::fs::remove_file(target);
    }
    std::fs::rename(&tmp_path, target)?;
    tmp_guard.disarm();
    on_progress("Downloading 100% · Finishing…", Some(1.0));
    Ok(())
}

#[cfg(not(windows))]
fn probe_remote_content_length(url: &str) -> Option<u64> {
    let out = std::process::Command::new("curl")
        .args([
            "-sSLI",
            "--connect-timeout",
            "10",
            "--max-time",
            "15",
            "--user-agent",
            "Eqho/0.9",
            url,
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    // Take the final redirect's Content-Length header.
    let mut found = None;
    for line in text.lines() {
        if let Some((k, v)) = line.split_once(':')
            && k.trim().eq_ignore_ascii_case("content-length")
            && let Ok(n) = v.trim().parse::<u64>()
            && n > 0
        {
            found = Some(n);
        }
    }
    found
}

#[cfg(not(windows))]
struct UnixTempFileGuard {
    path: PathBuf,
    armed: bool,
}

#[cfg(not(windows))]
impl UnixTempFileGuard {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

#[cfg(not(windows))]
impl Drop for UnixTempFileGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[cfg(not(windows))]
fn download_https_to_file(
    url: &str,
    target: &Path,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(&str, Option<f32>),
) -> Result<(), HostError> {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    if cancel.load(Ordering::Acquire) {
        return Err(HostError::DownloadCancelled);
    }
    let _ = parse_https_url(url)?;

    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let tmp_name = format!(
        "{}.tmp.{}",
        target
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("model.bin"),
        std::process::id()
    );
    let tmp_path = target.with_file_name(tmp_name);
    if tmp_path.exists() {
        let _ = std::fs::remove_file(&tmp_path);
    }
    let mut tmp_guard = UnixTempFileGuard::new(tmp_path.clone());

    on_progress("Downloading 0% · Connecting…", Some(0.0));
    let total_bytes = probe_remote_content_length(url);
    if cancel.load(Ordering::Acquire) {
        return Err(HostError::DownloadCancelled);
    }

    let mut child = Command::new("curl")
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--connect-timeout",
            "20",
            "--user-agent",
            "Eqho/0.9",
            "--output",
        ])
        .arg(&tmp_path)
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .or_else(|_| {
            Command::new("wget")
                .args(["-q", "--tries=2", "--timeout=20", "-O"])
                .arg(&tmp_path)
                .arg(url)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
        })
        .map_err(|e| {
            HostError::DownloadFailed(format!("Neither curl nor wget could be started: {e}"))
        })?;

    let started = Instant::now();
    let mut last_bytes = 0u64;
    let status = loop {
        if cancel.load(Ordering::Acquire) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(HostError::DownloadCancelled);
        }
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) => {
                let downloaded = std::fs::metadata(&tmp_path).map(|m| m.len()).unwrap_or(0);
                if downloaded != last_bytes {
                    last_bytes = downloaded;
                    let frac = total_bytes
                        .filter(|&t| t > 0)
                        .map(|t| (downloaded as f64 / t as f64).clamp(0.0, 1.0) as f32);
                    let msg = format_download_progress(
                        downloaded,
                        total_bytes,
                        started.elapsed().as_secs_f64(),
                    );
                    on_progress(&msg, frac);
                }
                std::thread::sleep(Duration::from_millis(120));
            }
            Err(err) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(HostError::DownloadFailed(format!(
                    "download wait failed: {err}"
                )));
            }
        }
    };

    if cancel.load(Ordering::Acquire) {
        return Err(HostError::DownloadCancelled);
    }

    if !status.success() {
        let mut stderr_msg = String::new();
        if let Some(mut err_pipe) = child.stderr.take() {
            let _ = std::io::Read::read_to_string(&mut err_pipe, &mut stderr_msg);
        }
        let detail = stderr_msg.trim();
        return Err(HostError::DownloadFailed(if detail.is_empty() {
            format!("download exited with status {status}")
        } else {
            detail.to_owned()
        }));
    }

    let final_size = std::fs::metadata(&tmp_path).map(|m| m.len()).unwrap_or(0);
    if final_size < MIN_CACHED_MODEL_BYTES {
        return Err(HostError::DownloadFailed(format!(
            "downloaded model is unexpectedly small ({final_size} bytes)"
        )));
    }

    if target.exists() {
        let _ = std::fs::remove_file(target);
    }
    std::fs::rename(&tmp_path, target)?;
    tmp_guard.disarm();
    on_progress("Downloading 100% · Finishing…", Some(1.0));
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn parse_https_url_valid_cases() {
        let (h, p, path) = parse_https_url(
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin",
        )
        .unwrap();
        assert_eq!(h, "huggingface.co");
        assert_eq!(p, 443);
        assert_eq!(path, "/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin");

        let (h2, p2, path2) = parse_https_url("https://example.com:8443").unwrap();
        assert_eq!(h2, "example.com");
        assert_eq!(p2, 8443);
        assert_eq!(path2, "/");

        let (h3, p3, path3) = parse_https_url("https://cdn.example.com/file.bin?token=abc").unwrap();
        assert_eq!(h3, "cdn.example.com");
        assert_eq!(p3, 443);
        assert_eq!(path3, "/file.bin?token=abc");
    }

    #[test]
    fn parse_https_url_rejects_invalid() {
        assert!(parse_https_url("http://huggingface.co/file.bin").is_err());
        assert!(parse_https_url("https://").is_err());
        assert!(parse_https_url("https:///path").is_err());
        assert!(parse_https_url("https://user@example.com/path").is_err());
        assert!(parse_https_url("https://example.com:0/path").is_err());
        assert!(parse_https_url("https://example.com:99999/path").is_err());
        assert!(parse_https_url("https://example.com/with space").is_err());
    }

    #[test]
    fn format_eta_and_download_progress() {
        assert_eq!(format_eta(2.0), "< 5s left");
        assert_eq!(format_eta(42.0), "~42s left");
        assert_eq!(format_eta(125.0), "~2m 05s left");
        assert_eq!(format_eta(3660.0), "~1h 01m left");
        assert_eq!(format_bytes(500 * 1_048_576), "500 MB");
        assert_eq!(format_bytes(1_624_555_275), "1.51 GB");

        let line = format_download_progress(500 * 1_048_576, Some(1_000 * 1_048_576), 50.0);
        assert!(line.contains("Downloading 50%"), "got: {line}");
        assert!(line.contains("10.0 MB/s"), "got: {line}");
        assert!(line.contains("~50s left"), "got: {line}");
    }

    #[test]
    fn cached_model_file_returns_without_network() {
        let tmp = std::env::temp_dir().join(format!("eqho_dl_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let model_path = whispercpp_model_path(&tmp, "tiny");
        std::fs::create_dir_all(model_path.parent().unwrap()).unwrap();
        std::fs::write(&model_path, vec![0u8; (MIN_CACHED_MODEL_BYTES + 1) as usize]).unwrap();

        let mut progress_called = false;
        let res = download_ggml_model("tiny", &tmp, &mut |_, _| {
            progress_called = true;
        });
        assert_eq!(res.unwrap(), model_path);
        assert!(!progress_called);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn cancellable_download_honours_cancel_flag() {
        let tmp = std::env::temp_dir().join(format!("eqho_dl_cancel_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let cancel = AtomicBool::new(true);
        let err = download_ggml_model_cancellable("tiny", &tmp, &cancel, &mut |_, _| {}).unwrap_err();
        assert!(matches!(err, HostError::DownloadCancelled));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[cfg(not(windows))]
    #[test]
    fn non_windows_rejects_non_https_download_url() {
        let tmp = std::env::temp_dir().join(format!("eqho_dl_nonwin_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let target = tmp.join("ggml-tiny.bin");
        let cancel = AtomicBool::new(false);
        let err =
            download_https_to_file("http://example.invalid/model.bin", &target, &cancel, &mut |_, _| {})
                .unwrap_err();
        assert!(matches!(err, HostError::InvalidUrl(_)), "unexpected error: {err:?}");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[cfg(not(windows))]
    #[test]
    fn non_windows_creates_missing_parent_directory_before_download() {
        let tmp = std::env::temp_dir().join(format!("eqho_dl_mkdir_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let nested_parent = tmp.join("whispercpp");
        let target = nested_parent.join("ggml-tiny.bin");
        let cancel = AtomicBool::new(false);
        // Cancel inside the very first progress callback so no network download runs, while still
        // verifying `create_dir_all(target.parent())` ran before probing/spawning curl.
        let err = download_https_to_file(
            "https://example.invalid/ggml-tiny.bin",
            &target,
            &cancel,
            &mut |_, _| {
                cancel.store(true, Ordering::Release);
            },
        )
        .unwrap_err();
        assert!(
            matches!(err, HostError::DownloadCancelled),
            "expected DownloadCancelled, got: {err:?}"
        );
        assert!(
            nested_parent.is_dir(),
            "parent directory must be created before curl runs"
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
