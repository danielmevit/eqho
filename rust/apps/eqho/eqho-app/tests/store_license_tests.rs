//! Integration tests for Eqho's Microsoft Store 15-day trial licensing logic, offline cache,
//! flaky-Store guard, coordinator dictation suspension, and feature-flag gating.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use damt_platform::Clock;
use damt_platform_win::{RawStoreLicense, StorePurchaseOutcome};
use eqho_app::license::{
    CachedLicense, LICENSE_RECHECK_INTERVAL, LicenseCache, LicenseManager, LicenseState,
    STORE_FEATURE_ENABLED, STORE_ID, StoreLicenseTexts, format_trial_days_left, map_raw_license,
};
use eqho_core::history::TranscriptHistory;
use eqho_core::settings::Settings;
use eqho_engine::{DictationCoordinator, SpeechBackend, UiEventSink};
use eqho_host::{HostSegment, ModelHostError, ModelInfo};
use eqho_platform::{
    AudioBackend, AudioBlockCallback, AudioDeviceInfo, AudioStreamHandle, ClipboardService,
    FocusTracker, GpuAdapter, GpuProbe, HotkeyCallback, HotkeyRegistration, HotkeyService,
    HotkeySpec, InputInjector, MemoryProbe, MicPrivacyProbe, ModifierKey, PlatformError,
    VolumeControl, WindowHandle,
};
use jiff::Timestamp;
use jiff::tz::TimeZone;

/// 100 ns ticks from 1601-01-01T00:00:00Z to 1970-01-01T00:00:00Z.
const FILETIME_UNIX_EPOCH_TICKS: i64 = 116_444_736_000_000_000;
const TICKS_PER_SECOND: i64 = 10_000_000;
const BASE_UNIX_SECS: i64 = 1_800_000_000; // 2027-01-15T08:00:00Z

fn unix_secs_to_filetime(unix_secs: i64) -> i64 {
    FILETIME_UNIX_EPOCH_TICKS + unix_secs * TICKS_PER_SECOND
}

fn ts(unix_secs: i64) -> Timestamp {
    Timestamp::new(unix_secs, 0).expect("valid unix timestamp")
}

struct FakeClock {
    unix_secs: Mutex<i64>,
}

impl FakeClock {
    fn new(unix_secs: i64) -> Arc<Self> {
        Arc::new(Self {
            unix_secs: Mutex::new(unix_secs),
        })
    }

    fn advance_secs(&self, delta: i64) {
        *self.unix_secs.lock().unwrap() += delta;
    }

    fn now_ts(&self) -> Timestamp {
        ts(*self.unix_secs.lock().unwrap())
    }
}

impl Clock for FakeClock {
    fn now(&self) -> jiff::ZonedDateTime {
        self.now_ts().to_zoned_datetime(TimeZone::UTC)
    }
}

fn temp_test_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "eqho_store_license_{tag}_{}_{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn feature_flag_matches_compile_time_cfg() {
    assert_eq!(STORE_FEATURE_ENABLED, cfg!(feature = "store"));
    assert!(!STORE_ID.trim().is_empty());
}

#[test]
fn allows_dictation_only_blocks_definite_trial_expired() {
    let now = ts(BASE_UNIX_SECS);
    let future = ts(BASE_UNIX_SECS + 3 * 86_400);

    assert!(LicenseState::Full.allows_dictation());
    assert!(LicenseState::Trial { ends_at: future }.allows_dictation());
    assert!(LicenseState::Unknown.allows_dictation());
    assert!(!LicenseState::TrialExpired.allows_dictation());

    assert!(!LicenseState::Full.is_trial_expired());
    assert!(!LicenseState::Trial { ends_at: future }.is_trial_expired());
    assert!(!LicenseState::Unknown.is_trial_expired());
    assert!(LicenseState::TrialExpired.is_trial_expired());

    assert_eq!(LicenseState::Full.ui_state_tag(), "full");
    assert_eq!(LicenseState::Trial { ends_at: future }.ui_state_tag(), "trial");
    assert_eq!(LicenseState::TrialExpired.ui_state_tag(), "trial-expired");
    assert_eq!(LicenseState::Unknown.ui_state_tag(), "unknown");
    assert_eq!(LicenseState::Trial { ends_at: future }.resolve_at(now), LicenseState::Trial { ends_at: future });
}

