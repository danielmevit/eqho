//! Full live desktop application controller (`run_desktop_app`).

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use damt_app::{Acquire, ErrorLog, SingleInstance, SingleInstanceGuard, install_panic_hook};
use damt_platform::PlatformServices;
use eqho_core::history::TranscriptHistory;
use eqho_core::modelpick::HardwareClass;
use eqho_core::modelstore::{MODELS, whispercpp_model_path};
use eqho_core::settings::Settings;
use eqho_core::vad::level_meter;
use eqho_engine::{DictationCoordinator, UiEventSink};
use eqho_host::downloader::download_ggml_model;
use eqho_host::{ModelHost, ModelHostConfig};
use eqho_overlay::{OverlayConfig, OverlayHandle};
use eqho_platform::{AudioBackend, AudioDeviceInfo, AudioStreamHandle, ClipboardService, GpuProbe, MicPrivacyProbe};
use eqho_ui::{
    DashboardWindow, EqhoTray, Theme, apply_settings_to_window, build_history_rows,
    build_model_cards, build_onboarding_model_options, filter_history_entries,
    language_code_for_index, language_warning_text, model_short_name,
    onboarding_model_copy_for_key, overlay_position_key, parse_paste_rules_text,
    parse_replacements_text, split_model_cards, status_summary_title, theme_mode_key,
    ui_scale_value, volume_duck_key,
};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::assets::{
    LOGO_32, LOGO_HORIZONTAL_DARK, LOGO_HORIZONTAL_LIGHT, build_tray_icon_rgba,
    decode_png_rgba, rgba_to_slint_image,
};
use crate::paths::{resolve_dll_dir, resolve_runtime_paths, resolve_theme_is_dark};

const SINGLE_INSTANCE_ID: &str = "xyz.damt.eqho.rust09";

#[cfg(windows)]
type EqhoPlatformImpl = eqho_platform_win::WinEqhoPlatform;
#[cfg(not(windows))]
type EqhoPlatformImpl = eqho_platform::UnsupportedEqhoPlatform;

#[cfg(windows)]
type DamtPlatformImpl = damt_platform_win::WinPlatform;
#[cfg(not(windows))]
type DamtPlatformImpl = damt_platform::Unsupported;

fn build_damt_platform() -> DamtPlatformImpl {
    #[cfg(windows)]
    {
        let notifier = damt_platform_win::WinNotifier::new(
            SINGLE_INSTANCE_ID,
            "Eqho (0.9 Dev)",
            "Eqho-Rust-0.9",
            crate::assets::LOGO_32_WHITE,
        );
        damt_platform_win::WinPlatform::new("Eqho-Rust-0.9").with_notifier(notifier)
    }
    #[cfg(not(windows))]
    {
        damt_platform::Unsupported
    }
}

