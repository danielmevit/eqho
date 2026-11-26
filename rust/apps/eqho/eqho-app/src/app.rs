//! Full live desktop application controller (`run_desktop_app`).

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::rc::Rc;
#[cfg(feature = "store")]
use std::rc::Weak as RcWeak;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use damt_app::{Acquire, ErrorLog, SingleInstance, SingleInstanceGuard, install_panic_hook};
#[cfg(feature = "store")]
use damt_platform::{Clock, SystemClock};
use damt_platform::PlatformServices;
use eqho_core::history::TranscriptHistory;
use eqho_core::modelpick::HardwareClass;
use eqho_core::modelstore::{MODELS, whispercpp_model_path};
use eqho_core::settings::Settings;
use eqho_core::vad::level_meter;
use eqho_engine::{DictationCoordinator, UiEventSink};
use eqho_host::downloader::download_ggml_model_cancellable;
use eqho_host::{HostError, ModelHost, ModelHostConfig};
use eqho_overlay::{
    FlagBitmapProvider, OverlayAction, OverlayActionCallback, OverlayConfig, OverlayHandle,
};
use eqho_platform::{AudioBackend, AudioDeviceInfo, AudioStreamHandle, ClipboardService, GpuProbe, MicPrivacyProbe};
use eqho_ui::{
    DashboardWindow, EqhoTray, Theme, apply_settings_to_window, blur_snapshot,
    blur_snapshot_strong, build_history_rows,
    build_history_rows_with_trash_meta, build_model_cards, build_onboarding_model_options,
    dashboard_window_title, delivery_mode_index, delivery_mode_key, engine_backend_key,
    filter_history_entries, format_active_driver_display, history_trash_days_value,
    language_code_for_index, language_index_for_code, language_warning_text, model_short_name,
    onboarding_model_copy_for_key, overlay_monitor_key, overlay_position_key,
    parse_paste_rules_text, parse_replacements_text, render_flag_rgba, split_model_cards,
    status_summary_title, theme_mode_key, ui_scale_value, volume_duck_key, wordmark_image,
};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::assets::{
    LOGO_64_DARK, LOGO_64_WHITE, build_tray_icon_rgba, decode_png_rgba, rgba_to_slint_image,
};
use crate::license::{DemoLicense, demo_license_is_honoured};
#[cfg(feature = "store")]
use crate::license::{
    LICENSE_CACHE_FILE_NAME, LicenseState, STORE_FEATURE_ENABLED, StoreLicenseTexts,
    demo_license_state,
};
use crate::paths::{RuntimePaths, resolve_dll_dir, resolve_runtime_paths, resolve_theme_is_dark};

const SINGLE_INSTANCE_ID: &str = "xyz.damt.eqho";

#[cfg(all(windows, feature = "store"))]
const STARTUP_TASK_ID: &str = "EqhoStartup";

#[cfg(windows)]
type EqhoPlatformImpl = eqho_platform_win::WinEqhoPlatform;
#[cfg(target_os = "linux")]
type EqhoPlatformImpl = eqho_platform_linux::LinuxEqhoPlatform;
#[cfg(target_os = "macos")]
type EqhoPlatformImpl = eqho_platform_macos::MacEqhoPlatform;
#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
type EqhoPlatformImpl = eqho_platform::UnsupportedEqhoPlatform;

#[cfg(windows)]
type DamtPlatformImpl = damt_platform_win::WinPlatform;
#[cfg(target_os = "linux")]
type DamtPlatformImpl = damt_platform_linux::LinuxPlatform;
#[cfg(target_os = "macos")]
type DamtPlatformImpl = damt_platform_macos::MacPlatform;
#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
type DamtPlatformImpl = damt_platform::Unsupported;

fn build_damt_platform(is_isolated: bool, error_log: &ErrorLog) -> DamtPlatformImpl {
    #[cfg(windows)]
    {
        let mut win_platform = damt_platform_win::WinPlatform::new("Eqho");
        #[cfg(feature = "store")]
        {
            win_platform = win_platform.with_startup_task_id(STARTUP_TASK_ID);
        }
        if !is_isolated {
            let notifier = damt_platform_win::WinNotifier::new(
                SINGLE_INSTANCE_ID,
                "Eqho",
                "Eqho",
                crate::assets::LOGO_32_WHITE,
            );
            win_platform = win_platform.with_notifier(notifier);
        }
        #[cfg(feature = "store")]
        if !is_isolated {
            use damt_platform::{SessionEvent, SessionEventSource};
            let dispatch_log = error_log.clone();
            win_platform.session().start(Box::new(move |event| {
                if event == SessionEvent::Resumed
                    && let Err(err) = slint::invoke_from_event_loop(store_license::refresh)
                {
                    dispatch_log.log_error("session_event_dispatch", &err);
                }
            }));
        }
        #[cfg(not(feature = "store"))]
        let _ = error_log;
        win_platform
    }
    #[cfg(target_os = "linux")]
    {
        let _ = error_log;
        let mut linux_platform = damt_platform_linux::LinuxPlatform::new("Eqho");
        if !is_isolated {
            linux_platform = linux_platform
                .with_notifier(damt_platform_linux::LinuxNotifier::new("Eqho", "eqho"));
        }
        linux_platform
    }
    #[cfg(target_os = "macos")]
    {
        let _ = error_log;
        let mut mac_platform = damt_platform_macos::MacPlatform::new(SINGLE_INSTANCE_ID);
        if !is_isolated {
            mac_platform = mac_platform.with_notifier(damt_platform_macos::MacNotifier);
        }
        mac_platform
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        let _ = (is_isolated, error_log);
        damt_platform::Unsupported
    }
}

#[cfg(feature = "store")]
fn raw_win32_hwnd(window: &slint::Window) -> Option<isize> {
    #[cfg(windows)]
    {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        use slint::winit_030::WinitWindowAccessor;
        window
            .with_winit_window(|winit_win| {
                let raw = winit_win.window_handle().ok()?;
                match raw.as_raw() {
                    RawWindowHandle::Win32(win32) => Some(win32.hwnd.get()),
                    _ => None,
                }
            })
            .flatten()
    }
    #[cfg(not(windows))]
    {
        let _ = window;
        None
    }
}

fn detect_hardware_info(platform: &dyn GpuProbe) -> (HardwareClass, Option<String>) {
    const MIN_VRAM: u64 = 2 * 1024 * 1024 * 1024;
    let best_gpu = platform
        .list_adapters()
        .into_iter()
        .filter(|a| a.dedicated_video_memory_bytes >= MIN_VRAM)
        .max_by_key(|a| a.dedicated_video_memory_bytes);
    match best_gpu {
        Some(adapter) => (HardwareClass::Gpu, Some(adapter.name)),
        None => (HardwareClass::Cpu, None),
    }
}

fn default_cpu_threads() -> u32 {
    std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(4)
        .clamp(1, 32)
}

fn format_trash_retention_label(days: i32) -> String {
    if days <= 0 {
        "Trash items stay until emptied".to_owned()
    } else {
        format!("Trash auto-clears after {days} days")
    }
}

fn cached_model_keys(model_dir: &Path) -> Vec<&'static str> {
    MODELS
        .iter()
        .filter(|m| {
            let p = whispercpp_model_path(model_dir, m.key);
            p.metadata().map(|meta| meta.is_file() && meta.len() > 1_048_576).unwrap_or(false)
        })
        .map(|m| m.key)
        .collect()
}

fn now_epoch_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

#[derive(Debug, Clone)]
struct UiSyncSnapshot {
    is_dictating: bool,
    dictation_allowed: bool,
    recovery_text: Option<String>,
    downloading_key: Option<String>,
    loading_key: Option<String>,
    download_progress: f32,
    download_detail: String,
    download_done_key: Option<String>,
    download_error_key: Option<String>,
    hotkey: String,
    light_taskbar: bool,
}

/// Thread-safe bridge from `DictationCoordinator` (and background download threads)
/// to the Slint UI thread.
struct AppUiSink {
    overlay: Arc<OverlayHandle>,
    overlay_preview_shown: Arc<AtomicBool>,
    damt_platform: Arc<DamtPlatformImpl>,
    error_log: ErrorLog,
    tray_weak: slint::Weak<EqhoTray>,
    dash_weak: Arc<Mutex<Option<slint::Weak<DashboardWindow>>>>,
    snapshot: Arc<Mutex<UiSyncSnapshot>>,
    history: TranscriptHistory,
    settings: Arc<Mutex<Settings>>,
    hw_class: HardwareClass,
}

impl AppUiSink {
    fn refresh_tray(&self) {
        let tray_weak = self.tray_weak.clone();
        let snap = self
            .snapshot
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(tray) = tray_weak.upgrade() {
                tray.set_is_dictating(snap.is_dictating);
                tray.set_dictation_allowed(snap.dictation_allowed);
                tray.set_toggle_label(
                    if snap.is_dictating {
                        "Stop Dictation"
                    } else {
                        "Start Dictation"
                    }
                    .into(),
                );
                tray.set_has_recovery(snap.recovery_text.is_some());
                let tooltip = if snap.dictation_allowed {
                    status_summary_title(snap.is_dictating, &snap.hotkey)
                } else {
                    "Your trial has ended.".to_owned()
                };
                tray.set_tray_tooltip(tooltip.into());
                if let Ok((w, h, rgba)) = build_tray_icon_rgba(snap.light_taskbar, snap.is_dictating) {
                    tray.set_tray_icon(rgba_to_slint_image(w, h, &rgba));
                }
            }
        });
    }

    fn refresh_models_in_dash(&self) {
        let dash_weak_opt = self
            .dash_weak
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let Some(dash_weak) = dash_weak_opt else {
            return;
        };
        let snap = self
            .snapshot
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let settings = self
            .settings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let hw = self.hw_class;
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(dash) = dash_weak.upgrade() {
                let model_dir = settings.resolve_model_dir();
                let cached = cached_model_keys(&model_dir);
                let cards = build_model_cards(
                    &settings.model_size,
                    &cached,
                    snap.downloading_key.as_deref(),
                    snap.loading_key.as_deref(),
                    snap.download_progress,
                    &snap.download_detail,
                    snap.download_error_key.as_deref(),
                    hw,
                    &settings.language,
                );
                let (en_cards, multi_cards) = split_model_cards(cards);
                dash.set_english_models(ModelRc::new(VecModel::from(en_cards)));
                dash.set_multilingual_models(ModelRc::new(VecModel::from(multi_cards)));
                dash.set_window_title(
                    dashboard_window_title(&settings.model_size, snap.is_dictating).into(),
                );
            }
        });
    }
    fn clear_cancelled_download(&self, model_key: &str) {
        {
            let mut snap = self.snapshot.lock().unwrap_or_else(|e| e.into_inner());
            if snap.downloading_key.as_deref() == Some(model_key) {
                snap.downloading_key = None;
                snap.download_progress = 0.0;
                snap.download_detail.clear();
                snap.download_error_key = None;
            }
        }
        self.refresh_models_in_dash();
        let dash_weak_opt = self
            .dash_weak
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(dash_weak) = dash_weak_opt {
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(dash) = dash_weak.upgrade() {
                    dash.set_bg_download_active(false);
                    dash.set_bg_download_error(false);
                    dash.set_bg_download_detail("".into());
                }
            });
        }
    }

    fn set_step6_hint(&self, text: &str) {
        let dash_weak_opt = self
            .dash_weak
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(dash_weak) = dash_weak_opt {
            let owned = text.to_owned();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(dash) = dash_weak.upgrade()
                    && dash.get_onboarding_active()
                    && dash.get_onboarding_step() == 5
                {
                    dash.set_onboarding_try_hint(owned.into());
                }
            });
        }
    }

    fn append_step6_text(&self, text: &str, hint: Option<&str>) {
        if text.trim().is_empty() {
            return;
        }
        let dash_weak_opt = self
            .dash_weak
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(dash_weak) = dash_weak_opt {
            let owned = text.to_owned();
            let hint_owned = hint.map(str::to_owned);
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(dash) = dash_weak.upgrade()
                    && dash.get_onboarding_active()
                    && dash.get_onboarding_step() == 5
                {
                    let cur = dash.get_onboarding_try_text().to_string();
                    if !cur.ends_with(&owned) && !cur.contains(&owned) {
                        let next = if cur.trim().is_empty() {
                            owned
                        } else {
                            format!("{cur} {owned}")
                        };
                        dash.set_onboarding_try_text(next.into());
                    }
                    if let Some(h) = hint_owned {
                        dash.set_onboarding_try_hint(h.into());
                    }
                }
            });
        }
    }
}

impl UiEventSink for AppUiSink {
    fn show_overlay(&self, text: &str) {
        self.overlay_preview_shown.store(false, Ordering::Release);
        self.overlay.set_finishing(false);
        self.overlay.show(text);
        self.set_step6_hint(text);
    }

    fn update_overlay_text(&self, text: &str) {
        if text == "Writing it down…" {
            self.overlay.set_finishing(true);
        }
        self.overlay.update_text(text);
        self.set_step6_hint(text);
    }

    fn set_overlay_level(&self, level: f32) {
        self.overlay.set_level(level);
    }

    fn set_overlay_waveform(&self, loud: f32, tone: f32) {
        self.overlay.set_waveform(loud, tone);
    }

