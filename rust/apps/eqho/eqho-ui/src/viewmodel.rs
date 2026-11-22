//! Pure viewmodel helpers converting `eqho-core` domain state into Slint UI models.

use std::collections::BTreeMap;

use eqho_core::history::TranscriptEntry;
use eqho_core::modelpick::{HardwareClass, recommend_model};
use eqho_core::modelstore::{MODELS, model_info};
use eqho_core::settings::{
    LANGUAGE_TIERS, OVERLAY_POSITIONS, Settings, VOLUME_DUCK_OPTIONS, is_english_only_model,
    supported_language_name,
};
use slint::{ComponentHandle, ModelRc, VecModel};

use crate::flags::flag_image_for_language;
use crate::{DashboardWindow, HistoryRowItem, LanguageOption, ModelCardItem, Theme};

const UI_SCALE_LEVELS: [f64; 5] = [1.0, 1.25, 1.5, 1.75, 2.0];
const THEME_MODES: [&str; 3] = ["light", "dark", "system"];

/// Extract the short display name from a model's full label (e.g. `"Distil Large v3"`).
pub fn model_short_name(model_key: &str) -> String {
    model_info(model_key)
        .map(|m| m.label.split(" (").next().unwrap_or(m.label).to_owned())
        .unwrap_or_else(|| model_key.to_owned())
}

/// Format a model size in megabytes as `"1.5 GB"` or `"330 MB"`.
pub fn model_size_label(size_mb: u64) -> String {
    if size_mb >= 1000 {
        format!("{:.1} GB", size_mb as f64 / 1000.0)
    } else {
        format!("{size_mb} MB")
    }
}

/// Hardware summary banner text for the Models tab.
pub fn hardware_summary_text(hw: HardwareClass) -> String {
    match hw {
        HardwareClass::Gpu => "On this PC, speech models run on your graphics card.".to_owned(),
        HardwareClass::Cpu => {
            "On this PC, speech models run on the processor — the smaller models are much faster here."
                .to_owned()
        }
    }
}

/// Build Slint `ModelCardItem` rows for all 9 supported Whisper models.
pub fn build_model_cards(
    selected_key: &str,
    cached_keys: &[&str],
    downloading_key: Option<&str>,
    progress: f32,
    error_key: Option<&str>,
    hw: HardwareClass,
    lang: &str,
) -> Vec<ModelCardItem> {
    let recommended = recommend_model(hw, lang);
    MODELS
        .iter()
        .map(|m| {
            let is_selected = m.key == selected_key;
            let is_cached = cached_keys.contains(&m.key);
            let is_downloading = downloading_key == Some(m.key);
            let has_error = error_key == Some(m.key);

            let status_text = if has_error {
                "Download failed"
            } else if is_downloading {
                "Downloading…"
            } else if is_cached {
                "✓ Ready"
            } else {
                "Not downloaded"
            };

            let card_progress = if is_downloading {
                progress.clamp(0.0, 1.0)
            } else if is_cached {
                1.0
            } else {
                0.0
            };

            let lang_kind = if m.english_only {
                "English"
            } else {
                "Multilingual"
            };
            let detail = if m.quantized {
                format!("{lang_kind} · {} · Q5 CPU-fast", model_size_label(m.size_mb))
            } else {
                format!("{lang_kind} · {}", model_size_label(m.size_mb))
            };
            let recommendation = if m.key == recommended {
                "Recommended for this PC"
            } else {
                ""
            };

            ModelCardItem {
                key: m.key.into(),
                name: model_short_name(m.key).into(),
                detail: detail.into(),
                recommendation: recommendation.into(),
                is_english_only: m.english_only,
                is_selected,
                is_cached,
                is_downloading,
                has_error,
                progress: card_progress,
                status_text: status_text.into(),
            }
        })
        .collect()
}

/// Split model cards into `(english_optimized, multilingual)` preserving order.
pub fn split_model_cards(cards: Vec<ModelCardItem>) -> (Vec<ModelCardItem>, Vec<ModelCardItem>) {
    cards.into_iter().partition(|c| c.is_english_only)
}

