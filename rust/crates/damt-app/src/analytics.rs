//! Opt-in anonymous product analytics (PostHog EU Cloud) shared across damt.xyz apps.
//!
//! Privacy & performance guarantees:
//! - **Strictly opt-in:** does nothing and spawns zero background threads while `enabled` is `false`
//!   or when no build-time API key is present (community/source builds stay 100% silent).
//! - **Stateless & IP-free:** every capture payload sets `"$process_person_profile": false` and
//!   `"$ip": null` targeting PostHog EU Cloud (`https://eu.i.posthog.com/capture/`) by default.
//! - **Non-blocking & offline-safe:** uses a bounded channel (`try_send`) and a short 5-second HTTPS
//!   timeout so network calls never block UI or dictation and silently drop when offline.
//! - **Zero extra crate dependencies:** uses native `WinHttp` on Windows and stdin-piped `curl` on
//!   Unix (so API keys never appear in process arguments).

use std::collections::hash_map::RandomState;
use std::fmt::Write as _;
use std::hash::{BuildHasher, Hasher};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

/// Default GDPR-friendly PostHog EU Cloud ingest host.
pub const DEFAULT_POSTHOG_EU_HOST: &str = "https://eu.i.posthog.com";

const QUEUE_CAPACITY: usize = 64;

static UUID_SEQ: AtomicU64 = AtomicU64::new(1);

/// Typed JSON value for anonymous analytics event properties.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnalyticsProp {
    Str(String),
    Int(i64),
    Bool(bool),
}

impl From<&str> for AnalyticsProp {
    fn from(v: &str) -> Self {
        Self::Str(v.to_owned())
    }
}

impl From<String> for AnalyticsProp {
    fn from(v: String) -> Self {
        Self::Str(v)
    }
}

impl From<i64> for AnalyticsProp {
    fn from(v: i64) -> Self {
        Self::Int(v)
    }
}

impl From<bool> for AnalyticsProp {
    fn from(v: bool) -> Self {
        Self::Bool(v)
    }
}

/// Configuration for [`AnalyticsClient`].
#[derive(Debug, Clone)]
pub struct AnalyticsConfig {
    /// PostHog project write-only API key (`phc_...`), injected at build time.
    /// When `None` or empty, analytics is completely inert.
    pub api_key: Option<String>,
    /// HTTPS PostHog ingest host (defaults to [`DEFAULT_POSTHOG_EU_HOST`]).
    pub host: String,
    /// Short application identifier (e.g. `"eqho"`).
    pub app_name: String,
    /// Semantic version (e.g. `"0.9.5"`).
    pub app_version: String,
    /// Distribution channel (`"direct"` or `"store"`).
    pub edition: String,
    /// Whether the user has explicitly opted into anonymous usage analytics.
    pub enabled: bool,
    /// Local random anonymous UUID v4 (never derived from hardware or user identity).
    pub distinct_id: String,
}

impl AnalyticsConfig {
    /// Build a safe default configuration (opted out, EU Cloud host).
    #[must_use]
    pub fn new(
        api_key: Option<&str>,
        app_name: impl Into<String>,
        app_version: impl Into<String>,
        edition: impl Into<String>,
    ) -> Self {
        let cleaned_key = api_key
            .map(str::trim)
            .filter(|k| !k.is_empty())
            .map(str::to_owned);
        Self {
            api_key: cleaned_key,
            host: DEFAULT_POSTHOG_EU_HOST.to_owned(),
            app_name: app_name.into(),
            app_version: app_version.into(),
            edition: edition.into(),
            enabled: false,
            distinct_id: String::new(),
        }
    }
}