#[test]
fn resolve_at_turns_elapsed_trial_into_expired_without_store_call() {
    let ends_at = ts(BASE_UNIX_SECS + 3_600);
    let trial = LicenseState::Trial { ends_at };
    assert_eq!(trial.resolve_at(ts(BASE_UNIX_SECS)), trial);
    assert_eq!(trial.resolve_at(ends_at), LicenseState::TrialExpired);
    assert_eq!(
        trial.resolve_at(ts(BASE_UNIX_SECS + 7_200)),
        LicenseState::TrialExpired
    );
}

#[test]
fn trial_days_left_rounds_up_partial_days() {
    let now = ts(BASE_UNIX_SECS);
    // 1 second left still shows 1 day left.
    assert_eq!(
        LicenseState::Trial {
            ends_at: ts(BASE_UNIX_SECS + 1)
        }
        .trial_days_left(now),
        Some(1)
    );
    // 12 hours left -> 1 day.
    assert_eq!(
        LicenseState::Trial {
            ends_at: ts(BASE_UNIX_SECS + 12 * 3_600)
        }
        .trial_days_left(now),
        Some(1)
    );
    // Exactly 24h -> 1 day; 24h + 1s -> 2 days.
    assert_eq!(
        LicenseState::Trial {
            ends_at: ts(BASE_UNIX_SECS + 86_400)
        }
        .trial_days_left(now),
        Some(1)
    );
    assert_eq!(
        LicenseState::Trial {
            ends_at: ts(BASE_UNIX_SECS + 86_401)
        }
        .trial_days_left(now),
        Some(2)
    );
    // 15 days -> 15.
    assert_eq!(
        LicenseState::Trial {
            ends_at: ts(BASE_UNIX_SECS + 15 * 86_400)
        }
        .trial_days_left(now),
        Some(15)
    );
    assert_eq!(LicenseState::TrialExpired.trial_days_left(now), Some(0));
    assert_eq!(LicenseState::Full.trial_days_left(now), None);
    assert_eq!(LicenseState::Unknown.trial_days_left(now), None);
    assert_eq!(LicenseState::Full.trial_days_left_i32(now), -1);
    assert_eq!(LicenseState::Unknown.trial_days_left_i32(now), -1);
}