    fn hide_overlay(&self) {
        self.overlay.set_finishing(false);
        self.overlay.hide();
        let dash_weak_opt = self
            .dash_weak
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(dash_weak) = dash_weak_opt {
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(dash) = dash_weak.upgrade()
                    && dash.get_onboarding_active()
                    && dash.get_onboarding_step() == 5
                {
                    let cur = dash.get_onboarding_try_hint().to_string();
                    if matches!(
                        cur.as_str(),
                        "Listening..." | "Loading model…" | "Writing it down…"
                    ) {
                        dash.set_onboarding_try_hint("".into());
                    }
                }
            });
        }
    }

    fn set_tray_active(&self, active: bool) {
        {
            let mut snap = self.snapshot.lock().unwrap_or_else(|e| e.into_inner());
            snap.is_dictating = active;
        }
        self.refresh_tray();
        let dash_weak_opt = self
            .dash_weak
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let model_size = self
            .settings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .model_size
            .clone();
        if let Some(dash_weak) = dash_weak_opt {
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(dash) = dash_weak.upgrade() {
                    dash.set_onboarding_is_dictating(active);
                    dash.set_window_title(dashboard_window_title(&model_size, active).into());
                }
            });
        }
    }

    fn notify(&self, message: &str) {
        self.error_log.log(Some("notify"), message);
        let _ = self.damt_platform.notifier().notify("Eqho", message);
        self.set_step6_hint(message);
    }

    fn on_model_progress(&self, model_key: &str, message: &str, fraction: Option<f32>) {
        let lower = message.to_ascii_lowercase();
        let is_load_error = lower.starts_with("load failed");
        let is_error = !is_load_error && lower.contains("failed");
        let is_ready = message.eq_ignore_ascii_case("ready");
        let is_loading = !is_error
            && !is_load_error
            && !is_ready
            && lower.contains("loading")
            && !lower.contains("downloading");
        let is_downloading = !is_error
            && !is_load_error
            && !is_ready
            && !is_loading
            && (lower.contains("downloading") || fraction.is_some());
        let clamped_frac = fraction.map(|f| f.clamp(0.0, 1.0));

        let (progress_val, detail_text) = {
            let mut snap = self.snapshot.lock().unwrap_or_else(|e| e.into_inner());
            if is_load_error {
                if snap.loading_key.as_deref() == Some(model_key) {
                    snap.loading_key = None;
                }
                if snap.downloading_key.as_deref() == Some(model_key) {
                    snap.downloading_key = None;
                }
                (snap.download_progress, message.to_owned())
            } else if is_error {
                snap.downloading_key = None;
                snap.loading_key = None;
                snap.download_done_key = None;
                snap.download_error_key = Some(model_key.to_owned());
                snap.download_detail = message.to_owned();
                (0.0_f32, message.to_owned())
            } else if is_ready {
                if snap.downloading_key.as_deref() == Some(model_key) {
                    snap.downloading_key = None;
                }
                if snap.loading_key.as_deref() == Some(model_key) {
                    snap.loading_key = None;
                }
                snap.download_progress = 1.0;
                snap.download_error_key = None;
                snap.download_done_key = Some(model_key.to_owned());
                snap.download_detail = "Ready".to_owned();
                (1.0_f32, "Ready".to_owned())
            } else if is_loading {
                if snap.downloading_key.as_deref() == Some(model_key) {
                    snap.downloading_key = None;
                }
                snap.loading_key = Some(model_key.to_owned());
                (1.0_f32, message.to_owned())
            } else if is_downloading {
                snap.downloading_key = Some(model_key.to_owned());
                if snap.loading_key.as_deref() == Some(model_key) {
                    snap.loading_key = None;
                }
                if let Some(frac) = clamped_frac {
                    snap.download_progress = frac;
                }
                snap.download_error_key = None;
                snap.download_done_key = None;
                snap.download_detail = message.to_owned();
                (snap.download_progress, message.to_owned())
            } else {
                (snap.download_progress, message.to_owned())
            }
        };
        self.refresh_models_in_dash();

        let dash_weak_opt = self
            .dash_weak
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(dash_weak) = dash_weak_opt {
            let model_name = model_short_name(model_key);
            let pct = (progress_val * 100.0).round().clamp(0.0, 100.0) as i32;
            let bg_active = is_downloading;
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(dash) = dash_weak.upgrade() {
                    if !is_loading {
                        dash.set_bg_download_active(bg_active);
                        dash.set_bg_download_done(is_ready);
                        dash.set_bg_download_error(is_error);
                        dash.set_bg_download_model(model_name.into());
                        dash.set_bg_download_progress(progress_val);
                        dash.set_bg_download_pct(pct);
                        dash.set_bg_download_detail(detail_text.into());
                    }
                    if is_ready {
                        dash.set_onboarding_model_cached(true);
                        dash.set_onboarding_model_ready(true);
                        dash.set_onboarding_model_loading(false);
                    }
                }
            });
        }
    }

    fn on_recovery_available(&self, text: Option<&str>) {
        {
            let mut snap = self.snapshot.lock().unwrap_or_else(|e| e.into_inner());
            snap.recovery_text = text.map(str::to_owned);
        }
        self.refresh_tray();

        if let Some(t) = text {
            self.overlay.show_recovery(t);
            self.append_step6_text(
                t,
                Some("✓ Dictation worked! Try another sentence or click Finish."),
            );
        } else {
            self.overlay.hide_recovery();
        }

        let dash_weak_opt = self
            .dash_weak
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(dash_weak) = dash_weak_opt {
            let txt = text.unwrap_or("").to_owned();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(dash) = dash_weak.upgrade() {
                    dash.set_recovery_text(txt.into());
                }
            });
        }
    }

    fn on_dictation_committed(&self, text: &str) {
        self.append_step6_text(
            text,
            Some("✓ Dictation worked! Try another sentence or click Finish."),
        );
    }

    fn on_history_changed(&self) {
        let dash_weak_opt = self
            .dash_weak
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let Some(dash_weak) = dash_weak_opt else {
            return;
        };
        let active_entries = self.history.read_all();
        let trash_entries = self.history.read_trash();
        let retention_days = self
            .settings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .history_trash_days;
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(dash) = dash_weak.upgrade() {
                dash.set_trash_count(trash_entries.len() as i32);
                dash.set_trash_retention_label(format_trash_retention_label(retention_days).into());
                let rows = if dash.get_showing_trash() {
                    build_history_rows_with_trash_meta(
                        &trash_entries,
                        retention_days,
                        now_epoch_secs(),
                    )
                } else {
                    let query = dash.get_history_query().to_string();
                    let period = dash.get_history_period_index();
                    let filtered =
                        filter_history_entries(&active_entries, &query, period, now_epoch_secs());
                    build_history_rows(&filtered)
                };
                dash.set_history_rows(ModelRc::new(VecModel::from(rows)));
            }
        });
    }

    fn on_settings_changed(&self, updated: &Settings) {
        {
            *self.settings.lock().unwrap_or_else(|e| e.into_inner()) = updated.clone();
        }
        self.overlay.update_session_meta(
            &updated.language,
            &updated.delivery_mode,
            updated.overlay_controls_pinned,
            updated.recent_languages.clone(),
        );
        let dash_weak_opt = self
            .dash_weak
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(dash_weak) = dash_weak_opt {
            let lang = updated.language.clone();
            let model_size = updated.model_size.clone();
            let delivery = updated.delivery_mode.clone();
            let controls_enabled = updated.overlay_controls_enabled;
            let controls_pinned = updated.overlay_controls_pinned;
            let hw = self.hw_class;
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(dash) = dash_weak.upgrade() {
                    dash.set_language_index(language_index_for_code(&lang));
                    dash.set_language_warning(language_warning_text(&lang, &model_size, hw).into());
                    dash.set_delivery_mode_index(delivery_mode_index(&delivery));
                    dash.set_overlay_controls_enabled(controls_enabled);
                    dash.set_overlay_controls_pinned(controls_pinned);
                }
            });
        }
        self.refresh_models_in_dash();
    }
}

const OVERLAY_PREVIEW_TEXT: &str =
    "Live preview — drag any slider to tune pill size, transparency, and text.";

struct AppController {
    settings: Arc<Mutex<Settings>>,
    settings_path: PathBuf,
    config_dir: PathBuf,
    history: TranscriptHistory,
    coordinator: DictationCoordinator,
    overlay: Arc<OverlayHandle>,
    overlay_preview_shown: Arc<AtomicBool>,
    overlay_preview_timer: slint::Timer,
    platform: Arc<EqhoPlatformImpl>,
    damt_platform: Arc<DamtPlatformImpl>,
    hw_class: HardwareClass,
    gpu_name: Option<String>,
    dashboard: Option<DashboardWindow>,
    dash_weak: Arc<Mutex<Option<slint::Weak<DashboardWindow>>>>,
    snapshot: Arc<Mutex<UiSyncSnapshot>>,
    mic_devices: Vec<AudioDeviceInfo>,
    mic_test_stream: Option<Box<dyn AudioStreamHandle>>,
    hotkey_ok: bool,
    is_isolated: bool,
    hotkey_capture_cancel: Option<Arc<AtomicBool>>,
    download_cancel: Option<Arc<AtomicBool>>,
    error_log: ErrorLog,
    ui_sink: Arc<AppUiSink>,
    #[cfg(feature = "store")]
    license_state: LicenseState,
    #[cfg(feature = "store")]
    license_now: jiff::Timestamp,
    #[cfg(feature = "store")]
    demo_license_active: bool,
}

impl AppController {
    #[cfg(feature = "store")]
    fn sync_license_to_dash(&self) {
        let Some(dash) = &self.dashboard else {
            return;
        };
        let resolved = self.license_state.resolve_at(self.license_now);
        let days_left = resolved.trial_days_left(self.license_now);
        let texts = StoreLicenseTexts::for_days(days_left);
        dash.set_store_build(STORE_FEATURE_ENABLED);
        dash.set_license_state(resolved.ui_state_tag().into());
        dash.set_trial_days_left(resolved.trial_days_left_i32(self.license_now));
        dash.set_trial_ended_title(texts.trial_ended_title.into());
        dash.set_trial_ended_body(texts.trial_ended_body.into());
        dash.set_buy_label(texts.buy_label.into());
        dash.set_trial_days_left_text(texts.trial_days_left_text.into());
        dash.set_made_with_slint_label(texts.made_with_slint_label.into());
    }

    #[cfg(feature = "store")]
    fn apply_license_state(&mut self, state: LicenseState, now: jiff::Timestamp) {
        let resolved = state.resolve_at(now);
        self.license_state = resolved;
        self.license_now = now;
        let allowed = resolved.allows_dictation();
        self.coordinator.set_dictation_suspended(!allowed);
        {
            let mut snap = self.snapshot.lock().unwrap_or_else(|e| e.into_inner());
            snap.dictation_allowed = allowed;
            if !allowed {
                snap.is_dictating = false;
            }
        }
        self.ui_sink.refresh_tray();
        self.sync_license_to_dash();
    }

    fn save_settings(&self, reload_model: bool) {
        let s = self.settings.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Err(err) = s.save_to_path(&self.settings_path) {
            self.error_log.log_error("save_settings", &err);
        }
        self.coordinator.apply_settings(s, reload_model);
    }

    fn resolved_is_dark(&self) -> bool {
        let theme = self
            .settings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .theme
            .clone();
        let sys_light = self.damt_platform.app_theme().is_light_app_theme();
        resolve_theme_is_dark(&theme, sys_light)
    }

    fn apply_theme_and_overlay(&self) {
        let is_dark = self.resolved_is_dark();
        let s = self.settings.lock().unwrap_or_else(|e| e.into_inner()).clone();
        self.overlay
            .update_config(OverlayConfig::from_settings(&s, is_dark));
        if let Some(dash) = &self.dashboard {
            dash.global::<Theme>().set_dark(is_dark);
            dash.set_logo_image(wordmark_image(is_dark));
            let icon_bytes = if is_dark { LOGO_64_DARK } else { LOGO_64_WHITE };
            if let Ok((w, h, rgba)) = decode_png_rgba(icon_bytes) {
                dash.set_window_icon(rgba_to_slint_image(w, h, &rgba));
            }
        }
        self.sync_overlay_preview();
    }

    fn stop_overlay_preview(&self) {
        self.overlay_preview_timer.stop();
        if self.overlay_preview_shown.swap(false, Ordering::AcqRel) {
            let is_dictating = self
                .snapshot
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_dictating;
            if !is_dictating {
                self.overlay.set_finishing(false);
                self.overlay.hide();
            }
        }
    }

    fn sync_overlay_preview(&self) {
        let is_dictating = self
            .snapshot
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_dictating;
        let s = self.settings.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let should_show = self.dashboard.as_ref().is_some_and(|d| {
            !d.get_onboarding_active()
                && d.get_settings_open()
                && d.get_settings_section().as_str() == "overlay"
                && d.get_overlay_preview_active()
        }) && s.overlay_enabled
            && !is_dictating;

        if !should_show {
            self.stop_overlay_preview();
            return;
        }

        let was_shown = self.overlay_preview_shown.swap(true, Ordering::AcqRel);
        self.overlay.update_session_meta(
            &s.language,
            &s.delivery_mode,
            s.overlay_controls_pinned,
            s.recent_languages.clone(),
        );
        let preview_text = if s.overlay_show_text {
            OVERLAY_PREVIEW_TEXT
        } else {
            ""
        };
        if !was_shown {
            self.overlay.set_finishing(false);
            self.overlay.show(preview_text);
            let ov = Arc::clone(&self.overlay);
            let active_flag = Arc::clone(&self.overlay_preview_shown);
            let tick = Rc::new(RefCell::new(0u64));
            self.overlay_preview_timer.start(
                slint::TimerMode::Repeated,
                std::time::Duration::from_millis(60),
                move || {
                    if !active_flag.load(Ordering::Acquire) {
                        return;
                    }
                    let mut n = tick.borrow_mut();
                    *n = n.wrapping_add(1);
                    let t = (*n as f32) * 0.16;
                    let loud = 0.24 + 0.26 * (t.sin() * 0.65 + (t * 2.3).cos() * 0.35).abs();
                    let tone = 0.5 + 0.35 * (t * 0.7).sin();
                    ov.set_waveform(loud, tone);
                },
            );
        } else if s.overlay_show_text {
            self.overlay.update_text(OVERLAY_PREVIEW_TEXT);
        }
    }

    fn refresh_active_engine_display(&self) {
        let Some(dash) = &self.dashboard else {
            return;
        };
        let backend = self
            .settings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .engine_backend
            .clone();
        let driver = format_active_driver_display(
            &backend,
            self.hw_class,
            self.gpu_name.as_deref(),
            default_cpu_threads(),
        );
        dash.set_active_engine_display("whisper.cpp 1.8.4".into());
        dash.set_active_driver_display(driver.into());
    }