/// Generate a random RFC 4122 UUID v4 string without hardware or user identity inputs.
#[must_use]
pub fn generate_anonymous_id() -> String {
    let mut bytes = [0u8; 16];
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = u64::from(std::process::id());
    let seq = UUID_SEQ.fetch_add(1, Ordering::Relaxed);

    let mut h1 = RandomState::new().build_hasher();
    h1.write_u128(nanos);
    h1.write_u64(pid);
    h1.write_u64(seq);
    let hi = h1.finish();

    let mut h2 = RandomState::new().build_hasher();
    h2.write_u64(hi.rotate_left(17));
    h2.write_u64(seq.wrapping_mul(0x9E37_79B9_7F4A_7C15));
    let lo = h2.finish();

    bytes[..8].copy_from_slice(&hi.to_le_bytes());
    bytes[8..].copy_from_slice(&lo.to_le_bytes());

    // RFC 4122 version 4 and variant 10xx bits.
    bytes[6] = (bytes[6] & 0x0F) | 0x40;
    bytes[8] = (bytes[8] & 0x3F) | 0x80;

    let mut out = String::with_capacity(36);
    for (idx, b) in bytes.iter().enumerate() {
        if matches!(idx, 4 | 6 | 8 | 10) {
            out.push('-');
        }
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

/// Validate that `raw` is a public HTTPS base URL and return `(host, port, capture_path)`.
///
/// Rejects plain HTTP, userinfo credentials, whitespace/control chars, loopback addresses,
/// and RFC 1918 / link-local private IPv4 hosts.
#[must_use]
pub fn parse_https_capture_target(raw: &str) -> Option<(String, u16, String)> {
    let trimmed = raw.trim().trim_end_matches('/');
    let rest = trimmed.strip_prefix("https://")?;
    if rest.is_empty()
        || rest.contains('@')
        || rest.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return None;
    }

    let (authority, base_path) = match rest.find('/') {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => (rest, ""),
    };
    if authority.is_empty() || authority.starts_with('[') {
        return None;
    }

    let (host_raw, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (h, p.parse::<u16>().ok()?),
        None => (authority, 443),
    };
    let host = host_raw.to_ascii_lowercase();
    if host.is_empty() || !host.contains('.') || is_disallowed_host(&host) {
        return None;
    }

    let clean_base = base_path.trim_end_matches('/');
    let path = format!("{clean_base}/capture/");
    Some((host, port, path))
}

fn is_disallowed_host(host: &str) -> bool {
    if host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local") {
        return true;
    }
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() == 4
        && let (Ok(a), Ok(b), Ok(_c), Ok(_d)) = (
            parts[0].parse::<u8>(),
            parts[1].parse::<u8>(),
            parts[2].parse::<u8>(),
            parts[3].parse::<u8>(),
        )
    {
        return a == 0
            || a == 10
            || a == 127
            || (a == 169 && b == 254)
            || (a == 172 && (16..=31).contains(&b))
            || (a == 192 && b == 168);
    }
    false
}

fn push_json_escaped(out: &mut String, s: &str) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Build the PostHog `/capture/` JSON payload with stateless, IP-free, GeoIP-free privacy flags.
#[must_use]
pub fn build_capture_payload(
    api_key: &str,
    distinct_id: &str,
    event: &str,
    props: &[(&str, AnalyticsProp)],
) -> String {
    let mut out = String::with_capacity(320);
    out.push_str("{\"api_key\":");
    push_json_escaped(&mut out, api_key);
    out.push_str(",\"event\":");
    push_json_escaped(&mut out, event);
    out.push_str(",\"distinct_id\":");
    push_json_escaped(&mut out, distinct_id);
    out.push_str(
        ",\"properties\":{\"$process_person_profile\":false,\"$ip\":null,\"$geoip_disable\":true",
    );
    for (k, v) in props {
        if *k == "$process_person_profile" || *k == "$ip" || *k == "$geoip_disable" {
            continue;
        }
        out.push(',');
        push_json_escaped(&mut out, k);
        out.push(':');
        match v {
            AnalyticsProp::Str(s) => push_json_escaped(&mut out, s),
            AnalyticsProp::Int(i) => {
                let _ = write!(&mut out, "{i}");
            }
            AnalyticsProp::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        }
    }
    out.push_str("}}");
    out
}

fn posthog_os_label() -> &'static str {
    match std::env::consts::OS {
        "windows" => "Windows",
        "linux" => "Linux",
        "macos" => "Mac OS X",
        "android" => "Android",
        "ios" => "iOS",
        other => other,
    }
}

fn posthog_device_type() -> &'static str {
    match std::env::consts::OS {
        "android" | "ios" => "Mobile",
        _ => "Desktop",
    }
}

#[derive(Debug)]
struct TargetEndpoint {
    #[cfg(windows)]
    host: String,
    #[cfg(windows)]
    port: u16,
    #[cfg(windows)]
    path: String,
    #[cfg(not(windows))]
    full_url: String,
}

#[derive(Debug)]
struct AnalyticsInner {
    api_key: Option<String>,
    endpoint: Option<TargetEndpoint>,
    app_name: String,
    app_version: String,
    edition: String,
    enabled: AtomicBool,
    distinct_id: Mutex<String>,
    tx: Mutex<Option<SyncSender<String>>>,
}