fn detect_hardware_class(platform: &dyn GpuProbe) -> HardwareClass {
    const MIN_VRAM: u64 = 2 * 1024 * 1024 * 1024;
    let has_gpu = platform
        .list_adapters()
        .iter()
        .any(|a| a.dedicated_video_memory_bytes >= MIN_VRAM);
    if has_gpu {
        HardwareClass::Gpu
    } else {
        HardwareClass::Cpu
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
    recovery_text: Option<String>,
    downloading_key: Option<String>,
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
                tray.set_toggle_label(
                    if snap.is_dictating {
                        "Stop Dictation"
                    } else {
                        "Start Dictation"
                    }
                    .into(),
                );
                tray.set_has_recovery(snap.recovery_text.is_some());
                tray.set_tray_tooltip(status_summary_title(snap.is_dictating, &snap.hotkey).into());
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
                    snap.download_progress,
                    snap.download_error_key.as_deref(),
                    hw,
                    &settings.language,
                );
                let (en_cards, multi_cards) = split_model_cards(cards);
                dash.set_english_models(ModelRc::new(VecModel::from(en_cards)));
                dash.set_multilingual_models(ModelRc::new(VecModel::from(multi_cards)));
            }
        });
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
        self.overlay.show(text);
        self.set_step6_hint(text);
    }

    fn update_overlay_text(&self, text: &str) {
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
        if let Some(dash_weak) = dash_weak_opt {
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(dash) = dash_weak.upgrade() {
                    dash.set_onboarding_is_dictating(active);
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
        let is_error = lower.contains("failed");
        let is_ready = message.eq_ignore_ascii_case("ready");
        let is_active = !is_error && !is_ready && (fraction.is_some() || lower.contains("loading") || lower.contains("downloading"));
        let clamped_frac = fraction.map(|f| f.clamp(0.0, 1.0));

        let (progress_val, detail_text) = {
            let mut snap = self.snapshot.lock().unwrap_or_else(|e| e.into_inner());
            if is_error {
                snap.downloading_key = None;
                snap.download_done_key = None;
                snap.download_error_key = Some(model_key.to_owned());
                snap.download_detail = message.to_owned();
                (0.0_f32, message.to_owned())
            } else if is_ready {
                snap.downloading_key = None;
                snap.download_progress = 1.0;
                snap.download_error_key = None;
                snap.download_done_key = Some(model_key.to_owned());
                snap.download_detail = "Ready".to_owned();
                (1.0_f32, "Ready".to_owned())
            } else {
                snap.downloading_key = Some(model_key.to_owned());
                if let Some(frac) = clamped_frac {
                    snap.download_progress = frac;
                }
                snap.download_error_key = None;
                snap.download_done_key = None;
                snap.download_detail = message.to_owned();
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
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(dash) = dash_weak.upgrade() {
                    dash.set_bg_download_active(is_active);
                    dash.set_bg_download_done(is_ready);
                    dash.set_bg_download_error(is_error);
                    dash.set_bg_download_model(model_name.into());
                    dash.set_bg_download_progress(progress_val);
                    dash.set_bg_download_pct(pct);
                    dash.set_bg_download_detail(detail_text.into());
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
            self.append_step6_text(
                t,
                Some("✓ Dictation worked! Try another sentence or click Finish."),
            );
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
        let entries = self.history.read_all();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(dash) = dash_weak.upgrade() {
                let query = dash.get_history_query().to_string();
                let period = dash.get_history_period_index();
                let filtered = filter_history_entries(&entries, &query, period, now_epoch_secs());
                let rows = build_history_rows(&filtered);
                dash.set_history_rows(ModelRc::new(VecModel::from(rows)));
            }
        });
    }
}

struct AppController {
    settings: Arc<Mutex<Settings>>,
    settings_path: PathBuf,
    config_dir: PathBuf,
    history: TranscriptHistory,
    coordinator: DictationCoordinator,
    overlay: Arc<OverlayHandle>,
    platform: Arc<EqhoPlatformImpl>,
    damt_platform: Arc<DamtPlatformImpl>,
    hw_class: HardwareClass,
    dashboard: Option<DashboardWindow>,
    dash_weak: Arc<Mutex<Option<slint::Weak<DashboardWindow>>>>,
    snapshot: Arc<Mutex<UiSyncSnapshot>>,
    mic_devices: Vec<AudioDeviceInfo>,
    mic_test_stream: Option<Box<dyn AudioStreamHandle>>,
    hotkey_ok: bool,
    hotkey_capture_cancel: Option<Arc<AtomicBool>>,
    error_log: ErrorLog,
    ui_sink: Arc<AppUiSink>,
}

impl AppController {
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
            let logo_bytes = if is_dark {
                LOGO_HORIZONTAL_DARK
            } else {
                LOGO_HORIZONTAL_LIGHT
            };
            if let Ok((w, h, rgba)) = decode_png_rgba(logo_bytes) {
                dash.set_logo_image(rgba_to_slint_image(w, h, &rgba));
            }
        }
    }

    fn refresh_history(&self) {
        let Some(dash) = &self.dashboard else {
            return;
        };
        let entries = self.history.read_all();
        let query = dash.get_history_query().to_string();
        let period = dash.get_history_period_index();
        let filtered = filter_history_entries(&entries, &query, period, now_epoch_secs());
        let rows = build_history_rows(&filtered);
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
            snap.download_progress,
            snap.download_error_key.as_deref(),
            self.hw_class,
            &s.language,
        );
        let (en_cards, multi_cards) = split_model_cards(cards);
        dash.set_english_models(ModelRc::new(VecModel::from(en_cards)));
        dash.set_multilingual_models(ModelRc::new(VecModel::from(multi_cards)));
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

    fn start_download(&self, model_key: String) {
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
                let res = download_ggml_model(&model_key, &model_dir, &mut |msg, frac| {
                    ui.on_model_progress(&model_key, msg, frac);
                });
                match res {
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
                    Err(err) => {
                        let msg = format!("Download failed: {err}");
                        ui.on_model_progress(&model_key, &msg, None);
                        ui.refresh_models_in_dash();
                    }
                }
            });
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

                if let Ok((w, h, rgba)) = decode_png_rgba(LOGO_32) {
                    dash.set_window_icon(rgba_to_slint_image(w, h, &rgba));
                }

                let logo_bytes = if is_dark {
                    LOGO_HORIZONTAL_DARK
                } else {
                    LOGO_HORIZONTAL_LIGHT
                };
                if let Ok((w, h, rgba)) = decode_png_rgba(logo_bytes) {
                    dash.set_logo_image(rgba_to_slint_image(w, h, &rgba));
                }

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
            }
            c.refresh_models();
            c.refresh_onboarding_models(Some(&initial_model_key));
            c.refresh_history();
        }

        wire_dashboard_callbacks(ctrl);
    }

    let c = ctrl.borrow();
    if let Some(dash) = &c.dashboard {
        if let Some(t) = tab {
            dash.set_active_tab(t.into());
        }
        let _ = dash.show();
    }
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
            let _ = ctrl_w.borrow().coordinator.copy_last_dictation();
            if let Some(d) = dash_w.upgrade() {
                d.set_recovery_text("".into());
            }
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        let dash_w = dash.as_weak();
        dash.on_dismiss_recovery(move || {
            ctrl_w
                .borrow()
                .snapshot
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .recovery_text = None;
            ctrl_w.borrow().ui_sink.refresh_tray();
            if let Some(d) = dash_w.upgrade() {
                d.set_recovery_text("".into());
            }
        });
    }

    // TopBar
    {
        let dash_w = dash.as_weak();
        dash.on_tab_selected(move |t| {
            if let Some(d) = dash_w.upgrade() {
                d.set_active_tab(t);
            }
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
                s.model_size.clone()
            };
            c.save_settings(false);
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
        let dash_w = dash.as_weak();
        dash.on_select_model(move |key| {
            let c = ctrl_w.borrow();
            let lang = {
                let mut s = c.settings.lock().unwrap_or_else(|e| e.into_inner());
                s.model_size = key.to_string();
                s.language.clone()
            };
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
            ctrl_w.borrow().start_download(key.to_string());
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

    // History tab
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_history_filter_changed(move || {
            ctrl_w.borrow().refresh_history();
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_copy_history_entry(move |txt| {
            let _ = ctrl_w.borrow().platform.set_text(&txt, false);
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_delete_history_entry(move |ts_key| {
            if let Ok(ts) = ts_key.parse::<f64>() {
                let c = ctrl_w.borrow();
                let _ = c.history.delete(ts);
                c.refresh_history();
            }
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_clear_history(move || {
            let c = ctrl_w.borrow();
            let _ = c.history.clear();
            c.refresh_history();
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_export_history(move || {
            let c = ctrl_w.borrow();
            let dest = c.config_dir.join("eqho-history.txt");
            if let Ok(count) = c.history.export_txt(&dest) {
                let msg = format!("Exported {count} entries to {}", dest.display());
                c.ui_sink.notify(&msg);
            }
        });
    }

    // Settings tab
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_theme_mode_changed(move |idx| {
            let c = ctrl_w.borrow();
            c.settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .theme = theme_mode_key(idx).to_owned();
            c.save_settings(false);
            c.apply_theme_and_overlay();
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
        dash.on_save_replacements(move |txt| {
            let c = ctrl_w.borrow();
            c.settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .replacements = parse_replacements_text(&txt);
            c.save_settings(false);
        });
    }
    {
        let ctrl_w = Rc::clone(ctrl);
        dash.on_save_paste_rules(move |txt| {
            let c = ctrl_w.borrow();
            c.settings
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .paste_rules = parse_paste_rules_text(&txt);
            c.save_settings(false);
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
            ctrl_w.borrow().start_download(key);
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
            let (model_size, model_dir) = {
                let mut s = c.settings.lock().unwrap_or_else(|e| e.into_inner());
                s.onboarding_done = true;
                (s.model_size.clone(), s.resolve_model_dir())
            };
            c.save_settings(false);
            if let Some(d) = dash_w.upgrade() {
                d.set_onboarding_active(false);
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

fn open_folder(path: &Path) {
    #[cfg(windows)]
    {
        let _ = Command::new("explorer.exe").arg(path).spawn();
    }
    #[cfg(not(windows))]
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
    #[cfg(not(windows))]
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

                    {
                        let mut snap = snapshot.lock().unwrap_or_else(|e| e.into_inner());
                        snap.hotkey = combo.clone();
                    }

                    let weak = dash_weak.clone();
                    let tray_w = tray_weak.clone();
                    let display = format_hotkey_display(&combo);
                    let tooltip = status_summary_title(false, &combo);
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
    cancel: Arc<AtomicBool>,
    dash_weak: slint::Weak<DashboardWindow>,
    _settings: Arc<Mutex<Settings>>,
    _settings_path: PathBuf,
    _coordinator: DictationCoordinator,
    _snapshot: Arc<Mutex<UiSyncSnapshot>>,
    _tray_weak: slint::Weak<EqhoTray>,
) {
    let _ = cancel;
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(d) = dash_weak.upgrade() {
            d.set_hotkey_capturing(false);
        }
    });
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
) -> ExitCode {
    let mut guard: SingleInstanceGuard = match SingleInstance::acquire(SINGLE_INSTANCE_ID) {
        Acquire::First(g) => g,
        Acquire::AlreadyRunning => {
            println!("Eqho is already running.");
            return ExitCode::SUCCESS;
        }
    };

    let paths = resolve_runtime_paths();
    let error_log = ErrorLog::new(&paths.log_dir, "eqho");
    install_panic_hook(error_log.clone());

    if let Err(err) = slint::BackendSelector::new()
        .renderer_name("software".into())
        .select()
    {
        error_log.log_error("slint_backend_select", &err);
        eprintln!("failed to select Slint software renderer: {err}");
        return ExitCode::from(1);
    }

    let settings = Settings::load_from_path(&paths.settings_path);
    let history = TranscriptHistory::with_default_cap(&paths.history_path);

    let platform = Arc::new(EqhoPlatformImpl::default());
    let damt_platform = Arc::new(build_damt_platform());
    let hw_class = detect_hardware_class(platform.as_ref());

    let is_dark = resolve_theme_is_dark(
        &settings.theme,
        damt_platform.app_theme().is_light_app_theme(),
    );
    let light_taskbar = damt_platform
        .taskbar_theme()
        .is_light_taskbar()
        .unwrap_or(false);

    let overlay = Arc::new(OverlayHandle::start(OverlayConfig::from_settings(
        &settings, is_dark,
    )));

    let exe_path = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("Eqho"));
    let dll_dir = resolve_dll_dir(dll_dir_override.as_deref(), &exe_path);
    let model_host = ModelHost::new(ModelHostConfig::new(exe_path, dll_dir));

    let tray = match EqhoTray::new() {
        Ok(t) => t,
        Err(err) => {
            error_log.log_error("EqhoTray::new", &err);
            overlay.shutdown();
            return ExitCode::from(1);
        }
    };

    if let Ok((w, h, rgba)) = build_tray_icon_rgba(light_taskbar, false) {
        tray.set_tray_icon(rgba_to_slint_image(w, h, &rgba));
    }
    tray.set_tray_tooltip(status_summary_title(false, &settings.hotkey).into());
    tray.set_toggle_label("Start Dictation".into());
    tray.set_has_recovery(false);

    let snapshot = Arc::new(Mutex::new(UiSyncSnapshot {
        is_dictating: false,
        recovery_text: None,
        downloading_key: None,
        download_progress: 0.0,
        download_detail: String::new(),
        download_done_key: None,
        download_error_key: None,
        hotkey: settings.hotkey.clone(),
        light_taskbar,
    }));
    let dash_weak = Arc::new(Mutex::new(None));
    let settings_arc = Arc::new(Mutex::new(settings.clone()));

    let ui_sink = Arc::new(AppUiSink {
        overlay: Arc::clone(&overlay),
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

    let hotkey_ok = match coordinator.register_hotkey() {
        Ok(()) => true,
        Err(err) => {
            error_log.log(Some("register_hotkey"), &err);
            false
        }
    };

    let ctrl = Rc::new(RefCell::new(AppController {
        settings: settings_arc,
        settings_path: paths.settings_path,
        config_dir: paths.config_dir,
        history: TranscriptHistory::with_default_cap(&paths.history_path),
        coordinator: coordinator.clone(),
        overlay: Arc::clone(&overlay),
        platform,
        damt_platform,
        hw_class,
        dashboard: None,
        dash_weak,
        snapshot,
        mic_devices: Vec::new(),
        mic_test_stream: None,
        hotkey_ok,
        hotkey_capture_cancel: None,
        error_log: error_log.clone(),
        ui_sink,
    }));

    // Wire tray callbacks
    {
        let ctrl_w = Rc::clone(&ctrl);
        tray.on_tray_clicked(move || {
            ensure_dashboard(&ctrl_w, None);
        });
    }
    {
        let ctrl_w = Rc::clone(&ctrl);
        tray.on_open_dashboard(move || {
            ensure_dashboard(&ctrl_w, None);
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

    // Lazy dashboard: only create at startup if onboarding is incomplete or --open-dashboard was passed.
    if !settings.onboarding_done || open_dashboard {
        ensure_dashboard(&ctrl, initial_tab.as_deref());
    }

    // Preload model in background if onboarding is done and the model file is already cached.
    if settings.onboarding_done {
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

    if let Err(err) = slint::run_event_loop_until_quit() {
        error_log.log_error("run_event_loop_until_quit", &err);
        coordinator.shutdown();
        overlay.shutdown();
        return ExitCode::from(1);
    }

    coordinator.shutdown();
    overlay.shutdown();
    drop(guard);
    ExitCode::SUCCESS
}