    fn refresh_history(&self) {
        let Some(dash) = &self.dashboard else {
            return;
        };
        let retention_days = self
            .settings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .history_trash_days;
        let trash_entries = self.history.read_trash();
        dash.set_trash_count(trash_entries.len() as i32);
        dash.set_trash_retention_label(format_trash_retention_label(retention_days).into());

        let rows = if dash.get_showing_trash() {
            build_history_rows_with_trash_meta(&trash_entries, retention_days, now_epoch_secs())
        } else {
            let entries = self.history.read_all();
            let query = dash.get_history_query().to_string();
            let period = dash.get_history_period_index();
            let filtered = filter_history_entries(&entries, &query, period, now_epoch_secs());
            build_history_rows(&filtered)
        };
        dash.set_history_rows(ModelRc::new(VecModel::from(rows)));
    }

    fn refresh_models(&self) {
        let Some(dash) = &self.dashboard else {
            return;
        };
        let s = self.settings.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let snap = self.snapshot.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let model_dir = s.resolve_model_dir();
        let cached = cached_model_keys(&model_dir);
        let cards = build_model_cards(
            &s.model_size,
            &cached,
            snap.downloading_key.as_deref(),
            snap.loading_key.as_deref(),
            snap.download_progress,
            &snap.download_detail,
            snap.download_error_key.as_deref(),
            self.hw_class,
            &s.language,
        );
        let (en_cards, multi_cards) = split_model_cards(cards);
        dash.set_english_models(ModelRc::new(VecModel::from(en_cards)));
        dash.set_multilingual_models(ModelRc::new(VecModel::from(multi_cards)));
        dash.set_window_title(dashboard_window_title(&s.model_size, snap.is_dictating).into());
        self.refresh_onboarding_models(None);
    }

    fn refresh_onboarding_models(&self, selected_override: Option<&str>) {
        let Some(dash) = &self.dashboard else {
            return;
        };
        let s = self.settings.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let model_dir = s.resolve_model_dir();
        let cached = cached_model_keys(&model_dir);
        let desired = selected_override
            .map(str::to_owned)
            .unwrap_or_else(|| {
                let cur = dash.get_onboarding_selected_model().to_string();
                if cur.is_empty() {
                    eqho_core::modelpick::recommend_model(self.hw_class, &s.language).to_owned()
                } else {
                    cur
                }
            });
        let options = build_onboarding_model_options(self.hw_class, &s.language, &desired, &cached);
        let active_key = options
            .iter()
            .find(|o| o.is_selected)
            .map(|o| o.key.to_string())
            .unwrap_or(desired);
        let is_cached = cached.contains(&active_key.as_str());
        let (summary, url) = onboarding_model_copy_for_key(&active_key);
        dash.set_onboarding_model_options(ModelRc::new(VecModel::from(options)));
        dash.set_onboarding_selected_model(active_key.clone().into());
        dash.set_onboarding_selected_model_name(model_short_name(&active_key).into());
        dash.set_onboarding_model_summary(summary.into());
        dash.set_onboarding_model_url(url.into());
        dash.set_onboarding_model_cached(is_cached);
    }

    fn ensure_hotkey_registered(&mut self) {
        if !self.is_isolated && !self.hotkey_ok {
            match self.coordinator.register_hotkey() {
                Ok(()) => self.hotkey_ok = true,
                Err(err) => self.error_log.log(Some("register_hotkey_retry"), &err),
            }
        }
        if let Some(dash) = &self.dashboard {
            dash.set_onboarding_hotkey_ok(self.hotkey_ok);
        }
    }

    fn preload_for_onboarding_try(&self) {
        let Some(dash) = &self.dashboard else {
            return;
        };
        let is_downloading = self
            .snapshot
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .downloading_key
            .is_some();
        if is_downloading {
            return;
        }
        let selected_key = dash.get_onboarding_selected_model().to_string();
        let (model_key, model_dir) = {
            let mut s = self.settings.lock().unwrap_or_else(|e| e.into_inner());
            if !selected_key.is_empty() && s.model_size != selected_key {
                s.model_size = selected_key;
            }
            (s.model_size.clone(), s.resolve_model_dir())
        };
        let cached = whispercpp_model_path(&model_dir, &model_key)
            .metadata()
            .map(|m| m.is_file() && m.len() > 1_048_576)
            .unwrap_or(false);
        if !cached {
            dash.set_onboarding_model_ready(false);
            dash.set_onboarding_model_loading(false);
            return;
        }
        self.save_settings(false);
        dash.set_onboarding_model_loading(true);
        let coord = self.coordinator.clone();
        let dash_weak = dash.as_weak();
        let _ = thread::Builder::new()
            .name("eqho-onboarding-preload".into())
            .spawn(move || {
                let ok = coord.preload_model().is_ok();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(d) = dash_weak.upgrade() {
                        d.set_onboarding_model_loading(false);
                        d.set_onboarding_model_ready(ok);
                    }
                });
            });
    }

    fn start_mic_test(&mut self) {
        self.stop_mic_test();
        let dev_idx = self
            .settings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .audio_device
            .and_then(|d| usize::try_from(d).ok());

        let dash_weak_opt = self
            .dash_weak
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let blocked = self.platform.mic_access_blocked() == Some(true);

        if blocked {
            if let Some(dash) = &self.dashboard {
                dash.set_onboarding_mic_error(
                    "Windows is blocking the microphone. Turn on microphone access for desktop apps."
                        .into(),
                );
            }
            return;
        }

        let res = self.platform.start_capture(
            dev_idx,
            Box::new(move |block| {
                let mut sum_sq = 0.0_f32;
                for &s in block {
                    sum_sq += s * s;
                }
                let rms = if block.is_empty() {
                    0.0
                } else {
                    (sum_sq / block.len() as f32).sqrt()
                };
                let level = level_meter(rms);
                if let Some(ref dash_weak) = dash_weak_opt {
                    let weak = dash_weak.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(dash) = weak.upgrade() {
                            dash.set_onboarding_mic_level(level);
                        }
                    });
                }
            }),
        );

        match res {
            Ok(stream) => {
                self.mic_test_stream = Some(stream);
                if let Some(dash) = &self.dashboard {
                    dash.set_onboarding_mic_error("".into());
                }
            }
            Err(err) => {
                if let Some(dash) = &self.dashboard {
                    dash.set_onboarding_mic_error(err.to_string().into());
                }
            }
        }
    }

    fn stop_mic_test(&mut self) {
        self.mic_test_stream = None;
    }

    fn refresh_mics(&mut self, user_triggered: bool) {
        self.mic_devices = self.platform.list_input_devices();
        let mut mic_names: Vec<SharedString> = vec!["System Default".into()];
        let mut mic_idx = 0_i32;
        let cur_dev = self
            .settings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .audio_device;
        for (i, d) in self.mic_devices.iter().enumerate() {
            mic_names.push(d.name.clone().into());
            if cur_dev == Some(d.index as i32) {
                mic_idx = (i + 1) as i32;
            }
        }
        let count = self.mic_devices.len();
        let mut restart_test = false;
        if let Some(dash) = &self.dashboard {
            dash.set_mic_names(ModelRc::new(VecModel::from(mic_names)));
            dash.set_mic_index(mic_idx);
            let status = if user_triggered {
                if count == 0 {
                    "Refreshed — no microphones detected yet. Check the connection and try again."
                        .to_owned()
                } else {
                    format!(
                        "Refreshed — {count} microphone{} detected.",
                        if count == 1 { "" } else { "s" }
                    )
                }
            } else if count > 0 {
                format!(
                    "{count} microphone{} detected. Plugged in a new one? Click Refresh devices.",
                    if count == 1 { "" } else { "s" }
                )
            } else {
                "No microphones detected. Plug one in and click Refresh devices.".to_owned()
            };
            dash.set_onboarding_mic_status(status.into());
            restart_test =
                user_triggered && dash.get_onboarding_active() && dash.get_onboarding_step() == 2;
        }
        if restart_test {
            self.start_mic_test();
        }
    }

    fn start_download(&mut self, model_key: String) {
        let already_downloading = {
            let mut snap = self.snapshot.lock().unwrap_or_else(|e| e.into_inner());
            if snap.downloading_key.as_deref() == Some(model_key.as_str()) {
                true
            } else {
                snap.downloading_key = Some(model_key.clone());
                snap.download_progress = 0.0;
                snap.download_detail = "Downloading 0% · Connecting…".to_owned();
                snap.download_done_key = None;
                snap.download_error_key = None;
                false
            }
        };
        if already_downloading {
            return;
        }
        if let Some(prev) = self.download_cancel.take() {
            prev.store(true, Ordering::Release);
        }
        let cancel_flag = Arc::new(AtomicBool::new(false));
        self.download_cancel = Some(Arc::clone(&cancel_flag));

        let model_dir = self
            .settings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .resolve_model_dir();
        if let Some(dash) = &self.dashboard {
            dash.set_bg_download_active(true);
            dash.set_bg_download_done(false);
            dash.set_bg_download_error(false);
            dash.set_bg_download_model(model_short_name(&model_key).into());
            dash.set_bg_download_progress(0.0);
            dash.set_bg_download_pct(0);
            dash.set_bg_download_detail("Downloading 0% · Connecting…".into());
        }
        self.refresh_models();

        let ui = Arc::clone(&self.ui_sink);
        let settings_arc = Arc::clone(&self.settings);
        let settings_path = self.settings_path.clone();
        let coordinator = self.coordinator.clone();

        let _ = thread::Builder::new()
            .name("eqho-model-download".into())
            .spawn(move || {
                let res = download_ggml_model_cancellable(
                    &model_key,
                    &model_dir,
                    &cancel_flag,
                    &mut |msg, frac| {
                        if !cancel_flag.load(Ordering::Acquire) {
                            ui.on_model_progress(&model_key, msg, frac);
                        }
                    },
                );
                match res {
                    Ok(_) if cancel_flag.load(Ordering::Acquire) => {
                        ui.clear_cancelled_download(&model_key);
                    }
                    Ok(_) => {
                        // CRITICAL: Drop `settings_arc` lock BEFORE calling into `coordinator`
                        // or `ui` so `on_model_progress -> refresh_models_in_dash` can never
                        // re-entrantly deadlock on `settings_arc`!
                        let updated_settings = {
                            let mut s = settings_arc.lock().unwrap_or_else(|e| e.into_inner());
                            s.model_size = model_key.clone();
                            let _ = s.save_to_path(&settings_path);
                            s.clone()
                        };
                        ui.on_model_progress(&model_key, "Loading model into memory…", Some(1.0));
                        coordinator.apply_settings(updated_settings, false);
                        let _ = coordinator.preload_model();
                        ui.on_model_progress(&model_key, "Ready", None);
                        ui.refresh_models_in_dash();
                    }
                    Err(HostError::DownloadCancelled) => {
                        ui.clear_cancelled_download(&model_key);
                    }
                    Err(err) => {
                        if cancel_flag.load(Ordering::Acquire) {
                            ui.clear_cancelled_download(&model_key);
                        } else {
                            let msg = format!("Download failed: {err}");
                            ui.on_model_progress(&model_key, &msg, None);
                            ui.refresh_models_in_dash();
                        }
                    }
                }
            });
    }

    fn cancel_download(&mut self, model_key: &str) {
        if let Some(flag) = self.download_cancel.take() {
            flag.store(true, Ordering::Release);
        }
        {
            let mut snap = self.snapshot.lock().unwrap_or_else(|e| e.into_inner());
            if snap.downloading_key.as_deref() == Some(model_key) || model_key.is_empty() {
                snap.downloading_key = None;
                snap.download_progress = 0.0;
                snap.download_detail.clear();
                snap.download_error_key = None;
            }
        }
        if let Some(dash) = &self.dashboard {
            dash.set_bg_download_active(false);
            dash.set_bg_download_error(false);
            dash.set_bg_download_detail("".into());
        }
        self.refresh_models();
    }
}