/// Thread-safe, non-blocking, strictly opt-in PostHog EU analytics client.
#[derive(Debug, Clone)]
pub struct AnalyticsClient {
    inner: Arc<AnalyticsInner>,
}

impl AnalyticsClient {
    /// Create a new client. Does not spawn a background thread until both an API key is configured
    /// and the user has opted in.
    #[must_use]
    pub fn new(config: AnalyticsConfig) -> Self {
        let endpoint = parse_https_capture_target(&config.host).map(|(host, port, path)| {
            #[cfg(windows)]
            {
                TargetEndpoint { host, port, path }
            }
            #[cfg(not(windows))]
            {
                let full_url = if port == 443 {
                    format!("https://{host}{path}")
                } else {
                    format!("https://{host}:{port}{path}")
                };
                TargetEndpoint { full_url }
            }
        });
        let distinct_id = if config.enabled && config.distinct_id.trim().is_empty() {
            generate_anonymous_id()
        } else {
            config.distinct_id
        };
        let client = Self {
            inner: Arc::new(AnalyticsInner {
                api_key: config.api_key,
                endpoint,
                app_name: config.app_name,
                app_version: config.app_version,
                edition: config.edition,
                enabled: AtomicBool::new(config.enabled),
                distinct_id: Mutex::new(distinct_id),
                tx: Mutex::new(None),
            }),
        };
        if config.enabled {
            client.ensure_worker_started();
        }
        client
    }

    /// Create a permanently disabled no-op client (for tests, `--smoke`, or isolated demo runs).
    #[must_use]
    pub fn disabled() -> Self {
        Self::new(AnalyticsConfig::new(None, "app", "0.0.0", "direct"))
    }

    /// Returns `true` when a build-time API key is present and the ingest host is valid.
    #[must_use]
    pub fn is_configured(&self) -> bool {
        self.inner.api_key.is_some() && self.inner.endpoint.is_some()
    }

    /// Returns `true` when the user has opted in and the client is configured.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.is_configured() && self.inner.enabled.load(Ordering::Acquire)
    }

    /// Update the user's opt-in state at runtime. When enabling with an empty ID, generates a fresh
    /// random UUID v4.
    pub fn set_enabled(&self, enabled: bool, distinct_id: Option<&str>) {
        self.inner.enabled.store(enabled, Ordering::Release);
        {
            let mut id_guard = self
                .inner
                .distinct_id
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if let Some(id) = distinct_id {
                *id_guard = id.to_owned();
            } else if enabled && id_guard.trim().is_empty() {
                *id_guard = generate_anonymous_id();
            } else if !enabled {
                id_guard.clear();
            }
        }
        if enabled {
            self.ensure_worker_started();
        }
    }

    /// Enqueue an anonymous event if and only if the user has opted in and a build-time key exists.
    ///
    /// Never blocks; silently drops if offline or if the queue is full.
    pub fn capture(&self, event: &str, extra_props: &[(&str, AnalyticsProp)]) {
        if !self.is_active() || event.trim().is_empty() {
            return;
        }
        let Some(api_key) = self.inner.api_key.as_deref() else {
            return;
        };
        let distinct_id = {
            let mut guard = self
                .inner
                .distinct_id
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if guard.trim().is_empty() {
                *guard = generate_anonymous_id();
            }
            guard.clone()
        };

        let mut props: Vec<(&str, AnalyticsProp)> = Vec::with_capacity(extra_props.len() + 9);
        props.push(("$lib", AnalyticsProp::from("damt-analytics")));
        props.push(("$lib_version", AnalyticsProp::from(env!("CARGO_PKG_VERSION"))));
        props.push(("$os", AnalyticsProp::from(posthog_os_label())));
        props.push(("$device_type", AnalyticsProp::from(posthog_device_type())));
        props.push(("app", AnalyticsProp::from(self.inner.app_name.as_str())));
        props.push((
            "app_version",
            AnalyticsProp::from(self.inner.app_version.as_str()),
        ));
        props.push(("edition", AnalyticsProp::from(self.inner.edition.as_str())));
        props.push(("os", AnalyticsProp::from(std::env::consts::OS)));
        props.push(("arch", AnalyticsProp::from(std::env::consts::ARCH)));
        for (k, v) in extra_props {
            props.push((k, v.clone()));
        }

        let payload = build_capture_payload(api_key, &distinct_id, event, &props);
        self.ensure_worker_started();
        if let Some(tx) = self
            .inner
            .tx
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            let _ = tx.try_send(payload);
        }
    }

    fn ensure_worker_started(&self) {
        if !self.is_configured() {
            return;
        }
        let mut guard = self.inner.tx.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_some() {
            return;
        }
        let (tx, rx) = mpsc::sync_channel::<String>(QUEUE_CAPACITY);
        let inner = Arc::clone(&self.inner);
        let spawned = thread::Builder::new()
            .name("damt-analytics".to_owned())
            .spawn(move || {
                while let Ok(payload) = rx.recv() {
                    if !inner.enabled.load(Ordering::Acquire) {
                        continue;
                    }
                    if let Some(endpoint) = inner.endpoint.as_ref() {
                        post_json_best_effort(endpoint, &payload);
                    }
                }
            });
        if spawned.is_ok() {
            *guard = Some(tx);
        }
    }
}

