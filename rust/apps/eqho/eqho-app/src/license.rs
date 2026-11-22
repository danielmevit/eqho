//! Microsoft Store license state, mapping, `%APPDATA%\Eqho` offline cache, demo mode, and purchase policy.
//!
//! Offline / error policy:
//! - On startup, Eqho loads the last known [`LicenseState`] cached in `<config_dir>/license.json`
//!   before querying `Windows.Services.Store.StoreContext::GetDefault()?.GetAppLicenseAsync()`.
//! - If the Store query fails (offline, Store service unavailable, or transient error), Eqho keeps
//!   using the cached state so a user with a valid paid or active trial licence is never locked out
//!   while offline. If a cached `Trial { ends_at }` has already passed `ends_at`, it resolves to
//!   [`LicenseState::TrialExpired`].
//! - On a first run while offline (no cached state yet), the state is [`LicenseState::Unknown`],
//!   which behaves as an active trial (`allows_dictation() == true`) until the Store can be reached.
//! - Live Store checks run on startup, every [`LICENSE_RECHECK_INTERVAL`] (4 hours), on OS resume
//!   (`SessionEvent::Resumed`), and immediately after a Store purchase completes.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use damt_platform::Clock;
use damt_platform_win::{RawStoreLicense, StoreLicenseKind, StoreLicenseMapper};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};

/// Compile-time flag indicating whether the `store` Cargo feature is enabled.
pub const STORE_FEATURE_ENABLED: bool = cfg!(feature = "store");

/// Placeholder Microsoft Store product ID for `StoreContext::RequestPurchaseAsync`.
///
/// Replace this with Eqho's reserved Partner Center Store ID before building the Store `.msix` release.
pub const STORE_ID: &str = "9NBLGGH4XXXX";

/// Periodic Store license re-check cadence (4 hours).
pub const LICENSE_RECHECK_INTERVAL: Duration = Duration::from_secs(4 * 60 * 60);

/// Cache file name inside the per-user app data directory (`%APPDATA%\Eqho\license.json`).
pub const LICENSE_CACHE_FILE_NAME: &str = "license.json";

/// Mapped Microsoft Store license state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum LicenseState {
    /// Active full (purchased) license.
    Full,
    /// Active 15-day free trial ending at `ends_at`.
    Trial { ends_at: Timestamp },
    /// Free trial has ended; dictation stops quietly until purchased.
    TrialExpired,
    /// License state is unknown (e.g. first run offline); behaves like an active trial.
    Unknown,
}

impl LicenseState {
    /// Returns `true` when dictation is allowed (`Full`, `Trial`, or `Unknown`).
    /// Only a definite [`LicenseState::TrialExpired`] stops dictation.
    #[must_use]
    pub const fn allows_dictation(self) -> bool {
        !matches!(self, Self::TrialExpired)
    }

    /// Returns `true` when the trial has expired.
    #[must_use]
    pub const fn is_trial_expired(self) -> bool {
        matches!(self, Self::TrialExpired)
    }

    /// Stable string tag exposed to Slint (`"full"`, `"trial"`, `"trial-expired"`, `"unknown"`).
    #[must_use]
    pub const fn ui_state_tag(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Trial { .. } => "trial",
            Self::TrialExpired => "trial-expired",
            Self::Unknown => "unknown",
        }
    }

    /// Remaining whole days in an active trial (rounded up so `< 24h` shows `1`), `Some(0)` when
    /// expired, and `None` for `Full` or `Unknown`.
    #[must_use]
    pub fn trial_days_left(self, now: Timestamp) -> Option<i32> {
        match self {
            Self::Trial { ends_at } => {
                let remaining_secs = ends_at.as_second().saturating_sub(now.as_second());
                if remaining_secs <= 0 {
                    Some(0)
                } else {
                    let days = u64::try_from(remaining_secs)
                        .unwrap_or(u64::MAX)
                        .div_ceil(86_400);
                    Some(i32::try_from(days).unwrap_or(i32::MAX))
                }
            }
            Self::TrialExpired => Some(0),
            Self::Full | Self::Unknown => None,
        }
    }

    /// Slint-friendly integer representation of `trial-days-left` (`-1` when not in a known trial).
    #[must_use]
    pub fn trial_days_left_i32(self, now: Timestamp) -> i32 {
        self.trial_days_left(now).unwrap_or(-1)
    }

    /// Evaluates `self` at `now`, turning an expired `Trial { ends_at }` into `TrialExpired`.
    #[must_use]
    pub fn resolve_at(self, now: Timestamp) -> Self {
        match self {
            Self::Trial { ends_at } if now >= ends_at => Self::TrialExpired,
            other => other,
        }
    }
}