#[test]
fn raw_license_mapping_covers_all_four_rows_sentinels_and_flaky_guard() {
    let now = ts(BASE_UNIX_SECS);
    let future_end = BASE_UNIX_SECS + 10 * 86_400;
    let past_end = BASE_UNIX_SECS - 86_400;

    // 1. Active + not trial -> Full
    let full = map_raw_license(
        RawStoreLicense {
            is_active: true,
            is_trial: false,
            expiration_universal_time: 0,
            trial_time_remaining_100ns: 0,
        },
        now,
        None,
    );
    assert_eq!(full, LicenseState::Full);

    // 2. Active + trial with valid future expiration -> Trial
    let trial = map_raw_license(
        RawStoreLicense {
            is_active: true,
            is_trial: true,
            expiration_universal_time: unix_secs_to_filetime(future_end),
            trial_time_remaining_100ns: 0,
        },
        now,
        None,
    );
    assert_eq!(
        trial,
        LicenseState::Trial {
            ends_at: ts(future_end)
        }
    );

    // 2b. Active + trial with WinRT far-future sentinel (year 9999) falls back to TrialTimeRemaining
    let sentinel_trial = map_raw_license(
        RawStoreLicense {
            is_active: true,
            is_trial: true,
            expiration_universal_time: i64::MAX,
            trial_time_remaining_100ns: 5 * 86_400 * TICKS_PER_SECOND,
        },
        now,
        None,
    );
    assert_eq!(
        sentinel_trial,
        LicenseState::Trial {
            ends_at: ts(BASE_UNIX_SECS + 5 * 86_400)
        }
    );

    // 2c. Active + trial with neither expiration nor remaining time in future -> TrialExpired
    let expired_zero = map_raw_license(
        RawStoreLicense {
            is_active: true,
            is_trial: true,
            expiration_universal_time: unix_secs_to_filetime(past_end),
            trial_time_remaining_100ns: 0,
        },
        now,
        None,
    );
    assert_eq!(expired_zero, LicenseState::TrialExpired);

    // 3. Inactive + trial -> TrialExpired
    let inactive_trial = map_raw_license(
        RawStoreLicense {
            is_active: false,
            is_trial: true,
            expiration_universal_time: 0,
            trial_time_remaining_100ns: 0,
        },
        now,
        None,
    );
    assert_eq!(inactive_trial, LicenseState::TrialExpired);

    // 4a. Inactive + not trial + past expiration -> TrialExpired
    let inactive_past = map_raw_license(
        RawStoreLicense {
            is_active: false,
            is_trial: false,
            expiration_universal_time: unix_secs_to_filetime(past_end),
            trial_time_remaining_100ns: 0,
        },
        now,
        None,
    );
    assert_eq!(inactive_past, LicenseState::TrialExpired);

    // 4b. Inactive + not trial + no valid expiration -> Unknown (when no previous expired)
    let unknown = map_raw_license(
        RawStoreLicense {
            is_active: false,
            is_trial: false,
            expiration_universal_time: 0,
            trial_time_remaining_100ns: 0,
        },
        now,
        None,
    );
    assert_eq!(unknown, LicenseState::Unknown);

    // 5. Flaky-Store guard: Unknown cannot undo TrialExpired or a past cached Trial
    let guarded_from_expired = map_raw_license(
        RawStoreLicense {
            is_active: false,
            is_trial: false,
            expiration_universal_time: 0,
            trial_time_remaining_100ns: 0,
        },
        now,
        Some(LicenseState::TrialExpired),
    );
    assert_eq!(guarded_from_expired, LicenseState::TrialExpired);

    let guarded_from_past_trial = map_raw_license(
        RawStoreLicense {
            is_active: false,
            is_trial: false,
            expiration_universal_time: 0,
            trial_time_remaining_100ns: 0,
        },
        now,
        Some(LicenseState::Trial {
            ends_at: ts(past_end),
        }),
    );
    assert_eq!(guarded_from_past_trial, LicenseState::TrialExpired);
}