#[cfg(windows)]
fn post_json_best_effort(endpoint: &TargetEndpoint, json_body: &str) {
    use std::ffi::c_void;
    use windows::Win32::Networking::WinHttp::{
        WINHTTP_ACCESS_TYPE_DEFAULT_PROXY, WINHTTP_FLAG_SECURE, WINHTTP_OPTION_REDIRECT_POLICY,
        WINHTTP_OPTION_REDIRECT_POLICY_DISALLOW_HTTPS_TO_HTTP, WinHttpCloseHandle, WinHttpConnect,
        WinHttpOpen, WinHttpOpenRequest, WinHttpReceiveResponse, WinHttpSendRequest,
        WinHttpSetOption, WinHttpSetTimeouts,
    };
    use windows::core::PCWSTR;

    struct HInternetGuard(*mut c_void);
    impl Drop for HInternetGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: Valid WinHTTP handle owned exclusively by this guard.
                unsafe {
                    let _ = WinHttpCloseHandle(self.0);
                }
            }
        }
    }

    fn to_wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    let agent_w = to_wide("damt-analytics/1.0");
    let host_w = to_wide(&endpoint.host);
    let path_w = to_wide(&endpoint.path);
    let verb_w = to_wide("POST");
    let headers_w = to_wide("Content-Type: application/json\r\n");

    // SAFETY: Standard WinHTTP synchronous HTTPS POST with strict 5-second timeouts.
    unsafe {
        let session = HInternetGuard(WinHttpOpen(
            PCWSTR(agent_w.as_ptr()),
            WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
            PCWSTR::null(),
            PCWSTR::null(),
            0,
        ));
        if session.0.is_null() {
            return;
        }
        let redirect_policy: u32 = WINHTTP_OPTION_REDIRECT_POLICY_DISALLOW_HTTPS_TO_HTTP;
        let _ = WinHttpSetOption(
            Some(session.0),
            WINHTTP_OPTION_REDIRECT_POLICY,
            Some(&redirect_policy.to_ne_bytes()),
        );
        let _ = WinHttpSetTimeouts(session.0, 5_000, 5_000, 5_000, 5_000);

        let connect =
            HInternetGuard(WinHttpConnect(session.0, PCWSTR(host_w.as_ptr()), endpoint.port, 0));
        if connect.0.is_null() {
            return;
        }
        let request = HInternetGuard(WinHttpOpenRequest(
            connect.0,
            PCWSTR(verb_w.as_ptr()),
            PCWSTR(path_w.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            WINHTTP_FLAG_SECURE,
        ));
        if request.0.is_null() {
            return;
        }
        let body = json_body.as_bytes();
        let Ok(body_len) = u32::try_from(body.len()) else {
            return;
        };
        let hdr_slice = &headers_w[..headers_w.len().saturating_sub(1)];
        if WinHttpSendRequest(
            request.0,
            Some(hdr_slice),
            Some(body.as_ptr().cast()),
            body_len,
            body_len,
            0,
        )
        .is_ok()
        {
            let _ = WinHttpReceiveResponse(request.0, std::ptr::null_mut());
        }
    }
}