/// Maps a WinRT [`RawStoreLicense`] at `now` (with optional `previous` cached state) into
/// [`LicenseState`].
#[must_use]
pub fn map_raw_license(
    raw: RawStoreLicense,
    now: Timestamp,
    previous: Option<LicenseState>,
) -> LicenseState {
    match StoreLicenseMapper::map(raw, now.as_second()) {
        StoreLicenseKind::Full => LicenseState::Full,
        StoreLicenseKind::Trial { ends_at_unix_secs } => Timestamp::new(ends_at_unix_secs, 0)
            .map_or(LicenseState::Unknown, |ends_at| {
                LicenseState::Trial { ends_at }.resolve_at(now)
            }),
        StoreLicenseKind::TrialExpired => LicenseState::TrialExpired,
        StoreLicenseKind::Unknown => match previous {
            Some(LicenseState::TrialExpired) => LicenseState::TrialExpired,
            Some(LicenseState::Trial { ends_at }) if now >= ends_at => LicenseState::TrialExpired,
            _ => LicenseState::Unknown,
        },
    }
}

/// Store trial / purchase copy (calm tone, zero GitHub links).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreLicenseTexts {
    pub trial_ended_title: String,
    pub trial_ended_body: String,
    pub buy_label: String,
    pub trial_days_left_text: String,
    pub made_with_slint_label: String,
}

impl StoreLicenseTexts {
    /// Build Store trial and purchase strings for `days_left` (`None` leaves `trial_days_left_text` empty).
    #[must_use]
    pub fn for_days(days_left: Option<i32>) -> Self {
        let trial_days_left_text = days_left
            .filter(|&d| d >= 0)
            .map(format_trial_days_left)
            .unwrap_or_default();
        Self {
            trial_ended_title: "Your trial has ended.".to_owned(),
            trial_ended_body: "If you'd like to keep using Eqho, please buy it.".to_owned(),
            buy_label: "Buy Eqho".to_owned(),
            trial_days_left_text,
            made_with_slint_label: "Made with Slint".to_owned(),
        }
    }
}

/// Format `"{days} day(s) left in your trial"` with English singular/plural grammar.
#[must_use]
pub fn format_trial_days_left(days: i32) -> String {
    let clamped = days.max(0);
    if clamped == 1 {
        "1 day left in your trial".to_owned()
    } else {
        format!("{clamped} days left in your trial")
    }
}

/// Hidden demo license state for visual verification without a Microsoft Store package.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DemoLicense {
    Full,
    Trial,
    TrialLastDay,
    TrialEnded,
    Unknown,
}

impl DemoLicense {
    /// Parse a `--demo-license` CLI argument value (case-insensitive).
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "full" => Some(Self::Full),
            "trial" => Some(Self::Trial),
            "trial-last-day" => Some(Self::TrialLastDay),
            "trial-ended" => Some(Self::TrialEnded),
            "unknown" => Some(Self::Unknown),
            _ => None,
        }
    }
}

/// Pure mapping from a [`DemoLicense`] value to a concrete [`LicenseState`] at `now`.
#[must_use]
pub fn demo_license_state(demo: DemoLicense, now: Timestamp) -> LicenseState {
    match demo {
        DemoLicense::Full => LicenseState::Full,
        DemoLicense::Unknown => LicenseState::Unknown,
        DemoLicense::TrialEnded => LicenseState::TrialExpired,
        DemoLicense::Trial => Timestamp::new(now.as_second().saturating_add(5 * 86_400), 0)
            .map_or(LicenseState::Unknown, |ends_at| LicenseState::Trial {
                ends_at,
            }),
        DemoLicense::TrialLastDay => {
            Timestamp::new(now.as_second().saturating_add(12 * 3_600), 0).map_or(
                LicenseState::Unknown,
                |ends_at| LicenseState::Trial { ends_at },
            )
        }
    }
}

/// `--demo-license` is ONLY honoured in isolated demo runs of a `store`-feature build.
#[must_use]
pub const fn demo_license_is_honoured(is_isolated: bool) -> bool {
    is_isolated && STORE_FEATURE_ENABLED
}

/// Persisted license cache payload (`<config_dir>/license.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CachedLicense {
    pub schema_version: u32,
    pub state: LicenseState,
    pub checked_at: Timestamp,
}

/// Atomic reader/writer for `license.json`.
#[derive(Debug, Clone)]
pub struct LicenseCache {
    path: PathBuf,
}

impl LicenseCache {
    /// Creates a cache handle backed by `path` (e.g. `<data_dir>/license.json`).
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Returns the underlying file path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Loads the cached license record, returning `None` when missing, unreadable, or corrupt.
    #[must_use]
    pub fn load(&self) -> Option<CachedLicense> {
        let raw = fs::read_to_string(&self.path).ok()?;
        serde_json::from_str::<CachedLicense>(&raw).ok()
    }