#[test]
fn cache_atomic_save_load_and_corrupt_tolerance() {
    let dir = temp_test_dir("cache");
    let cache_path = dir.join("license.json");
    let cache = LicenseCache::new(&cache_path);
    assert!(cache.load().is_none());

    let record = CachedLicense {
        schema_version: 1,
        state: LicenseState::Trial {
            ends_at: ts(BASE_UNIX_SECS + 15 * 86_400),
        },
        checked_at: ts(BASE_UNIX_SECS),
    };
    cache.save(&record).unwrap();
    assert_eq!(cache.load(), Some(record));
    assert!(!dir.join("license.json.tmp").exists());

    // Corrupt file safely returns None (never panics).
    fs::write(&cache_path, "{broken json").unwrap();
    assert!(cache.load().is_none());

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn manager_never_caches_unknown_keeps_cached_offline_and_enforces_4h_recheck() {
    let dir = temp_test_dir("mgr");
    let cache_path = dir.join("license.json");
    let clock = FakeClock::new(BASE_UNIX_SECS);

    let mut mgr = LicenseManager::new(Some(cache_path.clone()), clock.clone());
    assert_eq!(mgr.state(), LicenseState::Unknown);
    assert!(mgr.is_recheck_due());

    // Unknown result never writes license.json
    let st = mgr.on_query_result(Ok(RawStoreLicense {
        is_active: false,
        is_trial: false,
        expiration_universal_time: 0,
        trial_time_remaining_100ns: 0,
    }));
    assert_eq!(st, LicenseState::Unknown);
    assert!(!cache_path.exists());
    assert!(!mgr.is_recheck_due());

    // Advance just shy of 4 hours -> not due yet; advance to 4 hours -> due.
    let almost_4h = i64::try_from(LICENSE_RECHECK_INTERVAL.as_secs()).unwrap() - 1;
    clock.advance_secs(almost_4h);
    assert!(!mgr.is_recheck_due());
    clock.advance_secs(1);
    assert!(mgr.is_recheck_due());

    // Active 15-day trial persists to license.json
    let ends_at = clock.now_ts().as_second() + 15 * 86_400;
    let st = mgr.on_query_result(Ok(RawStoreLicense {
        is_active: true,
        is_trial: true,
        expiration_universal_time: unix_secs_to_filetime(ends_at),
        trial_time_remaining_100ns: 0,
    }));
    assert_eq!(
        st,
        LicenseState::Trial {
            ends_at: ts(ends_at)
        }
    );
    assert!(cache_path.exists());

    // Offline error keeps cached active trial without locking user out
    let st = mgr.on_query_result(Err("offline".to_owned()));
    assert_eq!(
        st,
        LicenseState::Trial {
            ends_at: ts(ends_at)
        }
    );
    assert_eq!(mgr.last_error(), Some("offline"));

    // New manager instance at startup reads cache and resolves to TrialExpired once 15 days pass
    clock.advance_secs(16 * 86_400);
    let mut mgr2 = LicenseManager::new(Some(cache_path.clone()), clock.clone());
    assert_eq!(mgr2.state(), LicenseState::TrialExpired);

    // Purchase succeeds -> immediately Full and persisted
    assert!(StorePurchaseOutcome::Succeeded.is_owned());
    assert!(StorePurchaseOutcome::AlreadyPurchased.is_owned());
    assert!(!StorePurchaseOutcome::NotPurchased.is_owned());
    assert_eq!(mgr2.on_purchase_succeeded(), LicenseState::Full);
    assert_eq!(mgr2.state(), LicenseState::Full);

    let mgr3 = LicenseManager::new(Some(cache_path), clock);
    assert_eq!(mgr3.state(), LicenseState::Full);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn store_copy_plurals_and_zero_github_links() {
    assert_eq!(format_trial_days_left(1), "1 day left in your trial");
    assert_eq!(format_trial_days_left(15), "15 days left in your trial");
    assert_eq!(format_trial_days_left(0), "0 days left in your trial");
    assert_eq!(format_trial_days_left(-3), "0 days left in your trial");

    let t = StoreLicenseTexts::for_days(Some(12));
    assert_eq!(t.trial_days_left_text, "12 days left in your trial");
    assert_eq!(t.buy_label, "Buy Eqho");
    assert_eq!(t.made_with_slint_label, "Made with Slint");
    for s in [
        &t.trial_ended_title,
        &t.trial_ended_body,
        &t.buy_label,
        &t.trial_days_left_text,
        &t.made_with_slint_label,
    ] {
        assert!(
            !s.to_ascii_lowercase().contains("github"),
            "Store UI copy must never mention GitHub: {s}"
        );
    }

    let empty_days = StoreLicenseTexts::for_days(None);
    assert!(empty_days.trial_days_left_text.is_empty());
}

// --- Coordinator suspension test (TrialExpired quietly disables hotkey & dictation) ---

struct FakeHotkeyReg {
    dropped: Arc<AtomicBool>,
}

impl HotkeyRegistration for FakeHotkeyReg {}

impl Drop for FakeHotkeyReg {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::Release);
    }
}