fn ensure_dashboard(ctrl: &Rc<RefCell<AppController>>, tab: Option<&str>) {
    let needs_create = ctrl.borrow().dashboard.is_none();
    if needs_create {
        let win = match DashboardWindow::new() {
            Ok(w) => w,
            Err(err) => {
                ctrl.borrow().error_log.log_error("DashboardWindow::new", &err);
                return;
            }
        };

        let weak = win.as_weak();
        {
            let mut c = ctrl.borrow_mut();
            *c.dash_weak.lock().unwrap_or_else(|e| e.into_inner()) = Some(weak.clone());
            c.dashboard = Some(win);
        }

        // Populate initial state
        {
            let mut c = ctrl.borrow_mut();
            c.refresh_mics(false);
            let is_dark = c.resolved_is_dark();
            let s = c.settings.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let snap = c.snapshot.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let hw = c.hw_class;
            let model_dir = s.resolve_model_dir();
            let rec_key = eqho_core::modelpick::recommend_model(hw, &s.language);
            let s_cached = whispercpp_model_path(&model_dir, &s.model_size)
                .metadata()
                .map(|m| m.is_file() && m.len() > 1_048_576)
                .unwrap_or(false);
            let initial_model_key = if s_cached {
                s.model_size.clone()
            } else {
                rec_key.to_owned()
            };
            let initial_cached = whispercpp_model_path(&model_dir, &initial_model_key)
                .metadata()
                .map(|m| m.is_file() && m.len() > 1_048_576)
                .unwrap_or(false);
            if let Some(dash) = &c.dashboard {
                dash.set_app_version(env!("CARGO_PKG_VERSION").into());
                apply_settings_to_window(dash, &s, hw, is_dark);
                dash.set_onboarding_model_cached(initial_cached);
                dash.set_onboarding_hotkey_ok(c.hotkey_ok);
                dash.set_onboarding_is_dictating(snap.is_dictating);

                let icon_bytes = if is_dark { LOGO_64_DARK } else { LOGO_64_WHITE };
                if let Ok((w, h, rgba)) = decode_png_rgba(icon_bytes) {
                    dash.set_window_icon(rgba_to_slint_image(w, h, &rgba));
                }

                dash.set_logo_image(wordmark_image(is_dark));

                if let Some(ref key) = snap.downloading_key {
                    dash.set_bg_download_active(true);
                    dash.set_bg_download_model(model_short_name(key).into());
                    dash.set_bg_download_progress(snap.download_progress);
                    dash.set_bg_download_pct((snap.download_progress * 100.0).round() as i32);
                    dash.set_bg_download_detail(snap.download_detail.clone().into());
                } else if let Some(ref key) = snap.download_done_key {
                    dash.set_bg_download_done(true);
                    dash.set_bg_download_model(model_short_name(key).into());
                    dash.set_bg_download_progress(1.0);
                    dash.set_bg_download_pct(100);
                    dash.set_bg_download_detail("Ready".into());
                } else if let Some(ref key) = snap.download_error_key {
                    dash.set_bg_download_error(true);
                    dash.set_bg_download_model(model_short_name(key).into());
                    dash.set_bg_download_detail(snap.download_detail.clone().into());
                }

                let recovery = snap.recovery_text.unwrap_or_default();
                dash.set_recovery_text(recovery.into());

                // Clamp initial logical window size to the active monitor work area so the window
                // and onboarding footer never slip behind the Windows taskbar at high DPI scaling.
                if let Some(work) = c.damt_platform.active_screen().active_screen_bounds() {
                    let scale = dash.window().scale_factor().max(1.0);
                    let max_w = ((work.width as f32 / scale) - 32.0).max(600.0);
                    let max_h = ((work.height as f32 / scale) - 48.0).max(420.0);
                    let target_w = 940.0_f32.min(max_w);
                    let target_h = 560.0_f32.min(max_h);
                    dash.window()
                        .set_size(slint::LogicalSize::new(target_w, target_h));
                }
            }
            c.refresh_models();
            c.refresh_onboarding_models(Some(&initial_model_key));
            c.refresh_history();
            c.refresh_active_engine_display();
            #[cfg(feature = "store")]
            c.sync_license_to_dash();
        }

        wire_dashboard_callbacks(ctrl);
    }

    let c = ctrl.borrow();
    if let Some(dash) = &c.dashboard {
        if let Some(t) = tab {
            if t == "settings" {
                dash.set_active_tab("general".into());
                dash.set_tour_active(false);
                dash.set_settings_open(true);
            } else {
                dash.set_settings_open(false);
                dash.set_active_tab(t.into());
            }
        }
        let _ = dash.show();
    }
}

fn capture_settings_backdrop(dash: &DashboardWindow) {
    if let Ok(snap) = dash.window().take_snapshot() {
        dash.set_settings_backdrop(blur_snapshot(&snap));
    } else {
        dash.set_settings_backdrop(slint::Image::default());
    }
}

fn capture_tour_backdrop(dash: &DashboardWindow) {
    let was_tour = dash.get_tour_active();
    dash.set_tour_active(false);
    if let Ok(snap) = dash.window().take_snapshot() {
        dash.set_tour_backdrop(blur_snapshot_strong(&snap));
    } else {
        dash.set_tour_backdrop(slint::Image::default());
    }
    dash.set_tour_active(was_tour);
}