#[cfg(not(windows))]
fn post_json_best_effort(endpoint: &TargetEndpoint, json_body: &str) {
    use std::io::Write;
    use std::process::{Command, Stdio};

    // Pipe payload over stdin so the PostHog key never appears in process command-line arguments.
    let Ok(mut child) = Command::new("curl")
        .args([
            "-fsS",
            "--max-time",
            "5",
            "--proto",
            "=https",
            "-H",
            "Content-Type: application/json",
            "--data-binary",
            "@-",
            &endpoint.full_url,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return;
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(json_body.as_bytes());
    }
    let _ = child.wait();
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn generates_distinct_valid_uuid_v4_ids() {
        let a = generate_anonymous_id();
        let b = generate_anonymous_id();
        assert_ne!(a, b);
        for id in [&a, &b] {
            assert_eq!(id.len(), 36);
            let parts: Vec<&str> = id.split('-').collect();
            assert_eq!(parts.len(), 5);
            assert_eq!(parts[0].len(), 8);
            assert_eq!(parts[1].len(), 4);
            assert_eq!(parts[2].len(), 4);
            assert_eq!(parts[3].len(), 4);
            assert_eq!(parts[4].len(), 12);
            assert!(parts[2].starts_with('4'), "expected v4 UUID, got {id}");
            let variant = parts[3].chars().next().unwrap();
            assert!(
                matches!(variant, '8' | '9' | 'a' | 'b'),
                "expected RFC 4122 variant, got {id}"
            );
        }
    }

    #[test]
    fn validates_https_hosts_and_blocks_private_or_http() {
        assert_eq!(
            parse_https_capture_target(DEFAULT_POSTHOG_EU_HOST),
            Some(("eu.i.posthog.com".to_owned(), 443, "/capture/".to_owned()))
        );
        assert_eq!(
            parse_https_capture_target("https://eu.i.posthog.com/"),
            Some(("eu.i.posthog.com".to_owned(), 443, "/capture/".to_owned()))
        );
        assert_eq!(
            parse_https_capture_target("https://analytics.damt.xyz:8443/ph"),
            Some((
                "analytics.damt.xyz".to_owned(),
                8443,
                "/ph/capture/".to_owned()
            ))
        );

        for bad in [
            "http://eu.i.posthog.com",
            "https://localhost",
            "https://127.0.0.1",
            "https://169.254.169.254",
            "https://10.0.0.1",
            "https://192.168.1.5",
            "https://172.16.0.1",
            "https://user:pass@eu.i.posthog.com",
            "https://eu.i.posthog.com/foo bar",
            "",
        ] {
            assert!(
                parse_https_capture_target(bad).is_none(),
                "expected rejection for {bad}"
            );
        }
    }

    #[test]
    fn payload_includes_privacy_flags_and_escapes_strings() {
        let json = build_capture_payload(
            "phc_test_key",
            "550e8400-e29b-41d4-a716-446655440000",
            "dictation_completed",
            &[
                ("model", AnalyticsProp::from("small.en-q5")),
                ("threads", AnalyticsProp::from(8)),
                ("gpu", AnalyticsProp::from(true)),
                ("note", AnalyticsProp::from("line1\n\"quoted\"\\end")),
                // Caller cannot override mandatory privacy flags:
                ("$ip", AnalyticsProp::from("1.2.3.4")),
                ("$process_person_profile", AnalyticsProp::from(true)),
                ("$geoip_disable", AnalyticsProp::from(false)),
            ],
        );
        assert!(json.contains("\"$process_person_profile\":false"));
        assert!(json.contains("\"$ip\":null"));
        assert!(json.contains("\"$geoip_disable\":true"));
        assert!(!json.contains("\"$geoip_disable\":false"));
        assert!(!json.contains("1.2.3.4"));
        assert!(json.contains("\"model\":\"small.en-q5\""));
        assert!(json.contains("\"threads\":8"));
        assert!(json.contains("\"gpu\":true"));
        assert!(json.contains("\"note\":\"line1\\n\\\"quoted\\\"\\\\end\""));
    }

    #[test]
    fn opted_out_or_unconfigured_client_is_completely_inert() {
        let disabled = AnalyticsClient::disabled();
        assert!(!disabled.is_configured());
        assert!(!disabled.is_active());
        disabled.capture("app_started", &[]);
        assert!(disabled.inner.tx.lock().unwrap().is_none());

        let opted_out = AnalyticsClient::new(AnalyticsConfig::new(
            Some("phc_fake"),
            "eqho",
            "0.9.5",
            "direct",
        ));
        assert!(opted_out.is_configured());
        assert!(!opted_out.is_active());
        opted_out.capture("app_started", &[]);
        assert!(opted_out.inner.tx.lock().unwrap().is_none());
    }
}