struct FakePlatform {
    register_calls: AtomicUsize,
    last_reg_dropped: Mutex<Option<Arc<AtomicBool>>>,
}

impl FakePlatform {
    fn new() -> Self {
        Self {
            register_calls: AtomicUsize::new(0),
            last_reg_dropped: Mutex::new(None),
        }
    }

    fn is_hotkey_registered(&self) -> bool {
        self.last_reg_dropped
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|flag| !flag.load(Ordering::Acquire))
    }
}

impl AudioBackend for FakePlatform {
    fn list_input_devices(&self) -> Vec<AudioDeviceInfo> {
        Vec::new()
    }
    fn start_capture(
        &self,
        _device_index: Option<usize>,
        _on_block: AudioBlockCallback,
    ) -> Result<Box<dyn AudioStreamHandle>, PlatformError> {
        Err(PlatformError("fake audio".to_owned()))
    }
    fn play_pcm(
        &self,
        _samples: &[f32],
        _sample_rate: u32,
        _blocking: bool,
    ) -> Result<(), PlatformError> {
        Ok(())
    }
}

impl HotkeyService for FakePlatform {
    fn register(
        &self,
        _spec: &HotkeySpec,
        _callback: HotkeyCallback,
    ) -> Result<Box<dyn HotkeyRegistration>, PlatformError> {
        self.register_calls.fetch_add(1, Ordering::AcqRel);
        let flag = Arc::new(AtomicBool::new(false));
        *self.last_reg_dropped.lock().unwrap() = Some(Arc::clone(&flag));
        Ok(Box::new(FakeHotkeyReg { dropped: flag }))
    }
}

impl ClipboardService for FakePlatform {
    fn get_text(&self) -> Option<String> {
        None
    }
    fn set_text(&self, _text: &str, _private: bool) -> Result<(), PlatformError> {
        Ok(())
    }
}

impl FocusTracker for FakePlatform {
    fn get_foreground_window(&self) -> WindowHandle {
        WindowHandle::NONE
    }
    fn restore_foreground_window(&self, _window: WindowHandle) -> bool {
        false
    }
    fn get_window_process_name(&self, _window: WindowHandle) -> Option<String> {
        None
    }
}

impl InputInjector for FakePlatform {
    fn held_modifiers(&self) -> Option<Vec<ModifierKey>> {
        Some(Vec::new())
    }
    fn release_modifiers(&self, _mods: &[ModifierKey]) -> Result<(), PlatformError> {
        Ok(())
    }
    fn send_paste_shortcut(&self) -> Result<(), PlatformError> {
        Ok(())
    }
    fn type_unicode_text(&self, _text: &str) -> Result<(), PlatformError> {
        Ok(())
    }
    fn send_backspaces(&self, _count: usize) -> Result<(), PlatformError> {
        Ok(())
    }
}

impl VolumeControl for FakePlatform {
    fn is_supported(&self) -> bool {
        false
    }
    fn get_volume(&self) -> Option<f32> {
        Some(1.0)
    }
    fn set_volume(&self, _level: f32) -> bool {
        true
    }
    fn set_mute(&self, _muted: bool) -> bool {
        true
    }
}

impl GpuProbe for FakePlatform {
    fn list_adapters(&self) -> Vec<GpuAdapter> {
        Vec::new()
    }
    fn cpu_has_avx2(&self) -> bool {
        true
    }
}

impl MemoryProbe for FakePlatform {
    fn free_memory_mb(&self) -> Option<u64> {
        Some(8192)
    }
}

impl MicPrivacyProbe for FakePlatform {
    fn mic_access_blocked(&self) -> Option<bool> {
        Some(false)
    }
    fn open_mic_privacy_settings(&self) -> bool {
        true
    }
}