fn wire_dashboard_callbacks(ctrl: &Rc<RefCell<AppController>>) {
    let Some(dash) = ctrl.borrow().dashboard.as_ref().map(|d| d.as_weak()) else {
        return;
    };
    let Some(dash) = dash.upgrade() else {
        return;
    };

    // Window close hides rather than quitting the tray app.
    {
        let ctrl_w = Rc::clone(ctrl);
        let dash_w = dash.as_weak();
        dash.window().on_close_requested(move || {
            let mut c = ctrl_w.borrow_mut();
            c.stop_mic_test();
            c.stop_overlay_preview();
            if let Some(cancel) = c.hotkey_capture_cancel.take() {
                cancel.store(true, Ordering::Release);
                let _ = c.coordinator.register_hotkey();
            }
            if let Some(d) = dash_w.upgrade() {
                d.set_hotkey_capturing(false);
                let _ = d.hide();
            }
            slint::CloseRequestResponse::HideWindow
        });
    }

    // Recovery banner
    {
        let ctrl_w = Rc::clone(ctrl);
        let dash_w = dash.as_weak();
        dash.on_copy_recovery(move || {
            let c = ctrl_w.borrow();
            let _ = c.coordinator.copy_last_dictation();
            c.overlay.hide_recovery();
            c.snapshot
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .recovery_text = None;
            c.ui_sink.refresh_tray();
            if let Some(d) = dash_w.upgrade() {
                d.set_recovery_text("".into());
            }
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        let dash_w = dash.as_weak();
        dash.on_view_recovery_in_history(move || {
            let c = ctrl_w.borrow();
            c.overlay.hide_recovery();
            if let Some(d) = dash_w.upgrade() {
                d.set_showing_trash(false);
                d.set_active_tab("history".into());
            }
            c.refresh_history();
        });
    }
    #[cfg(feature = "store")]
    {
        let ctrl_w = Rc::clone(ctrl);
        let dash_w = dash.as_weak();
        dash.on_buy_clicked(move || {
            let (is_demo, error_log) = {
                let c = ctrl_w.borrow();
                (c.demo_license_active, c.error_log.clone())
            };
            if is_demo {
                error_log.log(Some("demo_purchase"), "demo: purchase skipped");
                return;
            }
            let hwnd = dash_w
                .upgrade()
                .and_then(|d| raw_win32_hwnd(d.window()));
            store_license::trigger_purchase(hwnd);
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        let dash_w = dash.as_weak();
        dash.on_dismiss_recovery(move || {
            let c = ctrl_w.borrow();
            c.overlay.hide_recovery();
            c.snapshot
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .recovery_text = None;
            c.ui_sink.refresh_tray();
            if let Some(d) = dash_w.upgrade() {
                d.set_recovery_text("".into());
            }
        });
    }

    // TopBar, Settings modal & Interactive Guide tour
    {
        let dash_w = dash.as_weak();
        dash.on_tab_selected(move |t| {
            if let Some(d) = dash_w.upgrade() {
                d.set_active_tab(t);
                if d.get_tour_active() {
                    capture_tour_backdrop(&d);
                }
            }
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        let dash_w = dash.as_weak();
        dash.on_open_settings(move || {
            if let Some(d) = dash_w.upgrade() {
                d.set_tour_active(false);
                d.set_tour_backdrop(slint::Image::default());
                d.set_settings_open(false);
                capture_settings_backdrop(&d);
                d.set_settings_open(true);
            }
            ctrl_w.borrow().sync_overlay_preview();
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        let dash_w = dash.as_weak();
        dash.on_close_settings(move || {
            if let Some(d) = dash_w.upgrade() {
                d.set_settings_open(false);
                d.set_settings_backdrop(slint::Image::default());
            }
            ctrl_w.borrow().stop_overlay_preview();
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_settings_section_changed(move |_section| {
            ctrl_w.borrow().sync_overlay_preview();
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_overlay_preview_toggled(move |_active| {
            ctrl_w.borrow().sync_overlay_preview();
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        let dash_w = dash.as_weak();
        dash.on_start_tour(move || {
            ctrl_w.borrow().stop_overlay_preview();
            if let Some(d) = dash_w.upgrade() {
                d.set_settings_open(false);
                d.set_settings_backdrop(slint::Image::default());
                d.set_tour_step(0);
                d.set_active_tab("general".into());
                d.set_tour_active(true);
                capture_tour_backdrop(&d);
            }
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        let dash_w = dash.as_weak();
        dash.on_tour_next(move || {
            if let Some(d) = dash_w.upgrade() {
                let next = d.get_tour_step() + 1;
                if next >= 4 {
                    d.set_tour_active(false);
                    d.set_tour_backdrop(slint::Image::default());
                    d.set_tour_step(0);
                    let c = ctrl_w.borrow();
                    c.settings
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .tour_done = true;
                    c.save_settings(false);
                } else {
                    d.set_tour_step(next);
                    let tab = match next {
                        1 => "models",
                        2 => "history",
                        _ => "general",
                    };
                    d.set_active_tab(tab.into());
                    capture_tour_backdrop(&d);
                }
            }
        });
    }
    {
        let dash_w = dash.as_weak();
        dash.on_tour_back(move || {
            if let Some(d) = dash_w.upgrade() {
                let prev = (d.get_tour_step() - 1).max(0);
                d.set_tour_step(prev);
                let tab = match prev {
                    1 => "models",
                    2 => "history",
                    _ => "general",
                };
                d.set_active_tab(tab.into());
                capture_tour_backdrop(&d);
            }
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        let dash_w = dash.as_weak();
        dash.on_dismiss_tour(move || {
            if let Some(d) = dash_w.upgrade() {
                d.set_tour_active(false);
                d.set_tour_backdrop(slint::Image::default());
            }
            let c = ctrl_w.borrow();
            c.settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .tour_done = true;
            c.save_settings(false);
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        let dash_w = dash.as_weak();
        dash.on_theme_toggled(move || {
            let c = ctrl_w.borrow();
            let currently_dark = c.resolved_is_dark();
            let next_mode = if currently_dark { "light" } else { "dark" };
            c.settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .theme = next_mode.to_owned();
            c.save_settings(false);
            c.apply_theme_and_overlay();
            if let Some(d) = dash_w.upgrade() {
                d.set_theme_mode_index(if currently_dark { 0 } else { 1 });
                if d.get_tour_active() {
                    capture_tour_backdrop(&d);
                }
            }
        });
    }

    // General tab
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_mic_selected(move |idx| {
            let mut c = ctrl_w.borrow_mut();
            let dev = if idx <= 0 {
                None
            } else {
                c.mic_devices
                    .get((idx - 1) as usize)
                    .map(|d| d.index as i32)
            };
            c.settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .audio_device = dev;
            c.save_settings(false);
            if c.mic_test_stream.is_some() {
                c.start_mic_test();
            }
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_refresh_mics(move || {
            ctrl_w.borrow_mut().refresh_mics(true);
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_vad_sensitivity_changed(move |pct| {
            let c = ctrl_w.borrow();
            c.settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .vad_sensitivity = f64::from(pct) / 100.0;
            c.save_settings(false);
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        let dash_w = dash.as_weak();
        dash.on_start_hotkey_capture(move || {
            let mut c = ctrl_w.borrow_mut();
            if let Some(prev) = c.hotkey_capture_cancel.take() {
                prev.store(true, Ordering::Release);
            }
            // Pause the live global hotkey while capturing so pressing the current hotkey
            // (e.g. Alt+Q) never activates dictation during capture.
            c.coordinator.unregister_hotkey();
            if let Some(d) = dash_w.upgrade() {
                d.set_hotkey_capturing(true);
            }
            let cancel = Arc::new(AtomicBool::new(false));
            c.hotkey_capture_cancel = Some(Arc::clone(&cancel));
            spawn_hotkey_capture(
                cancel,
                dash_w.clone(),
                Arc::clone(&c.settings),
                c.settings_path.clone(),
                c.coordinator.clone(),
                Arc::clone(&c.snapshot),
                c.ui_sink.tray_weak.clone(),
            );
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        let dash_w = dash.as_weak();
        dash.on_cancel_hotkey_capture(move || {
            let mut c = ctrl_w.borrow_mut();
            if let Some(cancel) = c.hotkey_capture_cancel.take() {
                cancel.store(true, Ordering::Release);
                c.hotkey_ok = c.coordinator.register_hotkey().is_ok();
            }
            if let Some(d) = dash_w.upgrade() {
                d.set_hotkey_capturing(false);
                d.set_onboarding_hotkey_ok(c.hotkey_ok);
            }
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        let dash_w = dash.as_weak();
        dash.on_hotkey_capture_key(move |ctrl_k, alt_k, shift_k, meta_k, text| {
            let Some(combo) = slint_key_to_hotkey_combo(ctrl_k, alt_k, shift_k, meta_k, text.as_str())
            else {
                return false;
            };
            let mut c = ctrl_w.borrow_mut();
            if let Some(cancel) = c.hotkey_capture_cancel.take() {
                cancel.store(true, Ordering::Release);
            }
            let new_settings = {
                let mut s = c.settings.lock().unwrap_or_else(|e| e.into_inner());
                s.hotkey = combo.clone();
                let _ = s.save_to_path(&c.settings_path);
                s.clone()
            };
            c.coordinator.apply_settings(new_settings, false);
            c.hotkey_ok = c.coordinator.register_hotkey().is_ok();
            let dictation_allowed = {
                let mut snap = c.snapshot.lock().unwrap_or_else(|e| e.into_inner());
                snap.hotkey = combo.clone();
                snap.dictation_allowed
            };
            let display = eqho_ui::format_hotkey_display(&combo);
            let tooltip = if dictation_allowed {
                status_summary_title(false, &combo)
            } else {
                "Your trial has ended.".to_owned()
            };
            if let Some(d) = dash_w.upgrade() {
                d.set_hotkey_capturing(false);
                d.set_hotkey_display(display.into());
                d.set_onboarding_hotkey_ok(c.hotkey_ok);
            }
            if let Some(t) = c.ui_sink.tray_weak.upgrade() {
                t.set_tray_tooltip(tooltip.into());
            }
            true
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_hotkey_mode_changed(move |idx| {
            let c = ctrl_w.borrow();
            c.settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .hotkey_mode = if idx == 1 {
                "hold".to_owned()
            } else {
                "toggle".to_owned()
            };
            c.save_settings(false);
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        let dash_w = dash.as_weak();
        dash.on_language_selected(move |idx| {
            let c = ctrl_w.borrow();
            let code = language_code_for_index(idx);
            let model_size = {
                let mut s = c.settings.lock().unwrap_or_else(|e| e.into_inner());
                s.language = code.to_owned();
                s.record_recent_language(code);
                s.model_size.clone()
            };
            c.save_settings(false);
            c.apply_theme_and_overlay();
            if let Some(d) = dash_w.upgrade() {
                d.set_language_warning(language_warning_text(code, &model_size, c.hw_class).into());
            }
            let rec_key = eqho_core::modelpick::recommend_model(c.hw_class, code);
            c.refresh_onboarding_models(Some(rec_key));
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_initial_prompt_changed(move |txt| {
            let c = ctrl_w.borrow();
            c.settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .initial_prompt = txt.to_string();
            c.save_settings(false);
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_format_cleanup_toggled(move |v| {
            let c = ctrl_w.borrow();
            c.settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .format_cleanup = v;
            c.save_settings(false);
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_sound_feedback_toggled(move |v| {
            let c = ctrl_w.borrow();
            c.settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .sound_feedback = v;
            c.save_settings(false);
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_history_enabled_toggled(move |v| {
            let c = ctrl_w.borrow();
            c.settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .history_enabled = v;
            c.save_settings(false);
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_delivery_mode_changed(move |idx| {
            let c = ctrl_w.borrow();
            c.settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .delivery_mode = delivery_mode_key(idx).to_owned();
            c.save_settings(false);
            c.apply_theme_and_overlay();
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_paste_mode_changed(move |idx| {
            let c = ctrl_w.borrow();
            c.settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .auto_paste = idx == 0;
            c.save_settings(false);
        });
    }

    // Models tab
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_engine_backend_changed(move |idx| {
            let c = ctrl_w.borrow();
            c.settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .engine_backend = engine_backend_key(idx).to_owned();
            c.save_settings(false);
            c.refresh_models();
            c.refresh_active_engine_display();
            let coord = c.coordinator.clone();
            let _ = thread::Builder::new()
                .name("eqho-engine-reload".into())
                .spawn(move || {
                    let _ = coord.preload_model();
                });
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        let dash_w = dash.as_weak();
        dash.on_select_model(move |key| {
            let c = ctrl_w.borrow();
            let lang = {
                let mut s = c.settings.lock().unwrap_or_else(|e| e.into_inner());
                s.model_size = key.to_string();
                s.language.clone()
            };
            {
                let mut snap = c.snapshot.lock().unwrap_or_else(|e| e.into_inner());
                if snap.downloading_key.as_deref() == Some(key.as_str()) {
                    snap.downloading_key = None;
                }
                snap.loading_key = Some(key.to_string());
            }
            // Never reload model synchronously on the UI thread; the background thread below handles preload.
            c.save_settings(false);
            c.refresh_models();
            if let Some(d) = dash_w.upgrade() {
                d.set_language_warning(language_warning_text(&lang, &key, c.hw_class).into());
            }
            let coord = c.coordinator.clone();
            let _ = thread::Builder::new()
                .name("eqho-model-reload".into())
                .spawn(move || {
                    let _ = coord.preload_model();
                });
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_download_model(move |key| {
            ctrl_w.borrow_mut().start_download(key.to_string());
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_cancel_download_model(move |key| {
            ctrl_w.borrow_mut().cancel_download(&key);
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_delete_model(move |key| {
            let c = ctrl_w.borrow();
            let (selected, model_dir) = {
                let s = c.settings.lock().unwrap_or_else(|e| e.into_inner());
                (s.model_size.clone(), s.resolve_model_dir())
            };
            if selected == key.as_str() {
                return;
            }
            let path = whispercpp_model_path(&model_dir, &key);
            let _ = std::fs::remove_file(path);
            c.refresh_models();
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_open_model_folder(move || {
            let model_dir = ctrl_w
                .borrow()
                .settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .resolve_model_dir();
            let _ = std::fs::create_dir_all(&model_dir);
            open_folder(&model_dir);
        });
    }

    // History tab (with Trash bin & Copy feedback state)
    {
        let ctrl_w = Rc::clone(ctrl);
        let dash_w = dash.as_weak();
        dash.on_history_filter_changed(move || {
            if let Some(d) = dash_w.upgrade() {
                d.set_history_status("".into());
            }
            ctrl_w.borrow().refresh_history();
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        let dash_w = dash.as_weak();
        dash.on_copy_history_entry(move |ts_key, txt| {
            let _ = ctrl_w.borrow().platform.set_text(&txt, false);
            if let Some(d) = dash_w.upgrade() {
                d.set_copied_ts_key(ts_key.clone());
                d.set_history_status("Copied to clipboard".into());
                let dash_clear = dash_w.clone();
                let key_clear = ts_key;
                slint::Timer::single_shot(std::time::Duration::from_millis(1500), move || {
                    if let Some(d2) = dash_clear.upgrade()
                        && d2.get_copied_ts_key() == key_clear
                    {
                        d2.set_copied_ts_key("".into());
                    }
                });
            }
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        let dash_w = dash.as_weak();
        dash.on_delete_history_entry(move |ts_key| {
            if let Ok(ts) = ts_key.parse::<f64>() {
                let c = ctrl_w.borrow();
                let _ = c.history.move_to_trash(ts);
                c.refresh_history();
                if let Some(d) = dash_w.upgrade() {
                    d.set_history_status("Moved to Trash".into());
                }
            }
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        let dash_w = dash.as_weak();
        dash.on_clear_history(move || {
            let c = ctrl_w.borrow();
            let _ = c.history.move_all_to_trash();
            c.refresh_history();
            if let Some(d) = dash_w.upgrade() {
                d.set_history_status("Moved all to Trash".into());
            }
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        let dash_w = dash.as_weak();
        dash.on_toggle_trash_view(move |show_trash| {
            if let Some(d) = dash_w.upgrade() {
                d.set_showing_trash(show_trash);
                d.set_history_status("".into());
            }
            ctrl_w.borrow().refresh_history();
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        let dash_w = dash.as_weak();
        dash.on_restore_trash_entry(move |ts_key| {
            if let Ok(ts) = ts_key.parse::<f64>() {
                let c = ctrl_w.borrow();
                let _ = c.history.restore_from_trash(ts);
                c.refresh_history();
                if let Some(d) = dash_w.upgrade() {
                    d.set_history_status("Restored to History".into());
                }
            }
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        let dash_w = dash.as_weak();
        dash.on_delete_trash_permanently(move |ts_key| {
            if let Ok(ts) = ts_key.parse::<f64>() {
                let c = ctrl_w.borrow();
                let _ = c.history.delete(ts);
                c.refresh_history();
                if let Some(d) = dash_w.upgrade() {
                    d.set_history_status("Deleted permanently".into());
                }
            }
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        let dash_w = dash.as_weak();
        dash.on_empty_trash(move || {
            let c = ctrl_w.borrow();
            let _ = c.history.empty_trash();
            c.refresh_history();
            if let Some(d) = dash_w.upgrade() {
                d.set_history_status("Trash emptied".into());
            }
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        let dash_w = dash.as_weak();
        dash.on_export_history(move || {
            let c = ctrl_w.borrow();
            let dest = c.config_dir.join("eqho-history.txt");
            if let Ok(count) = c.history.export_txt(&dest) {
                let msg = format!("Exported {count} entries to {}", dest.display());
                c.ui_sink.notify(&msg);
                if let Some(d) = dash_w.upgrade() {
                    d.set_history_status(format!("Exported {count} entries").into());
                }
            }
        });
    }

    // Settings tab
    {
        let ctrl_w = Rc::clone(ctrl);
        let dash_w = dash.as_weak();
        dash.on_theme_mode_changed(move |idx| {
            let c = ctrl_w.borrow();
            c.settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .theme = theme_mode_key(idx).to_owned();
            c.save_settings(false);
            c.apply_theme_and_overlay();
            if let Some(d) = dash_w.upgrade()
                && d.get_settings_open()
            {
                d.set_settings_open(false);
                capture_settings_backdrop(&d);
                d.set_settings_open(true);
            }
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_ui_scale_changed(move |idx| {
            let c = ctrl_w.borrow();
            c.settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .ui_scale = ui_scale_value(idx);
            c.save_settings(false);
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_remove_fillers_toggled(move |v| {
            let c = ctrl_w.borrow();
            c.settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove_fillers = v;
            c.save_settings(false);
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_voice_commands_toggled(move |v| {
            let c = ctrl_w.borrow();
            c.settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .voice_commands = v;
            c.save_settings(false);
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        let dash_w = dash.as_weak();
        dash.on_save_replacements(move |txt| {
            let parsed = parse_replacements_text(&txt);
            let count = parsed.len();
            let c = ctrl_w.borrow();
            c.settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .replacements = parsed;
            c.save_settings(false);
            if let Some(d) = dash_w.upgrade() {
                d.set_replacements_status(format!("Saved ({count})").into());
            }
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        let dash_w = dash.as_weak();
        dash.on_save_paste_rules(move |txt| {
            let parsed = parse_paste_rules_text(&txt);
            let count = parsed.len();
            let c = ctrl_w.borrow();
            c.settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .paste_rules = parsed;
            c.save_settings(false);
            if let Some(d) = dash_w.upgrade() {
                d.set_paste_rules_status(format!("Saved ({count})").into());
            }
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_volume_duck_selected(move |idx| {
            let c = ctrl_w.borrow();
            c.settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .volume_duck = volume_duck_key(idx).to_owned();
            c.save_settings(false);
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_history_trash_days_selected(move |idx| {
            let days = history_trash_days_value(idx);
            let c = ctrl_w.borrow();
            c.settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .history_trash_days = days;
            c.save_settings(false);
            let _ = c.history.purge_expired_trash(days, now_epoch_secs());
            c.refresh_history();
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        let dash_w = dash.as_weak();
        dash.on_open_trash(move || {
            if let Some(d) = dash_w.upgrade() {
                d.set_settings_open(false);
                d.set_settings_backdrop(slint::Image::default());
                d.set_showing_trash(true);
                d.set_history_status("".into());
                d.set_active_tab("history".into());
            }
            ctrl_w.borrow().refresh_history();
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_start_with_windows_toggled(move |enabled| {
            let c = ctrl_w.borrow();
            if let Err(err) = c.damt_platform.autostart().set_enabled(enabled) {
                c.error_log.log_error("autostart", &err);
            }
            c.settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .start_with_windows = enabled;
            c.save_settings(false);
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_overlay_enabled_toggled(move |v| {
            let c = ctrl_w.borrow();
            c.settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .overlay_enabled = v;
            c.save_settings(false);
            c.apply_theme_and_overlay();
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_overlay_show_text_toggled(move |v| {
            let c = ctrl_w.borrow();
            c.settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .overlay_show_text = v;
            c.save_settings(false);
            c.apply_theme_and_overlay();
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_overlay_show_level_toggled(move |v| {
            let c = ctrl_w.borrow();
            c.settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .overlay_show_level = v;
            c.save_settings(false);
            c.apply_theme_and_overlay();
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_overlay_controls_enabled_toggled(move |v| {
            let c = ctrl_w.borrow();
            c.settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .overlay_controls_enabled = v;
            c.save_settings(false);
            c.apply_theme_and_overlay();
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_overlay_controls_pinned_toggled(move |v| {
            let c = ctrl_w.borrow();
            c.settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .overlay_controls_pinned = v;
            c.save_settings(false);
            c.apply_theme_and_overlay();
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_overlay_monitor_selected(move |idx| {
            let c = ctrl_w.borrow();
            c.settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .overlay_monitor = overlay_monitor_key(idx).to_owned();
            c.save_settings(false);
            c.apply_theme_and_overlay();
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_overlay_position_selected(move |idx| {
            let c = ctrl_w.borrow();
            c.settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .overlay_position = overlay_position_key(idx).to_owned();
            c.save_settings(false);
            c.apply_theme_and_overlay();
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_overlay_pill_scale_changed(move |pct| {
            let c = ctrl_w.borrow();
            c.settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .overlay_pill_scale = f64::from(pct) / 100.0;
            c.save_settings(false);
            c.apply_theme_and_overlay();
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_overlay_see_through_changed(move |pct| {
            let c = ctrl_w.borrow();
            c.settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .overlay_opacity = 1.0 - (f64::from(pct) / 100.0);
            c.save_settings(false);
            c.apply_theme_and_overlay();
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_overlay_panel_see_through_changed(move |pct| {
            let c = ctrl_w.borrow();
            c.settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .overlay_panel_opacity = 1.0 - (f64::from(pct) / 100.0);
            c.save_settings(false);
            c.apply_theme_and_overlay();
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_overlay_font_size_changed(move |sz| {
            let c = ctrl_w.borrow();
            c.settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .overlay_font_size = sz;
            c.save_settings(false);
            c.apply_theme_and_overlay();
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        let dash_w = dash.as_weak();
        dash.on_start_onboarding(move || {
            let c = ctrl_w.borrow();
            c.stop_overlay_preview();
            let s = c.settings.lock().unwrap_or_else(|e| e.into_inner()).clone();
            let model_dir = s.resolve_model_dir();
            let s_cached = whispercpp_model_path(&model_dir, &s.model_size)
                .metadata()
                .map(|m| m.is_file() && m.len() > 1_048_576)
                .unwrap_or(false);
            let initial_key = if s_cached {
                s.model_size.clone()
            } else {
                eqho_core::modelpick::recommend_model(c.hw_class, &s.language).to_owned()
            };
            c.refresh_onboarding_models(Some(&initial_key));
            if let Some(d) = dash_w.upgrade() {
                d.set_settings_open(false);
                d.set_settings_backdrop(slint::Image::default());
                d.set_tour_active(false);
                d.set_onboarding_hotkey_ok(c.hotkey_ok);
                d.set_onboarding_is_dictating(false);
                d.set_onboarding_try_hint("".into());
                d.set_onboarding_try_text("".into());
                d.set_onboarding_step(0);
                d.set_onboarding_active(true);
            }
        });
    }

    // Onboarding callbacks
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_onboarding_open_mic_settings(move || {
            let _ = ctrl_w.borrow().platform.open_mic_privacy_settings();
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_onboarding_retry_mic(move || {
            ctrl_w.borrow_mut().start_mic_test();
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_onboarding_select_model(move |key| {
            let c = ctrl_w.borrow();
            {
                let mut s = c.settings.lock().unwrap_or_else(|e| e.into_inner());
                s.model_size = key.to_string();
            }
            c.save_settings(false);
            c.refresh_onboarding_models(Some(&key));
            c.refresh_models();
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        let dash_w = dash.as_weak();
        dash.on_onboarding_download_model(move || {
            let key = dash_w
                .upgrade()
                .map(|d| d.get_onboarding_selected_model().to_string())
                .filter(|k| !k.is_empty())
                .unwrap_or_else(|| {
                    let c = ctrl_w.borrow();
                    let lang = c
                        .settings
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .language
                        .clone();
                    eqho_core::modelpick::recommend_model(c.hw_class, &lang).to_owned()
                });
            ctrl_w.borrow_mut().start_download(key);
        });
    }
    {
        let dash_w = dash.as_weak();
        dash.on_onboarding_later_model(move || {
            if let Some(d) = dash_w.upgrade() {
                d.set_onboarding_step(4);
            }
        });
    }
    {
        let coord = ctrl.borrow().coordinator.clone();
        dash.on_onboarding_toggle_test(move || {
            let c = coord.clone();
            let _ = thread::Builder::new()
                .name("eqho-onboarding-test".into())
                .spawn(move || c.toggle());
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        let coord = ctrl.borrow().coordinator.clone();
        dash.on_onboarding_try_hotkey(move |ctrl_k, alt_k, shift_k, meta_k, text| {
            let raw_hotkey = ctrl_w
                .borrow()
                .settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .hotkey
                .clone();
            if matches_slint_hotkey(&raw_hotkey, ctrl_k, alt_k, shift_k, meta_k, text.as_str()) {
                let c = coord.clone();
                let _ = thread::Builder::new()
                    .name("eqho-onboarding-hotkey-test".into())
                    .spawn(move || c.toggle());
                true
            } else {
                false
            }
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        let dash_w = dash.as_weak();
        dash.on_onboarding_go_to_step(move |target_step| {
            if let Some(d) = dash_w.upgrade() {
                d.set_onboarding_step(target_step);
            }
            let mut c = ctrl_w.borrow_mut();
            if target_step == 2 {
                c.refresh_mics(false);
                c.start_mic_test();
            } else {
                c.stop_mic_test();
                if target_step == 3 {
                    c.refresh_onboarding_models(None);
                } else if target_step == 5 {
                    c.ensure_hotkey_registered();
                    c.refresh_onboarding_models(None);
                    c.preload_for_onboarding_try();
                }
            }
        });
    }
    {
        dash.on_open_url(move |url| {
            open_https_url(&url);
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_copy_text(move |txt| {
            let _ = ctrl_w.borrow().platform.set_text(&txt, false);
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        let dash_w = dash.as_weak();
        dash.on_onboarding_sound_chosen(move |mute| {
            let c = ctrl_w.borrow();
            c.settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .volume_duck = if mute {
                "mute".to_owned()
            } else {
                "off".to_owned()
            };
            c.save_settings(false);
            if let Some(d) = dash_w.upgrade() {
                d.set_onboarding_duck_mute(mute);
                d.set_volume_duck_index(if mute { 4 } else { 0 });
            }
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_onboarding_step_changed(move |step| {
            let mut c = ctrl_w.borrow_mut();
            if step == 2 {
                c.refresh_mics(false);
                c.start_mic_test();
            } else {
                c.stop_mic_test();
                if step == 3 {
                    c.refresh_onboarding_models(None);
                } else if step == 5 {
                    c.ensure_hotkey_registered();
                    c.refresh_onboarding_models(None);
                    c.preload_for_onboarding_try();
                }
            }
        });
    }
    let finish_onboarding = {
        let ctrl_w = Rc::clone(ctrl);
        let dash_w = dash.as_weak();
        move || {
            let mut c = ctrl_w.borrow_mut();
            c.stop_mic_test();
            let (model_size, model_dir, show_tour) = {
                let mut s = c.settings.lock().unwrap_or_else(|e| e.into_inner());
                s.onboarding_done = true;
                (s.model_size.clone(), s.resolve_model_dir(), !s.tour_done)
            };
            c.save_settings(false);
            if let Some(d) = dash_w.upgrade() {
                d.set_onboarding_active(false);
                if show_tour {
                    d.set_active_tab("general".into());
                    d.set_tour_step(0);
                    d.set_tour_active(true);
                }
            }
            if whispercpp_model_path(&model_dir, &model_size).is_file() {
                let coord = c.coordinator.clone();
                let _ = thread::Builder::new()
                    .name("eqho-model-preload".into())
                    .spawn(move || {
                        let _ = coord.preload_model();
                    });
            }
        }
    };
    {
        let finish_a = finish_onboarding.clone();
        dash.on_onboarding_skip(finish_a);
    }
    {
        let finish_b = finish_onboarding;
        dash.on_onboarding_finish(finish_b);
    }
}

/// Match a Slint `KeyEvent` (modifiers + text) against a stored hotkey combo such as
/// `"alt+q"`, `"ctrl+shift+space"`, or `"win+f2"`.
///
/// Used as a belt-and-suspenders guard on Onboarding Step 6's test text box so pressing the
/// dictation hotkey while typing in the box always toggles dictation and never types the
/// trigger character (e.g. `'q'`) even if another app prevented `RegisterHotKey`.
fn matches_slint_hotkey(
    raw: &str,
    ctrl: bool,
    alt: bool,
    shift: bool,
    meta: bool,
    text: &str,
) -> bool {
    let trimmed = raw.trim();
    if trimmed.is_empty() || text.is_empty() {
        return false;
    }

    let mut req_ctrl = false;
    let mut req_alt = false;
    let mut req_shift = false;
    let mut req_meta = false;
    let mut trigger: Option<String> = None;

    for part in trimmed.split('+') {
        let token = part
            .trim()
            .trim_start_matches('<')
            .trim_end_matches('>')
            .to_ascii_lowercase();
        if token.is_empty() {
            return false;
        }
        match token.as_str() {
            "ctrl" | "control" | "ctrl_l" | "ctrl_r" | "lctrl" | "rctrl" => req_ctrl = true,
            "alt" | "alt_l" | "alt_r" | "alt_gr" | "lalt" | "ralt" | "menu" | "option" => {
                req_alt = true;
            }
            "shift" | "shift_l" | "shift_r" | "lshift" | "rshift" => req_shift = true,
            "win" | "windows" | "super" | "cmd" | "cmd_l" | "cmd_r" | "meta" | "lwin"
            | "rwin" => req_meta = true,
            other => {
                if trigger.is_some() {
                    return false;
                }
                trigger = Some(other.to_owned());
            }
        }
    }

    let Some(trigger) = trigger else {
        return false;
    };

    if req_ctrl != ctrl || req_alt != alt || req_shift != shift || req_meta != meta {
        return false;
    }

    if trigger.len() == 1 {
        let target = trigger.as_bytes()[0];
        if target.is_ascii_alphanumeric() {
            let mut chars = text.chars();
            if let (Some(ch), None) = (chars.next(), chars.next()) {
                let b = ch as u32;
                // Handle ASCII control characters produced by Ctrl+A..Z (0x01..=0x1A).
                if ctrl && (1..=26).contains(&b) {
                    let mapped = (b as u8 - 1) + b'a';
                    return mapped == target;
                }
                return ch.is_ascii() && (ch as u8).to_ascii_lowercase() == target;
            }
            return false;
        }
    }

    if let Some(num) = trigger.strip_prefix('f')
        && let Ok(n) = num.parse::<u32>()
        && (1..=24).contains(&n)
    {
        // Slint encodes F1..F24 as U+F704..U+F71B.
        let expected = char::from_u32(0xF704 + (n - 1));
        return expected.is_some_and(|c| {
            let mut chars = text.chars();
            chars.next() == Some(c) && chars.next().is_none()
        });
    }

    match trigger.as_str() {
        "space" | "spacebar" => text == " ",
        "enter" | "return" => text == "\n" || text == "\r",
        "tab" => text == "\t",
        "esc" | "escape" => text == "\u{001b}",
        "backspace" | "back" => text == "\u{0008}",
        "delete" | "del" => text == "\u{007f}",
        _ => false,
    }
}

/// Convert a Slint `KeyEvent` into a stored hotkey combo such as `"alt+q"` or `"ctrl+shift+f9"`.
fn slint_key_to_hotkey_combo(
    ctrl: bool,
    alt: bool,
    shift: bool,
    meta: bool,
    text: &str,
) -> Option<String> {
    let mut chars = text.chars();
    let ch = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    let key_name: String = match ch {
        ' ' => "space".to_owned(),
        '\n' | '\r' => "enter".to_owned(),
        '\t' => "tab".to_owned(),
        '\u{0008}' => "backspace".to_owned(),
        '\u{007f}' => "delete".to_owned(),
        c if ctrl && (1..=26).contains(&(c as u32)) => {
            (((c as u8) - 1 + b'a') as char).to_string()
        }
        c if c.is_ascii_alphanumeric() => c.to_ascii_lowercase().to_string(),
        c if ('\u{F704}'..='\u{F71B}').contains(&c) => {
            let n = (c as u32) - 0xF704 + 1;
            format!("f{n}")
        }
        _ => return None,
    };

    let mut parts: Vec<String> = Vec::new();
    if ctrl {
        parts.push("ctrl".to_owned());
    }
    if alt {
        parts.push("alt".to_owned());
    }
    if shift {
        parts.push("shift".to_owned());
    }
    if meta {
        parts.push("win".to_owned());
    }
    parts.push(key_name);
    Some(parts.join("+"))
}

fn open_folder(path: &Path) {
    #[cfg(windows)]
    {
        let _ = Command::new("explorer.exe").arg(path).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("open").arg(path).spawn();
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = Command::new("xdg-open").arg(path).spawn();
    }
}

fn open_https_url(url: &str) {
    let trimmed = url.trim();
    if !trimmed.starts_with("https://")
        || trimmed.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return;
    }
    #[cfg(windows)]
    {
        let _ = Command::new("rundll32.exe")
            .args(["url.dll,FileProtocolHandler", trimmed])
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("open").arg(trimmed).spawn();
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = Command::new("xdg-open").arg(trimmed).spawn();
    }
}

#[cfg(windows)]
fn spawn_hotkey_capture(
    cancel: Arc<AtomicBool>,
    dash_weak: slint::Weak<DashboardWindow>,
    settings: Arc<Mutex<Settings>>,
    settings_path: PathBuf,
    coordinator: DictationCoordinator,
    snapshot: Arc<Mutex<UiSyncSnapshot>>,
    tray_weak: slint::Weak<EqhoTray>,
) {
    use std::time::Duration;
    use eqho_ui::format_hotkey_display;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, VK_CONTROL, VK_ESCAPE, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
    };

    let _ = thread::Builder::new()
        .name("eqho-hotkey-capture".into())
        .spawn(move || {
            let is_down = |vk: i32| -> bool {
                // SAFETY: `GetAsyncKeyState` is a stateless Win32 key-state query.
                unsafe { (GetAsyncKeyState(vk) as u16 & 0x8000) != 0 }
            };

            // Wait for any currently held keys to settle briefly.
            thread::sleep(Duration::from_millis(120));
            while !cancel.load(Ordering::Acquire) {
                if is_down(i32::from(VK_ESCAPE.0)) {
                    let ok = coordinator.register_hotkey().is_ok();
                    let weak = dash_weak.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(d) = weak.upgrade() {
                            d.set_hotkey_capturing(false);
                            d.set_onboarding_hotkey_ok(ok);
                        }
                    });
                    return;
                }

                for vk in 0x08_i32..=0xFE_i32 {
                    if matches!(vk, 0x10..=0x12 | 0xA0..=0xA5 | 0x5B | 0x5C | 0x1B) {
                        continue;
                    }
                    if !is_down(vk) {
                        continue;
                    }
                    let Some(key_name) = vk_to_hotkey_name(vk) else {
                        continue;
                    };

                    let mut parts = Vec::new();
                    if is_down(i32::from(VK_CONTROL.0)) {
                        parts.push("ctrl");
                    }
                    if is_down(i32::from(VK_MENU.0)) {
                        parts.push("alt");
                    }
                    if is_down(i32::from(VK_SHIFT.0)) {
                        parts.push("shift");
                    }
                    if is_down(i32::from(VK_LWIN.0)) || is_down(i32::from(VK_RWIN.0)) {
                        parts.push("win");
                    }
                    parts.push(key_name);
                    let combo = parts.join("+");

                    // Wait briefly for the captured key to be released before re-installing the
                    // global hook so the capture keypress itself never triggers dictation.
                    let mut waited = 0_u32;
                    while is_down(vk) && waited < 75 {
                        thread::sleep(Duration::from_millis(20));
                        waited += 1;
                    }

                    let new_settings = {
                        let mut s = settings.lock().unwrap_or_else(|e| e.into_inner());
                        s.hotkey = combo.clone();
                        let _ = s.save_to_path(&settings_path);
                        s.clone()
                    };
                    coordinator.apply_settings(new_settings, false);
                    let ok = coordinator.register_hotkey().is_ok();

                    let dictation_allowed = {
                        let mut snap = snapshot.lock().unwrap_or_else(|e| e.into_inner());
                        snap.hotkey = combo.clone();
                        snap.dictation_allowed
                    };

                    let weak = dash_weak.clone();
                    let tray_w = tray_weak.clone();
                    let display = format_hotkey_display(&combo);
                    let tooltip = if dictation_allowed {
                        status_summary_title(false, &combo)
                    } else {
                        "Your trial has ended.".to_owned()
                    };
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(d) = weak.upgrade() {
                            d.set_hotkey_capturing(false);
                            d.set_hotkey_display(display.into());
                            d.set_onboarding_hotkey_ok(ok);
                        }
                        if let Some(t) = tray_w.upgrade() {
                            t.set_tray_tooltip(tooltip.into());
                        }
                    });
                    return;
                }
                thread::sleep(Duration::from_millis(20));
            }
        });
}

#[cfg(not(windows))]
fn spawn_hotkey_capture(
    _cancel: Arc<AtomicBool>,
    _dash_weak: slint::Weak<DashboardWindow>,
    _settings: Arc<Mutex<Settings>>,
    _settings_path: PathBuf,
    _coordinator: DictationCoordinator,
    _snapshot: Arc<Mutex<UiSyncSnapshot>>,
    _tray_weak: slint::Weak<EqhoTray>,
) {
    // On Linux & macOS, `GeneralTab`'s `FocusScope` captures the pressed shortcut natively
    // (works across X11, Wayland, and macOS without extra OS polling permissions).
}

#[cfg(windows)]
fn vk_to_hotkey_name(vk: i32) -> Option<&'static str> {
    match vk {
        0x20 => Some("space"),
        0x0D => Some("enter"),
        0x09 => Some("tab"),
        0x30 => Some("0"),
        0x31 => Some("1"),
        0x32 => Some("2"),
        0x33 => Some("3"),
        0x34 => Some("4"),
        0x35 => Some("5"),
        0x36 => Some("6"),
        0x37 => Some("7"),
        0x38 => Some("8"),
        0x39 => Some("9"),
        0x41 => Some("a"),
        0x42 => Some("b"),
        0x43 => Some("c"),
        0x44 => Some("d"),
        0x45 => Some("e"),
        0x46 => Some("f"),
        0x47 => Some("g"),
        0x48 => Some("h"),
        0x49 => Some("i"),
        0x4A => Some("j"),
        0x4B => Some("k"),
        0x4C => Some("l"),
        0x4D => Some("m"),
        0x4E => Some("n"),
        0x4F => Some("o"),
        0x50 => Some("p"),
        0x51 => Some("q"),
        0x52 => Some("r"),
        0x53 => Some("s"),
        0x54 => Some("t"),
        0x55 => Some("u"),
        0x56 => Some("v"),
        0x57 => Some("w"),
        0x58 => Some("x"),
        0x59 => Some("y"),
        0x5A => Some("z"),
        0x70 => Some("f1"),
        0x71 => Some("f2"),
        0x72 => Some("f3"),
        0x73 => Some("f4"),
        0x74 => Some("f5"),
        0x75 => Some("f6"),
        0x76 => Some("f7"),
        0x77 => Some("f8"),
        0x78 => Some("f9"),
        0x79 => Some("f10"),
        0x7A => Some("f11"),
        0x7B => Some("f12"),
        _ => None,
    }
}

/// Start the full Eqho desktop tray application.
pub fn run_desktop_app(
    open_dashboard: bool,
    initial_tab: Option<String>,
    dll_dir_override: Option<PathBuf>,
    demo_dashboard: bool,
    demo_license: Option<DemoLicense>,
) -> ExitCode {
    let is_isolated = demo_dashboard;
    let honoured_demo_license = if let Some(demo) = demo_license {
        if demo_license_is_honoured(is_isolated) {
            Some(demo)
        } else {
            eprintln!(
                "--demo-license needs an isolated demo run of the Store build (pass --demo-dashboard); ignoring."
            );
            None
        }
    } else {
        None
    };
    #[cfg(not(feature = "store"))]
    let _ = honoured_demo_license;

    let instance_id = if is_isolated {
        format!("{SINGLE_INSTANCE_ID}.demo.{}", std::process::id())
    } else {
        SINGLE_INSTANCE_ID.to_owned()
    };
    let mut guard: SingleInstanceGuard = match SingleInstance::acquire(&instance_id) {
        Acquire::First(g) => g,
        Acquire::AlreadyRunning => {
            println!("Eqho is already running.");
            return ExitCode::SUCCESS;
        }
    };

    let (paths, temp_demo_dir) = if is_isolated {
        let dir = std::env::temp_dir().join(format!("eqho-demo-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let demo_paths = RuntimePaths {
            settings_path: dir.join("settings.json"),
            history_path: dir.join("history.jsonl"),
            log_dir: dir.clone(),
            config_dir: dir.clone(),
        };
        (demo_paths, Some(dir))
    } else {
        (resolve_runtime_paths(), None)
    };
    let cleanup_demo_dir = |dir: &Option<PathBuf>| {
        if let Some(d) = dir {
            let _ = std::fs::remove_dir_all(d);
        }
    };

    let error_log = ErrorLog::new(&paths.log_dir, "eqho");
    install_panic_hook(error_log.clone());

    if let Err(err) = slint::BackendSelector::new()
        .renderer_name("software".into())
        .select()
    {
        error_log.log_error("slint_backend_select", &err);
        eprintln!("failed to select Slint software renderer: {err}");
        cleanup_demo_dir(&temp_demo_dir);
        return ExitCode::from(1);
    }

    let mut settings = Settings::load_from_path(&paths.settings_path);
    if is_isolated {
        settings.onboarding_done = true;
    }
    let history = TranscriptHistory::with_default_cap(&paths.history_path);
    if !is_isolated {
        let _ = history.purge_expired_trash(settings.history_trash_days, now_epoch_secs());
    }

    let platform = Arc::new(EqhoPlatformImpl::default());
    let damt_platform = Arc::new(build_damt_platform(is_isolated, &error_log));
    let (hw_class, gpu_name) = detect_hardware_info(platform.as_ref());

    let is_dark = resolve_theme_is_dark(
        &settings.theme,
        damt_platform.app_theme().is_light_app_theme(),
    );
    let light_taskbar = damt_platform
        .taskbar_theme()
        .is_light_taskbar()
        .unwrap_or(false);

    let dash_weak: Arc<Mutex<Option<slint::Weak<DashboardWindow>>>> = Arc::new(Mutex::new(None));
    let settings_arc = Arc::new(Mutex::new(settings.clone()));
    let coord_slot: Arc<OnceLock<DictationCoordinator>> = Arc::new(OnceLock::new());
    let overlay_slot: Arc<OnceLock<Arc<OverlayHandle>>> = Arc::new(OnceLock::new());
    let tray_slot: Arc<OnceLock<slint::Weak<EqhoTray>>> = Arc::new(OnceLock::new());
    let snapshot_slot: Arc<OnceLock<Arc<Mutex<UiSyncSnapshot>>>> = Arc::new(OnceLock::new());
    let open_history_requested = Arc::new(AtomicBool::new(false));
    let overlay_preview_shown = Arc::new(AtomicBool::new(false));

    let on_action: OverlayActionCallback = {
        let coord_slot = Arc::clone(&coord_slot);
        let overlay_slot = Arc::clone(&overlay_slot);
        let tray_slot = Arc::clone(&tray_slot);
        let snapshot_slot = Arc::clone(&snapshot_slot);
        let open_history_requested = Arc::clone(&open_history_requested);
        let overlay_preview_shown = Arc::clone(&overlay_preview_shown);
        let settings_arc = Arc::clone(&settings_arc);
        let settings_path = paths.settings_path.clone();
        let dash_weak = Arc::clone(&dash_weak);
        let error_log = error_log.clone();
        Arc::new(move |action| {
            let coord_slot = Arc::clone(&coord_slot);
            let overlay_slot = Arc::clone(&overlay_slot);
            let tray_slot = Arc::clone(&tray_slot);
            let snapshot_slot = Arc::clone(&snapshot_slot);
            let open_history_requested = Arc::clone(&open_history_requested);
            let overlay_preview_shown = Arc::clone(&overlay_preview_shown);
            let settings_arc = Arc::clone(&settings_arc);
            let settings_path = settings_path.clone();
            let dash_weak = Arc::clone(&dash_weak);
            let error_log = error_log.clone();
            let _ = thread::Builder::new()
                .name("eqho-overlay-action".into())
                .spawn(move || {
                    let Some(coord) = coord_slot.get().cloned() else {
                        return;
                    };
                    match action {
                        OverlayAction::Cancel | OverlayAction::Done
                            if overlay_preview_shown.swap(false, Ordering::AcqRel) =>
                        {
                            if let Some(ov) = overlay_slot.get() {
                                ov.set_finishing(false);
                                ov.hide();
                            }
                            let dash_weak_opt = dash_weak
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .clone();
                            if let Some(dw) = dash_weak_opt {
                                let _ = slint::invoke_from_event_loop(move || {
                                    if let Some(d) = dw.upgrade() {
                                        d.set_overlay_preview_active(false);
                                    }
                                });
                            }
                        }
                        OverlayAction::Cancel => coord.cancel(),
                        OverlayAction::Done => coord.deactivate(),
                        OverlayAction::ToggleDeliveryMode(mode) => coord.set_delivery_mode(&mode),
                        OverlayAction::SelectLanguage(code) => {
                            let _ = coord.set_language_live(&code);
                        }
                        OverlayAction::TogglePin(pinned) => {
                            let updated = {
                                let mut s = settings_arc.lock().unwrap_or_else(|e| e.into_inner());
                                s.overlay_controls_pinned = pinned;
                                s.clone()
                            };
                            if let Err(err) = updated.save_to_path(&settings_path) {
                                error_log.log_error("overlay_toggle_pin", &err);
                            }
                            coord.apply_settings(updated.clone(), false);
                            if let Some(ov) = overlay_slot.get() {
                                ov.update_session_meta(
                                    &updated.language,
                                    &updated.delivery_mode,
                                    updated.overlay_controls_pinned,
                                    updated.recent_languages.clone(),
                                );
                            }
                            let dash_weak_opt = dash_weak
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .clone();
                            if let Some(weak) = dash_weak_opt {
                                let _ = slint::invoke_from_event_loop(move || {
                                    if let Some(dash) = weak.upgrade() {
                                        dash.set_overlay_controls_pinned(pinned);
                                    }
                                });
                            }
                        }
                        OverlayAction::RecoveryShowText => {
                            if let Some(ov) = overlay_slot.get() {
                                ov.hide_recovery();
                            }
                            open_history_requested.store(true, Ordering::Release);
                            let dash_weak_opt = dash_weak
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .clone();
                            let tray_w = tray_slot.get().cloned();
                            let open_hist = Arc::clone(&open_history_requested);
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(weak) = dash_weak_opt
                                    && let Some(dash) = weak.upgrade()
                                {
                                    open_hist.store(false, Ordering::Release);
                                    dash.set_showing_trash(false);
                                    dash.set_active_tab("history".into());
                                    let _ = dash.show();
                                    return;
                                }
                                if let Some(tw) = tray_w
                                    && let Some(t) = tw.upgrade()
                                {
                                    t.invoke_open_dashboard();
                                }
                            });
                        }
                        OverlayAction::RecoveryCopy | OverlayAction::RecoveryDiscard => {
                            if matches!(action, OverlayAction::RecoveryCopy) {
                                let _ = coord.copy_last_dictation();
                            }
                            if let Some(ov) = overlay_slot.get() {
                                ov.hide_recovery();
                            }
                            if let Some(snap) = snapshot_slot.get() {
                                snap.lock().unwrap_or_else(|e| e.into_inner()).recovery_text = None;
                            }
                            let dash_weak_opt = dash_weak
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .clone();
                            let tray_w = tray_slot.get().cloned();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(weak) = dash_weak_opt
                                    && let Some(dash) = weak.upgrade()
                                {
                                    dash.set_recovery_text("".into());
                                }
                                if let Some(tw) = tray_w
                                    && let Some(t) = tw.upgrade()
                                {
                                    t.set_has_recovery(false);
                                }
                            });
                        }
                    }
                });
        })
    };
    let flag_provider: FlagBitmapProvider = Arc::new(render_flag_rgba);

    let overlay = Arc::new(OverlayHandle::start_with_callbacks(
        OverlayConfig::from_settings(&settings, is_dark),
        Some(on_action),
        Some(flag_provider),
    ));
    let _ = overlay_slot.set(Arc::clone(&overlay));

    let exe_path = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("Eqho"));
    let dll_dir = resolve_dll_dir(dll_dir_override.as_deref(), &exe_path);
    let model_host = ModelHost::new(ModelHostConfig::new(exe_path, dll_dir));

    let tray = match EqhoTray::new() {
        Ok(t) => t,
        Err(err) => {
            error_log.log_error("EqhoTray::new", &err);
            overlay.shutdown();
            cleanup_demo_dir(&temp_demo_dir);
            return ExitCode::from(1);
        }
    };
    let _ = tray_slot.set(tray.as_weak());

    if let Ok((w, h, rgba)) = build_tray_icon_rgba(light_taskbar, false) {
        tray.set_tray_icon(rgba_to_slint_image(w, h, &rgba));
    }
    tray.set_tray_tooltip(status_summary_title(false, &settings.hotkey).into());
    tray.set_toggle_label("Start Dictation".into());
    tray.set_dictation_allowed(true);
    tray.set_has_recovery(false);

    let snapshot = Arc::new(Mutex::new(UiSyncSnapshot {
        is_dictating: false,
        dictation_allowed: true,
        recovery_text: None,
        downloading_key: None,
        loading_key: None,
        download_progress: 0.0,
        download_detail: String::new(),
        download_done_key: None,
        download_error_key: None,
        hotkey: settings.hotkey.clone(),
        light_taskbar,
    }));
    let _ = snapshot_slot.set(Arc::clone(&snapshot));

    let ui_sink = Arc::new(AppUiSink {
        overlay: Arc::clone(&overlay),
        overlay_preview_shown: Arc::clone(&overlay_preview_shown),
        damt_platform: Arc::clone(&damt_platform),
        error_log: error_log.clone(),
        tray_weak: tray.as_weak(),
        dash_weak: Arc::clone(&dash_weak),
        snapshot: Arc::clone(&snapshot),
        history: TranscriptHistory::with_default_cap(&paths.history_path),
        settings: Arc::clone(&settings_arc),
        hw_class,
    });

    let coordinator = DictationCoordinator::new(
        settings.clone(),
        paths.settings_path.clone(),
        history,
        platform.clone(),
        Box::new(model_host),
        ui_sink.clone(),
    );
    let _ = coord_slot.set(coordinator.clone());

    let hotkey_ok = if is_isolated {
        true
    } else {
        match coordinator.register_hotkey() {
            Ok(()) => true,
            Err(err) => {
                error_log.log(Some("register_hotkey"), &err);
                false
            }
        }
    };

    #[cfg(feature = "store")]
    let store_clock: Arc<dyn Clock> = Arc::new(SystemClock);
    #[cfg(feature = "store")]
    let startup_now = store_clock.now().timestamp();

    let ctrl = Rc::new(RefCell::new(AppController {
        settings: settings_arc,
        settings_path: paths.settings_path,
        config_dir: paths.config_dir.clone(),
        history: TranscriptHistory::with_default_cap(&paths.history_path),
        coordinator: coordinator.clone(),
        overlay: Arc::clone(&overlay),
        overlay_preview_shown,
        overlay_preview_timer: slint::Timer::default(),
        platform,
        damt_platform,
        hw_class,
        gpu_name,
        dashboard: None,
        dash_weak,
        snapshot,
        mic_devices: Vec::new(),
        mic_test_stream: None,
        hotkey_ok,
        is_isolated,
        hotkey_capture_cancel: None,
        download_cancel: None,
        error_log: error_log.clone(),
        ui_sink,
        #[cfg(feature = "store")]
        license_state: LicenseState::Unknown,
        #[cfg(feature = "store")]
        license_now: startup_now,
        #[cfg(feature = "store")]
        demo_license_active: honoured_demo_license.is_some(),
    }));

    #[cfg(feature = "store")]
    let _store_recheck_timer: Option<slint::Timer> = if !is_isolated {
        let cache_path = paths.config_dir.join(LICENSE_CACHE_FILE_NAME);
        let initial_license = store_license::init(
            Rc::downgrade(&ctrl),
            cache_path,
            Arc::clone(&store_clock),
            error_log.clone(),
        );
        ctrl.borrow_mut()
            .apply_license_state(initial_license, store_clock.now().timestamp());
        store_license::refresh();
        let timer = slint::Timer::default();
        timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_secs(15 * 60),
            store_license::maybe_periodic_recheck,
        );
        Some(timer)
    } else if let Some(demo) = honoured_demo_license {
        // Honoured `--demo-license` run: set the state directly before the dashboard paints.
        // Isolated runs never call `store_license::init` / `refresh`, so no Store API call runs
        // and `license.json` is never written.
        let now = store_clock.now().timestamp();
        let state = demo_license_state(demo, now);
        ctrl.borrow_mut().apply_license_state(state, now);
        None
    } else {
        None
    };

    // Wire tray callbacks
    {
        let ctrl_w = Rc::clone(&ctrl);
        tray.on_tray_clicked(move || {
            ensure_dashboard(&ctrl_w, None);
        });
    }
    {
        let ctrl_w = Rc::clone(&ctrl);
        let open_hist = Arc::clone(&open_history_requested);
        tray.on_open_dashboard(move || {
            let tab = if open_hist.swap(false, Ordering::AcqRel) {
                Some("history")
            } else {
                None
            };
            ensure_dashboard(&ctrl_w, tab);
            if tab == Some("history") {
                let c = ctrl_w.borrow();
                if let Some(d) = &c.dashboard {
                    d.set_showing_trash(false);
                }
                c.refresh_history();
            }
        });
    }
    {
        let coord = coordinator.clone();
        tray.on_toggle_dictation(move || {
            let c = coord.clone();
            let _ = thread::Builder::new()
                .name("eqho-tray-toggle".into())
                .spawn(move || c.toggle());
        });
    }
    {
        let coord = coordinator.clone();
        tray.on_copy_last_dictation(move || {
            let _ = coord.copy_last_dictation();
        });
    }
    {
        let ctrl_w = Rc::clone(&ctrl);
        tray.on_quit_clicked(move || {
            let mut c = ctrl_w.borrow_mut();
            c.stop_mic_test();
            c.coordinator.shutdown();
            c.overlay.shutdown();
            #[cfg(feature = "store")]
            store_license::clear();
            let _ = slint::quit_event_loop();
        });
    }

    // Listen for second-instance signal to surface the dashboard.
    {
        let dash_w = Arc::clone(&ctrl.borrow().dash_weak);
        let tray_w = tray.as_weak();
        guard.listen(move || {
            let dash_w = Arc::clone(&dash_w);
            let tray_w = tray_w.clone();
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(weak) = dash_w.lock().unwrap_or_else(|e| e.into_inner()).as_ref()
                    && let Some(d) = weak.upgrade()
                {
                    let _ = d.show();
                    return;
                }
                if let Some(t) = tray_w.upgrade() {
                    t.invoke_open_dashboard();
                }
            });
        });
    }

    // Lazy dashboard: only create at startup if onboarding is incomplete, --open-dashboard was
    // passed, or this is an isolated --demo-dashboard run.
    if !settings.onboarding_done || open_dashboard || is_isolated {
        ensure_dashboard(&ctrl, initial_tab.as_deref());
    }

    // Preload model in background if onboarding is done (non-demo runs) and the model is cached.
    if settings.onboarding_done && !is_isolated {
        let model_dir = settings.resolve_model_dir();
        if whispercpp_model_path(&model_dir, &settings.model_size).is_file() {
            let coord = coordinator.clone();
            let _ = thread::Builder::new()
                .name("eqho-model-preload".into())
                .spawn(move || {
                    let _ = coord.preload_model();
                });
        }
    }

    let run_result = slint::run_event_loop_until_quit();
    if let Err(err) = &run_result {
        error_log.log_error("run_event_loop_until_quit", err);
    }

    #[cfg(feature = "store")]
    store_license::clear();
    coordinator.shutdown();
    overlay.shutdown();
    drop(ctrl);
    drop(guard);
    cleanup_demo_dir(&temp_demo_dir);

    if run_result.is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

#[cfg(feature = "store")]
mod store_license {
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::sync::Arc;

    use damt_app::ErrorLog;
    use damt_platform::Clock;
    use damt_platform_win::{RawStoreLicense, StorePurchaseOutcome};

    use crate::license::{LicenseManager, LicenseState};

    use super::{AppController, RcWeak};

    struct StoreLicenseRuntime {
        ctrl: RcWeak<RefCell<AppController>>,
        manager: LicenseManager,
        last_applied: LicenseState,
        in_flight: bool,
        error_logged: bool,
        error_log: ErrorLog,
        clock: Arc<dyn Clock>,
    }

    thread_local! {
        static RUNTIME: RefCell<Option<StoreLicenseRuntime>> = const { RefCell::new(None) };
    }

    pub fn init(
        ctrl: RcWeak<RefCell<AppController>>,
        cache_path: PathBuf,
        clock: Arc<dyn Clock>,
        error_log: ErrorLog,
    ) -> LicenseState {
        let manager = LicenseManager::new(Some(cache_path), Arc::clone(&clock));
        let initial = manager.state();
        RUNTIME.with(|slot| {
            *slot.borrow_mut() = Some(StoreLicenseRuntime {
                ctrl,
                manager,
                last_applied: initial,
                in_flight: false,
                error_logged: false,
                error_log,
                clock,
            });
        });
        initial
    }

    pub fn clear() {
        RUNTIME.with(|slot| {
            *slot.borrow_mut() = None;
        });
    }

    fn push_state(ctrl: &RcWeak<RefCell<AppController>>, state: LicenseState, now: jiff::Timestamp) {
        if let Some(rc) = ctrl.upgrade()
            && let Ok(mut c) = rc.try_borrow_mut()
        {
            c.apply_license_state(state, now);
        }
    }

    pub fn maybe_periodic_recheck() {
        let (local_update, should_refresh) = RUNTIME.with(|slot| {
            let mut borrow = slot.borrow_mut();
            let Some(rt) = borrow.as_mut() else {
                return (None, false);
            };
            let current = rt.manager.state();
            let now = rt.clock.now().timestamp();
            let update = if current != rt.last_applied {
                rt.last_applied = current;
                Some((rt.ctrl.clone(), current, now))
            } else {
                None
            };
            let due = !rt.in_flight && rt.manager.is_recheck_due();
            (update, due)
        });
        if let Some((ctrl, state, now)) = local_update {
            push_state(&ctrl, state, now);
        }
        if should_refresh {
            refresh();
        }
    }

    pub fn refresh() {
        let can_start = RUNTIME.with(|slot| {
            let mut borrow = slot.borrow_mut();
            let Some(rt) = borrow.as_mut() else {
                return false;
            };
            if rt.in_flight {
                return false;
            }
            rt.in_flight = true;
            rt.manager.mark_check_started();
            true
        });
        if !can_start {
            return;
        }

        #[cfg(windows)]
        {
            damt_platform_win::query_app_license_async(|res| {
                let mapped = res.map_err(|err| err.to_string());
                let _ = slint::invoke_from_event_loop(move || {
                    on_query_completed(mapped);
                });
            });
        }
        #[cfg(not(windows))]
        {
            on_query_completed(Err(
                "Microsoft Store licensing is only available on Windows".to_owned(),
            ));
        }
    }

    fn on_query_completed(res: Result<RawStoreLicense, String>) {
        let update = RUNTIME.with(|slot| {
            let mut borrow = slot.borrow_mut();
            let rt = borrow.as_mut()?;
            rt.in_flight = false;
            let state = rt.manager.on_query_result(res);
            rt.last_applied = state;
            if let Some(err) = rt.manager.last_error() {
                if !rt.error_logged {
                    rt.error_logged = true;
                    rt.error_log.log(Some("store_license"), err);
                }
            } else {
                rt.error_logged = false;
            }
            Some((rt.ctrl.clone(), state, rt.clock.now().timestamp()))
        });

        if let Some((ctrl, state, now)) = update {
            push_state(&ctrl, state, now);
        }
    }

    pub fn trigger_purchase(owner_hwnd: Option<isize>) {
        let Some(hwnd) = owner_hwnd else {
            RUNTIME.with(|slot| {
                if let Some(rt) = slot.borrow().as_ref() {
                    rt.error_log.log(
                        Some("store_purchase"),
                        "Dashboard window HWND unavailable for Store purchase",
                    );
                }
            });
            return;
        };

        #[cfg(windows)]
        {
            if let Err(err) =
                damt_platform_win::request_purchase_async(crate::license::STORE_ID, hwnd, |res| {
                    let mapped = res.map_err(|err| err.to_string());
                    let _ = slint::invoke_from_event_loop(move || {
                        on_purchase_completed(mapped);
                    });
                })
            {
                RUNTIME.with(|slot| {
                    if let Some(rt) = slot.borrow().as_ref() {
                        rt.error_log.log_error("store_purchase_start", &err);
                    }
                });
            }
        }
        #[cfg(not(windows))]
        {
            let _ = hwnd;
            on_purchase_completed(Err(
                "Microsoft Store purchase is only available on Windows".to_owned(),
            ));
        }
    }

    fn on_purchase_completed(res: Result<StorePurchaseOutcome, String>) {
        match res {
            Ok(outcome) if outcome.is_owned() => {
                let update = RUNTIME.with(|slot| {
                    let mut borrow = slot.borrow_mut();
                    let rt = borrow.as_mut()?;
                    let state = rt.manager.on_purchase_succeeded();
                    rt.last_applied = state;
                    Some((rt.ctrl.clone(), state, rt.clock.now().timestamp()))
                });
                if let Some((ctrl, state, now)) = update {
                    push_state(&ctrl, state, now);
                }
                refresh();
            }
            Ok(StorePurchaseOutcome::NotPurchased) => {}
            Ok(other) => {
                RUNTIME.with(|slot| {
                    if let Some(rt) = slot.borrow().as_ref() {
                        rt.error_log.log(
                            Some("store_purchase"),
                            &format!("Store purchase did not complete: {other:?}"),
                        );
                    }
                });
            }
            Err(err) => {
                RUNTIME.with(|slot| {
                    if let Some(rt) = slot.borrow().as_ref() {
                        rt.error_log.log(Some("store_purchase"), &err);
                    }
                });
            }
        }
    }
}

#[cfg(test)]
mod hotkey_match_tests {
    use super::{matches_slint_hotkey, slint_key_to_hotkey_combo};

    #[test]
    fn converts_slint_keys_to_hotkey_combo() {
        assert_eq!(
            slint_key_to_hotkey_combo(false, true, false, false, "Q").as_deref(),
            Some("alt+q")
        );
        assert_eq!(
            slint_key_to_hotkey_combo(true, false, true, false, " ").as_deref(),
            Some("ctrl+shift+space")
        );
        assert_eq!(
            slint_key_to_hotkey_combo(false, false, false, true, "\u{F70B}").as_deref(),
            Some("win+f8")
        );
        assert_eq!(
            slint_key_to_hotkey_combo(false, false, false, false, ""),
            None
        );
    }

    #[test]
    fn matches_default_alt_q_and_rejects_plain_q() {
        assert!(matches_slint_hotkey("alt+q", false, true, false, false, "q"));
        assert!(matches_slint_hotkey("alt+q", false, true, false, false, "Q"));
        assert!(matches_slint_hotkey("<alt>+q", false, true, false, false, "q"));
        assert!(!matches_slint_hotkey("alt+q", false, false, false, false, "q"));
        assert!(!matches_slint_hotkey("alt+q", true, true, false, false, "q"));
        assert!(!matches_slint_hotkey("alt+q", false, true, false, false, "w"));
    }

    #[test]
    fn matches_ctrl_shift_space_and_function_keys() {
        assert!(matches_slint_hotkey(
            "ctrl+shift+space",
            true,
            false,
            true,
            false,
            " "
        ));
        assert!(matches_slint_hotkey(
            "ctrl+q",
            true,
            false,
            false,
            false,
            "\u{0011}"
        ));
        assert!(matches_slint_hotkey(
            "win+f2",
            false,
            false,
            false,
            true,
            "\u{F705}"
        ));
        assert!(!matches_slint_hotkey(
            "win+f2",
            false,
            false,
            false,
            true,
            "\u{F704}"
        ));
    }
}