    /// Atomically writes `record` (`*.tmp` then `rename`).
    pub fn save(&self, record: &CachedLicense) -> Result<(), std::io::Error> {
        if let Some(parent) = self.path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(record)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;

        let mut tmp_os = OsString::from(self.path.as_os_str());
        tmp_os.push(".tmp");
        let tmp_path = PathBuf::from(tmp_os);

        fs::write(&tmp_path, json.as_bytes())?;
        fs::rename(&tmp_path, &self.path)?;
        Ok(())
    }
}

/// Stateful license coordinator managing cache loading, Store query reconciliation, offline
/// fallback, and periodic re-check timing.
pub struct LicenseManager {
    cache: Option<LicenseCache>,
    clock: Arc<dyn Clock>,
    state: LicenseState,
    last_known: Option<LicenseState>,
    last_check_attempt: Option<Timestamp>,
    last_error: Option<String>,
}

impl std::fmt::Debug for LicenseManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LicenseManager")
            .field("state", &self.state)
            .field("last_known", &self.last_known)
            .field("last_check_attempt", &self.last_check_attempt)
            .finish_non_exhaustive()
    }
}

impl LicenseManager {
    /// Initializes a [`LicenseManager`], reading any cached license from `cache_path` (if given).
    /// When no cache is present, starts in [`LicenseState::Unknown`] (behaves as active trial).
    #[must_use]
    pub fn new(cache_path: Option<PathBuf>, clock: Arc<dyn Clock>) -> Self {
        let now = clock.now().timestamp();
        let cache = cache_path.map(LicenseCache::new);
        let cached = cache.as_ref().and_then(LicenseCache::load);
        let last_known = cached.as_ref().map(|c| c.state);
        let state = last_known.map_or(LicenseState::Unknown, |s| s.resolve_at(now));
        Self {
            cache,
            clock,
            state,
            last_known,
            last_check_attempt: None,
            last_error: None,
        }
    }

    /// Current effective [`LicenseState`] evaluated against the clock.
    #[must_use]
    pub fn state(&self) -> LicenseState {
        self.state.resolve_at(self.clock.now().timestamp())
    }

    /// Most recent Store query error message, if any.
    #[must_use]
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// Marks that a Store license query attempt has been started now (used to space periodic checks).
    pub fn mark_check_started(&mut self) {
        self.last_check_attempt = Some(self.clock.now().timestamp());
    }

    /// Returns `true` when [`LICENSE_RECHECK_INTERVAL`] has elapsed since the last check attempt.
    #[must_use]
    pub fn is_recheck_due(&self) -> bool {
        let Some(last) = self.last_check_attempt else {
            return true;
        };
        let elapsed = self.clock.now().timestamp().duration_since(last);
        Duration::try_from(elapsed).is_ok_and(|d| d >= LICENSE_RECHECK_INTERVAL)
    }

    /// Applies the outcome of `GetAppLicenseAsync()` (or an offline/service error), returning
    /// the updated [`LicenseState`].
    ///
    /// On `Err`, falls back to the last known cached state (never locking out an offline user with
    /// a valid licence) or [`LicenseState::Unknown`] on first run offline.
    pub fn on_query_result(&mut self, result: Result<RawStoreLicense, String>) -> LicenseState {
        let now = self.clock.now().timestamp();
        self.last_check_attempt = Some(now);

        match result {
            Ok(raw) => {
                self.last_error = None;
                let mapped = map_raw_license(raw, now, self.last_known);
                self.state = mapped;
                if !matches!(mapped, LicenseState::Unknown) {
                    self.last_known = Some(mapped);
                    self.persist_state(mapped, now);
                }
            }
            Err(err) => {
                self.last_error = Some(err);
                let fallback = self.last_known.or_else(|| {
                    self.cache
                        .as_ref()
                        .and_then(LicenseCache::load)
                        .map(|c| c.state)
                });
                self.state = fallback.map_or(LicenseState::Unknown, |s| s.resolve_at(now));
            }
        }
        self.state
    }

    /// Records an immediate transition to [`LicenseState::Full`] after `RequestPurchaseAsync`
    /// reports `Succeeded` or `AlreadyPurchased`, persisting the state before the follow-up query.
    pub fn on_purchase_succeeded(&mut self) -> LicenseState {
        let now = self.clock.now().timestamp();
        self.last_error = None;
        self.state = LicenseState::Full;
        self.last_known = Some(LicenseState::Full);
        self.persist_state(LicenseState::Full, now);
        self.state
    }

    fn persist_state(&mut self, state: LicenseState, now: Timestamp) {
        if let Some(cache) = &self.cache
            && let Err(err) = cache.save(&CachedLicense {
                schema_version: 1,
                state,
                checked_at: now,
            })
        {
            self.last_error = Some(format!("failed to save license cache: {err}"));
        }
    }
}