struct NoopSpeech;
impl SpeechBackend for NoopSpeech {
    fn is_ready(&mut self, _model_key: &str, _use_gpu: bool) -> bool {
        false
    }
    fn load_model(
        &mut self,
        _model_key: &str,
        _model_dir: &Path,
        _use_gpu: bool,
        _threads: u32,
        _on_progress: &mut dyn FnMut(&str, Option<f32>),
    ) -> Result<ModelInfo, ModelHostError> {
        Err(ModelHostError::Protocol("noop".to_owned()))
    }
    fn transcribe(
        &mut self,
        _audio: &[f32],
        _language: &str,
        _beam_size: u32,
        _initial_prompt: &str,
    ) -> Result<Vec<HostSegment>, ModelHostError> {
        Ok(Vec::new())
    }
    fn info(&self) -> Option<&ModelInfo> {
        None
    }
    fn shutdown(&mut self) {}
}

struct RecordingUi {
    notify_count: AtomicUsize,
    overlay_shows: AtomicUsize,
}

impl RecordingUi {
    fn new() -> Self {
        Self {
            notify_count: AtomicUsize::new(0),
            overlay_shows: AtomicUsize::new(0),
        }
    }
}

impl UiEventSink for RecordingUi {
    fn show_overlay(&self, _text: &str) {
        self.overlay_shows.fetch_add(1, Ordering::AcqRel);
    }
    fn update_overlay_text(&self, _text: &str) {}
    fn set_overlay_level(&self, _level: f32) {}
    fn set_overlay_waveform(&self, _loud: f32, _tone: f32) {}
    fn hide_overlay(&self) {}
    fn set_tray_active(&self, _active: bool) {}
    fn notify(&self, _message: &str) {
        self.notify_count.fetch_add(1, Ordering::AcqRel);
    }
    fn on_model_progress(&self, _model_key: &str, _message: &str, _fraction: Option<f32>) {}
    fn on_recovery_available(&self, _text: Option<&str>) {}
    fn on_dictation_committed(&self, _text: &str) {}
    fn on_history_changed(&self) {}
}

#[test]
fn trial_expired_quietly_suspends_dictation_and_re_enables_on_purchase() {
    let dir = temp_test_dir("coord");
    let platform = Arc::new(FakePlatform::new());
    let ui = Arc::new(RecordingUi::new());
    let coord = DictationCoordinator::new(
        Settings::default(),
        dir.join("settings.json"),
        TranscriptHistory::with_default_cap(dir.join("history.jsonl")),
        platform.clone(),
        Box::new(NoopSpeech),
        ui.clone(),
    );

    // Normal state: hotkey registers cleanly.
    coord.register_hotkey().unwrap();
    assert!(platform.is_hotkey_registered());
    assert!(!coord.is_dictation_suspended());

    // Trial ends: hotkey is quietly unregistered, activate/toggle are silent no-ops (no nag!).
    coord.set_dictation_suspended(true);
    assert!(coord.is_dictation_suspended());
    assert!(!platform.is_hotkey_registered());

    coord.activate();
    coord.toggle();
    assert_eq!(ui.notify_count.load(Ordering::Acquire), 0);
    assert_eq!(ui.overlay_shows.load(Ordering::Acquire), 0);

    // Re-calling register_hotkey while suspended does NOT re-register the hook.
    let reg_before = platform.register_calls.load(Ordering::Acquire);
    coord.register_hotkey().unwrap();
    assert_eq!(platform.register_calls.load(Ordering::Acquire), reg_before);
    assert!(!platform.is_hotkey_registered());

    // Purchase completes -> suspended becomes false and hotkey re-registers immediately.
    coord.set_dictation_suspended(false);
    assert!(!coord.is_dictation_suspended());
    assert!(platform.is_hotkey_registered());

    coord.shutdown();
    let _ = fs::remove_dir_all(&dir);
}