/// Format a Unix epoch timestamp (seconds) as `YYYY-MM-DD HH:MM` in UTC.
pub fn format_timestamp_utc(ts_secs: f64) -> String {
    let secs = if ts_secs.is_finite() && ts_secs >= 0.0 {
        ts_secs.floor() as i64
    } else {
        0
    };
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let hour = rem / 3_600;
    let minute = (rem % 3_600) / 60;
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}")
}

/// Howard Hinnant's civil-from-days algorithm (days since 1970-01-01).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = y + i64::from(m <= 2);
    (year, m as u32, d as u32)
}

/// Convert `TranscriptEntry` items into Slint `HistoryRowItem` structs.
pub fn build_history_rows(entries: &[TranscriptEntry]) -> Vec<HistoryRowItem> {
    entries
        .iter()
        .map(|e| {
            let mut meta_parts = Vec::new();
            if !e.model.is_empty() {
                meta_parts.push(e.model.as_str());
            }
            if !e.lang.is_empty() {
                meta_parts.push(e.lang.as_str());
            }
            HistoryRowItem {
                ts_key: format!("{:.6}", e.ts).into(),
                timestamp_text: format_timestamp_utc(e.ts).into(),
                meta_text: meta_parts.join("  ·  ").into(),
                text: e.text.clone().into(),
            }
        })
        .collect()
}

/// Filter transcript entries by search query (text or `YYYY-MM-DD HH:MM` stamp) and period index:
/// `0` = All time, `1` = Today, `2` = Yesterday, `3` = Last 7 days, `4` = Last 30 days.
pub fn filter_history_entries(
    entries: &[TranscriptEntry],
    query: &str,
    period_index: i32,
    now_epoch_s: f64,
) -> Vec<TranscriptEntry> {
    let q = query.trim().to_lowercase();
    let now_day = (now_epoch_s.floor() as i64).div_euclid(86_400);
    let today_start = (now_day * 86_400) as f64;
    let yesterday_start = ((now_day - 1) * 86_400) as f64;
    let week_start = ((now_day - 6) * 86_400) as f64;
    let month_start = ((now_day - 29) * 86_400) as f64;

    entries
        .iter()
        .filter(|e| {
            let period_ok = match period_index {
                1 => e.ts >= today_start,
                2 => e.ts >= yesterday_start && e.ts < today_start,
                3 => e.ts >= week_start,
                4 => e.ts >= month_start,
                _ => true,
            };
            if !period_ok {
                return false;
            }
            if q.is_empty() {
                return true;
            }
            e.text.to_lowercase().contains(&q)
                || format_timestamp_utc(e.ts).to_lowercase().contains(&q)
        })
        .cloned()
        .collect()
}

/// Format a stored hotkey string (e.g. `"alt+q"`) into display form (`"Alt + Q"`).
pub fn format_hotkey_display(hotkey: &str) -> String {
    hotkey
        .split('+')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(|part| match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => "Ctrl".to_owned(),
            "alt" => "Alt".to_owned(),
            "shift" => "Shift".to_owned(),
            "win" | "super" | "meta" | "cmd" => "Win".to_owned(),
            "space" => "Space".to_owned(),
            "esc" | "escape" => "Esc".to_owned(),
            other => {
                let mut chars = other.chars();
                match chars.next() {
                    Some(first) => {
                        let mut out = first.to_uppercase().to_string();
                        out.push_str(chars.as_str());
                        out
                    }
                    None => String::new(),
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" + ")
}

/// Return the warning text when a non-English language is paired with an English-only model.
pub fn language_warning_text(language: &str, model_size: &str, hw: HardwareClass) -> String {
    if language == "en" || !is_english_only_model(model_size) {
        return String::new();
    }
    let lang_name = supported_language_name(language).unwrap_or(language);
    let rec = recommend_model(hw, language);
    let rec_label = model_short_name(rec);
    format!(
        "The current speech model only understands English. For {lang_name}, choose {rec_label} in Eqho → Models."
    )
}

/// Status summary title for tray tooltip / window status.
pub fn status_summary_title(is_dictating: bool, hotkey: &str) -> String {
    let hk = format_hotkey_display(hotkey);
    if is_dictating {
        format!("Listening… (press {hk} to stop)")
    } else {
        format!("Ready — press {hk} to dictate")
    }
}

/// Parse `spoken => written` lines into a sorted replacement map.
pub fn parse_replacements_text(raw: &str) -> BTreeMap<String, String> {
    let mut rules = BTreeMap::new();
    for line in raw.lines() {
        if let Some((src, dst)) = line.split_once("=>") {
            let src = src.trim();
            let dst = dst.trim();
            if !src.is_empty() {
                rules.insert(src.to_owned(), dst.to_owned());
            }
        }
    }
    rules
}

/// Format replacement rules as `spoken => written` lines.
pub fn format_replacements_text(map: &BTreeMap<String, String>) -> String {
    map.iter()
        .map(|(k, v)| format!("{k} => {v}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Parse `app.exe = typing` (or `= clipboard`) lines into a sorted paste-rules map.
pub fn parse_paste_rules_text(raw: &str) -> BTreeMap<String, String> {
    let mut rules = BTreeMap::new();
    for line in raw.lines() {
        if let Some((app, mode)) = line.split_once('=') {
            let app = app.trim().to_ascii_lowercase();
            let mode = mode.trim().to_ascii_lowercase();
            if !app.is_empty() && (mode == "typing" || mode == "clipboard") {
                rules.insert(app, mode);
            }
        }
    }
    rules
}

/// Format per-app paste rules as `app.exe = mode` lines.
pub fn format_paste_rules_text(map: &BTreeMap<String, String>) -> String {
    map.iter()
        .map(|(k, v)| format!("{k} = {v}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Display names for all 99 supported Whisper languages in tier order.
pub fn language_display_names() -> Vec<String> {
    LANGUAGE_TIERS
        .iter()
        .map(|e| e.name.to_owned())
        .collect()
}

/// Build Slint `LanguageOption` rows with crisp country flags for all 99 supported languages.
pub fn build_language_options() -> Vec<LanguageOption> {
    LANGUAGE_TIERS
        .iter()
        .map(|e| LanguageOption {
            label: e.name.into(),
            flag: flag_image_for_language(e.code),
        })
        .collect()
}

/// Build onboarding Step 4 recommended model copy: `(summary, direct_download_url)`.
pub fn onboarding_model_copy(hw: HardwareClass, lang: &str) -> (String, String) {
    let rec_key = recommend_model(hw, lang);
    let rec_info = model_info(rec_key);
    let rec_size = rec_info
        .map(|m| model_size_label(m.size_mb))
        .unwrap_or_default();
    let rec_url = rec_info
        .map(|m| m.download_url.to_owned())
        .unwrap_or_default();
    let summary = format!(
        "Eqho needs to download its speech model once: {}, about {rec_size}. It stays on this computer.",
        model_short_name(rec_key)
    );
    (summary, rec_url)
}

/// Find the dropdown index for a language code (defaults to `0` = English).
pub fn language_index_for_code(code: &str) -> i32 {
    LANGUAGE_TIERS
        .iter()
        .position(|e| e.code == code)
        .unwrap_or(0) as i32
}

/// Resolve a language code from a dropdown index (defaults to `"en"`).
pub fn language_code_for_index(idx: i32) -> &'static str {
    LANGUAGE_TIERS
        .get(idx.max(0) as usize)
        .map(|e| e.code)
        .unwrap_or("en")
}

pub fn overlay_position_index(pos: &str) -> i32 {
    OVERLAY_POSITIONS
        .iter()
        .position(|&p| p == pos)
        .unwrap_or(0) as i32
}

pub fn overlay_position_key(idx: i32) -> &'static str {
    OVERLAY_POSITIONS
        .get(idx.max(0) as usize)
        .copied()
        .unwrap_or("bottom-center")
}

pub fn volume_duck_index(opt: &str) -> i32 {
    VOLUME_DUCK_OPTIONS
        .iter()
        .position(|(k, _)| *k == opt)
        .unwrap_or(0) as i32
}

pub fn volume_duck_key(idx: i32) -> &'static str {
    VOLUME_DUCK_OPTIONS
        .get(idx.max(0) as usize)
        .map(|(k, _)| *k)
        .unwrap_or("off")
}

pub fn theme_mode_index(theme: &str) -> i32 {
    THEME_MODES
        .iter()
        .position(|&t| t == theme)
        .unwrap_or(1) as i32
}

pub fn theme_mode_key(idx: i32) -> &'static str {
    THEME_MODES
        .get(idx.max(0) as usize)
        .copied()
        .unwrap_or("dark")
}

pub fn ui_scale_index(scale: f64) -> i32 {
    UI_SCALE_LEVELS
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            (*a - scale)
                .abs()
                .partial_cmp(&(*b - scale).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(i, _)| i as i32)
        .unwrap_or(1)
}

pub fn ui_scale_value(idx: i32) -> f64 {
    UI_SCALE_LEVELS
        .get(idx.max(0) as usize)
        .copied()
        .unwrap_or(1.25)
}

/// Push `Settings` and hardware metadata onto a live `DashboardWindow`.
pub fn apply_settings_to_window(
    window: &DashboardWindow,
    settings: &Settings,
    hw: HardwareClass,
    is_dark: bool,
) {
    window.global::<Theme>().set_dark(is_dark);

    window.set_language_options(ModelRc::new(VecModel::from(build_language_options())));
    window.set_language_index(language_index_for_code(&settings.language));
    window.set_language_warning(
        language_warning_text(&settings.language, &settings.model_size, hw).into(),
    );

    window.set_vad_sensitivity_pct((settings.vad_sensitivity * 100.0).round() as i32);
    window.set_hotkey_display(format_hotkey_display(&settings.hotkey).into());
    window.set_hotkey_mode_index(if settings.hotkey_mode == "hold" { 1 } else { 0 });
    window.set_initial_prompt(settings.initial_prompt.clone().into());
    window.set_format_cleanup(settings.format_cleanup);
    window.set_sound_feedback(settings.sound_feedback);
    window.set_history_enabled(settings.history_enabled);
    window.set_paste_mode_index(if settings.auto_paste { 0 } else { 1 });

    window.set_hardware_summary(hardware_summary_text(hw).into());
    window.set_model_dir_display(settings.resolve_model_dir().display().to_string().into());

    window.set_theme_mode_index(theme_mode_index(&settings.theme));
    window.set_ui_scale_index(ui_scale_index(settings.ui_scale));
    window.set_remove_fillers(settings.remove_fillers);
    window.set_voice_commands(settings.voice_commands);
    window.set_replacements_text(format_replacements_text(&settings.replacements).into());
    window.set_paste_rules_text(format_paste_rules_text(&settings.paste_rules).into());
    window.set_volume_duck_index(volume_duck_index(&settings.volume_duck));
    window.set_start_with_windows(settings.start_with_windows);

    window.set_overlay_enabled(settings.overlay_enabled);
    window.set_overlay_show_level(settings.overlay_show_level);
    window.set_overlay_position_index(overlay_position_index(&settings.overlay_position));
    window.set_overlay_pill_scale_pct((settings.overlay_pill_scale * 100.0).round() as i32);
    window.set_overlay_see_through_pct(((1.0 - settings.overlay_opacity) * 100.0).round() as i32);
    window.set_overlay_panel_see_through_pct(
        ((1.0 - settings.overlay_panel_opacity) * 100.0).round() as i32,
    );
    window.set_overlay_font_size(settings.overlay_font_size);

    window.set_onboarding_active(!settings.onboarding_done);
    window.set_onboarding_duck_mute(settings.volume_duck == "mute");
    let (model_summary, model_url) = onboarding_model_copy(hw, &settings.language);
    window.set_onboarding_model_summary(model_summary.into());
    window.set_onboarding_model_url(model_url.into());
    let hk = format_hotkey_display(&settings.hotkey);
    let how = if settings.hotkey_mode == "hold" {
        format!("Hold {hk} down, say a sentence, then let go.")
    } else {
        format!("Press {hk}, say a sentence, press {hk} again.")
    };
    window.set_onboarding_try_hint(
        format!("{how} Your words appear where your cursor is. Try it here:").into(),
    );
}
