//! User preferences persisted to a JSON config file, with 0.8.11 compatibility.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

/// Default per-user config folder name (`%APPDATA%\Eqho` on Windows).
pub const DEFAULT_CONFIG_DIR_NAME: &str = "Eqho";

pub const HOTKEY_MODES: [&str; 2] = ["toggle", "hold"];

pub const DELIVERY_MODES: [&str; 2] = ["batch", "live"];

pub const MAX_RECENT_LANGUAGES: usize = 8;

pub const OVERLAY_POSITIONS: [&str; 6] = [
    "bottom-center",
    "top-center",
    "top-left",
    "top-right",
    "bottom-left",
    "bottom-right",
];

pub const OVERLAY_MONITORS: [&str; 3] = ["active", "cursor", "primary"];

pub const VOLUME_DUCK_OPTIONS: [(&str, Option<f32>); 5] = [
    ("off", None),
    ("50%", Some(0.5)),
    ("25%", Some(0.25)),
    ("10%", Some(0.10)),
    ("mute", Some(0.0)),
];

pub const HISTORY_TRASH_OPTIONS: [(&str, i32); 5] = [
    ("7 days", 7),
    ("14 days", 14),
    ("30 days", 30),
    ("90 days", 90),
    ("Never", 0),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LanguageTier {
    Tuned,
    Strong,
    Experimental,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LanguageEntry {
    pub code: &'static str,
    pub name: &'static str,
    pub tier: LanguageTier,
}

/// All 99 supported Whisper languages across three honesty tiers
/// (13 Tuned, 20 Strong, 66 Experimental), matching Eqho 0.8.11 verbatim.
pub const LANGUAGE_TIERS: &[LanguageEntry] = &[
    // Tuned (13)
    LanguageEntry { code: "en", name: "English", tier: LanguageTier::Tuned },
    LanguageEntry { code: "es", name: "Spanish", tier: LanguageTier::Tuned },
    LanguageEntry { code: "zh", name: "Mandarin", tier: LanguageTier::Tuned },
    LanguageEntry { code: "ja", name: "Japanese", tier: LanguageTier::Tuned },
    LanguageEntry { code: "ko", name: "Korean", tier: LanguageTier::Tuned },
    LanguageEntry { code: "vi", name: "Vietnamese", tier: LanguageTier::Tuned },
    LanguageEntry { code: "ar", name: "Arabic", tier: LanguageTier::Tuned },
    LanguageEntry { code: "uk", name: "Ukrainian", tier: LanguageTier::Tuned },
    LanguageEntry { code: "fr", name: "French", tier: LanguageTier::Tuned },
    LanguageEntry { code: "de", name: "German", tier: LanguageTier::Tuned },
    LanguageEntry { code: "pt", name: "Portuguese", tier: LanguageTier::Tuned },
    LanguageEntry { code: "ru", name: "Russian", tier: LanguageTier::Tuned },
    LanguageEntry { code: "it", name: "Italian", tier: LanguageTier::Tuned },
    // Strong (20)
    LanguageEntry { code: "nl", name: "Dutch", tier: LanguageTier::Strong },
    LanguageEntry { code: "pl", name: "Polish", tier: LanguageTier::Strong },
    LanguageEntry { code: "tr", name: "Turkish", tier: LanguageTier::Strong },
    LanguageEntry { code: "sv", name: "Swedish", tier: LanguageTier::Strong },
    LanguageEntry { code: "da", name: "Danish", tier: LanguageTier::Strong },
    LanguageEntry { code: "no", name: "Norwegian", tier: LanguageTier::Strong },
    LanguageEntry { code: "fi", name: "Finnish", tier: LanguageTier::Strong },
    LanguageEntry { code: "cs", name: "Czech", tier: LanguageTier::Strong },
    LanguageEntry { code: "ro", name: "Romanian", tier: LanguageTier::Strong },
    LanguageEntry { code: "hu", name: "Hungarian", tier: LanguageTier::Strong },
    LanguageEntry { code: "el", name: "Greek", tier: LanguageTier::Strong },
    LanguageEntry { code: "bg", name: "Bulgarian", tier: LanguageTier::Strong },
    LanguageEntry { code: "hr", name: "Croatian", tier: LanguageTier::Strong },
    LanguageEntry { code: "sk", name: "Slovak", tier: LanguageTier::Strong },
    LanguageEntry { code: "ca", name: "Catalan", tier: LanguageTier::Strong },
    LanguageEntry { code: "hi", name: "Hindi", tier: LanguageTier::Strong },
    LanguageEntry { code: "id", name: "Indonesian", tier: LanguageTier::Strong },
    LanguageEntry { code: "ms", name: "Malay", tier: LanguageTier::Strong },
    LanguageEntry { code: "th", name: "Thai", tier: LanguageTier::Strong },
    LanguageEntry { code: "he", name: "Hebrew", tier: LanguageTier::Strong },
    // Experimental (66)
    LanguageEntry { code: "af", name: "Afrikaans", tier: LanguageTier::Experimental },
    LanguageEntry { code: "sq", name: "Albanian", tier: LanguageTier::Experimental },
    LanguageEntry { code: "am", name: "Amharic", tier: LanguageTier::Experimental },
    LanguageEntry { code: "hy", name: "Armenian", tier: LanguageTier::Experimental },
    LanguageEntry { code: "as", name: "Assamese", tier: LanguageTier::Experimental },
    LanguageEntry { code: "az", name: "Azerbaijani", tier: LanguageTier::Experimental },
    LanguageEntry { code: "ba", name: "Bashkir", tier: LanguageTier::Experimental },
    LanguageEntry { code: "eu", name: "Basque", tier: LanguageTier::Experimental },
    LanguageEntry { code: "be", name: "Belarusian", tier: LanguageTier::Experimental },
    LanguageEntry { code: "bn", name: "Bengali", tier: LanguageTier::Experimental },
    LanguageEntry { code: "bs", name: "Bosnian", tier: LanguageTier::Experimental },
    LanguageEntry { code: "br", name: "Breton", tier: LanguageTier::Experimental },
    LanguageEntry { code: "my", name: "Burmese", tier: LanguageTier::Experimental },
    LanguageEntry { code: "et", name: "Estonian", tier: LanguageTier::Experimental },
    LanguageEntry { code: "fo", name: "Faroese", tier: LanguageTier::Experimental },
    LanguageEntry { code: "gl", name: "Galician", tier: LanguageTier::Experimental },
    LanguageEntry { code: "ka", name: "Georgian", tier: LanguageTier::Experimental },
    LanguageEntry { code: "gu", name: "Gujarati", tier: LanguageTier::Experimental },
    LanguageEntry { code: "ht", name: "Haitian Creole", tier: LanguageTier::Experimental },
    LanguageEntry { code: "ha", name: "Hausa", tier: LanguageTier::Experimental },
    LanguageEntry { code: "haw", name: "Hawaiian", tier: LanguageTier::Experimental },
    LanguageEntry { code: "is", name: "Icelandic", tier: LanguageTier::Experimental },
    LanguageEntry { code: "jw", name: "Javanese", tier: LanguageTier::Experimental },
    LanguageEntry { code: "kn", name: "Kannada", tier: LanguageTier::Experimental },
    LanguageEntry { code: "kk", name: "Kazakh", tier: LanguageTier::Experimental },
    LanguageEntry { code: "km", name: "Khmer", tier: LanguageTier::Experimental },
    LanguageEntry { code: "lo", name: "Lao", tier: LanguageTier::Experimental },
    LanguageEntry { code: "la", name: "Latin", tier: LanguageTier::Experimental },
    LanguageEntry { code: "lv", name: "Latvian", tier: LanguageTier::Experimental },
    LanguageEntry { code: "ln", name: "Lingala", tier: LanguageTier::Experimental },
    LanguageEntry { code: "lt", name: "Lithuanian", tier: LanguageTier::Experimental },
    LanguageEntry { code: "lb", name: "Luxembourgish", tier: LanguageTier::Experimental },
    LanguageEntry { code: "mk", name: "Macedonian", tier: LanguageTier::Experimental },
    LanguageEntry { code: "mg", name: "Malagasy", tier: LanguageTier::Experimental },
    LanguageEntry { code: "ml", name: "Malayalam", tier: LanguageTier::Experimental },
    LanguageEntry { code: "mt", name: "Maltese", tier: LanguageTier::Experimental },
    LanguageEntry { code: "mi", name: "Māori", tier: LanguageTier::Experimental },
    LanguageEntry { code: "mr", name: "Marathi", tier: LanguageTier::Experimental },
    LanguageEntry { code: "mn", name: "Mongolian", tier: LanguageTier::Experimental },
    LanguageEntry { code: "ne", name: "Nepali", tier: LanguageTier::Experimental },
    LanguageEntry { code: "nn", name: "Norwegian Nynorsk", tier: LanguageTier::Experimental },
    LanguageEntry { code: "oc", name: "Occitan", tier: LanguageTier::Experimental },
    LanguageEntry { code: "ps", name: "Pashto", tier: LanguageTier::Experimental },
    LanguageEntry { code: "fa", name: "Persian", tier: LanguageTier::Experimental },
    LanguageEntry { code: "pa", name: "Punjabi", tier: LanguageTier::Experimental },
    LanguageEntry { code: "sa", name: "Sanskrit", tier: LanguageTier::Experimental },
    LanguageEntry { code: "sr", name: "Serbian", tier: LanguageTier::Experimental },
    LanguageEntry { code: "sn", name: "Shona", tier: LanguageTier::Experimental },
    LanguageEntry { code: "sd", name: "Sindhi", tier: LanguageTier::Experimental },
    LanguageEntry { code: "si", name: "Sinhala", tier: LanguageTier::Experimental },
    LanguageEntry { code: "sl", name: "Slovenian", tier: LanguageTier::Experimental },
    LanguageEntry { code: "so", name: "Somali", tier: LanguageTier::Experimental },
    LanguageEntry { code: "su", name: "Sundanese", tier: LanguageTier::Experimental },
    LanguageEntry { code: "sw", name: "Swahili", tier: LanguageTier::Experimental },
    LanguageEntry { code: "tl", name: "Tagalog", tier: LanguageTier::Experimental },
    LanguageEntry { code: "tg", name: "Tajik", tier: LanguageTier::Experimental },
    LanguageEntry { code: "ta", name: "Tamil", tier: LanguageTier::Experimental },
    LanguageEntry { code: "tt", name: "Tatar", tier: LanguageTier::Experimental },
    LanguageEntry { code: "te", name: "Telugu", tier: LanguageTier::Experimental },
    LanguageEntry { code: "bo", name: "Tibetan", tier: LanguageTier::Experimental },
    LanguageEntry { code: "tk", name: "Turkmen", tier: LanguageTier::Experimental },
    LanguageEntry { code: "ur", name: "Urdu", tier: LanguageTier::Experimental },
    LanguageEntry { code: "uz", name: "Uzbek", tier: LanguageTier::Experimental },
    LanguageEntry { code: "cy", name: "Welsh", tier: LanguageTier::Experimental },
    LanguageEntry { code: "yi", name: "Yiddish", tier: LanguageTier::Experimental },
    LanguageEntry { code: "yo", name: "Yoruba", tier: LanguageTier::Experimental },
];

pub fn supported_language_name(code: &str) -> Option<&'static str> {
    LANGUAGE_TIERS
        .iter()
        .find(|entry| entry.code == code)
        .map(|entry| entry.name)
}

pub fn is_english_only_model(model_key: &str) -> bool {
    model_key.starts_with("distil")
        || model_key.ends_with(".en")
        || model_key.contains(".en-")
}

pub fn volume_duck_level(option: &str) -> Option<f32> {
    VOLUME_DUCK_OPTIONS
        .iter()
        .find(|(k, _)| *k == option)
        .and_then(|(_, v)| *v)
}

/// Normalize a dictation delivery mode string (`"live"` -> `"live"`,
/// anything else -> `"batch"`), case-insensitively.
pub fn normalize_delivery_mode(raw: &str) -> &'static str {
    if raw.trim().eq_ignore_ascii_case("live") {
        "live"
    } else {
        "batch"
    }
}

#[derive(Debug, Error)]
pub enum SettingsError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub language: String,
    pub hotkey: String,
    pub hotkey_mode: String,
    pub hotkey_backend: String,
    pub model_size: String,
    pub model_dir: String,
    pub audio_device: Option<i32>,
    pub auto_paste: bool,
    pub overlay_enabled: bool,
    pub overlay_show_text: bool,
    pub overlay_monitor: String,
    pub overlay_opacity: f64,
    pub overlay_font_size: i32,
    pub overlay_position: String,
    pub overlay_show_level: bool,
    pub overlay_pill_scale: f64,
    pub overlay_panel_opacity: f64,
    pub volume_duck: String,
    pub start_with_windows: bool,
    pub theme: String,
    pub ui_scale: f64,
    pub history_enabled: bool,
    /// Days to retain soft-deleted History items in Trash before permanent deletion (`0` = never auto-delete).
    pub history_trash_days: i32,
    pub initial_prompt: String,
    pub replacements: BTreeMap<String, String>,
    pub voice_commands: bool,
    pub sound_feedback: bool,
    pub format_cleanup: bool,
    pub remove_fillers: bool,
    pub engine_backend: String,
    pub vad_sensitivity: f64,
    pub paste_rules: BTreeMap<String, String>,
    pub onboarding_done: bool,
    pub slow_hint_shown_for: String,
    pub delivery_mode: String,
    pub overlay_controls_enabled: bool,
    pub overlay_controls_pinned: bool,
    pub recent_languages: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: "en".to_owned(),
            hotkey: "alt+q".to_owned(),
            hotkey_mode: "toggle".to_owned(),
            hotkey_backend: "auto".to_owned(),
            model_size: "distil-large-v3".to_owned(),
            model_dir: String::new(),
            audio_device: None,
            auto_paste: true,
            overlay_enabled: true,
            overlay_show_text: true,
            overlay_monitor: "active".to_owned(),
            overlay_opacity: 0.95,
            overlay_font_size: 14,
            overlay_position: "bottom-center".to_owned(),
            overlay_show_level: true,
            overlay_pill_scale: 0.8,
            overlay_panel_opacity: 0.78,
            volume_duck: "off".to_owned(),
            start_with_windows: false,
            theme: "dark".to_owned(),
            ui_scale: 1.25,
            history_enabled: true,
            history_trash_days: 30,
            initial_prompt: String::new(),
            replacements: BTreeMap::new(),
            voice_commands: false,
            sound_feedback: true,
            format_cleanup: true,
            remove_fillers: false,
            engine_backend: "auto".to_owned(),
            vad_sensitivity: 1.0,
            paste_rules: BTreeMap::new(),
            onboarding_done: false,
            slow_hint_shown_for: String::new(),
            delivery_mode: "batch".to_owned(),
            overlay_controls_enabled: true,
            overlay_controls_pinned: false,
            recent_languages: Vec::new(),
        }
    }
}

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

impl Settings {
    /// Load settings from `path`, tolerating a UTF-8 BOM, coercing field types
    /// leniently, applying 0.8.11 upgrade defaults, and moving corrupt files to
    /// `<filename>.bad`.
    pub fn load_from_path(path: &Path) -> Self {
        if !path.exists() {
            return Self::default();
        }

        let raw = match fs::read_to_string(path) {
            Ok(s) => s,
            Err(_) => {
                backup_corrupt_file(path);
                return Self {
                    onboarding_done: true,
                    ..Self::default()
                };
            }
        };

        let trimmed = raw.strip_prefix('\u{FEFF}').unwrap_or(&raw);
        let parsed: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => {
                backup_corrupt_file(path);
                return Self {
                    onboarding_done: true,
                    ..Self::default()
                };
            }
        };

        let Some(obj) = parsed.as_object() else {
            backup_corrupt_file(path);
            return Self {
                onboarding_done: true,
                ..Self::default()
            };
        };

        Self::from_json_object(obj)
    }

    /// Coerce a JSON object into `Settings` with upgrade defaults and clamping.
    pub fn from_json_object(obj: &serde_json::Map<String, Value>) -> Self {
        let mut s = Self::default();

        if let Some(v) = obj.get("language") {
            s.language = coerce_string(v, &s.language);
        }
        if let Some(v) = obj.get("hotkey") {
            s.hotkey = coerce_string(v, &s.hotkey);
        }
        if let Some(v) = obj.get("hotkey_mode") {
            s.hotkey_mode = coerce_string(v, &s.hotkey_mode);
        }
        if let Some(v) = obj.get("hotkey_backend") {
            s.hotkey_backend = coerce_string(v, &s.hotkey_backend);
        }
        if let Some(v) = obj.get("model_size") {
            s.model_size = coerce_string(v, &s.model_size);
        }
        if let Some(v) = obj.get("model_dir") {
            s.model_dir = coerce_string(v, &s.model_dir);
        }
        if let Some(v) = obj.get("audio_device") {
            s.audio_device = coerce_opt_i32(v);
        }
        if let Some(v) = obj.get("auto_paste") {
            s.auto_paste = coerce_bool(v, s.auto_paste);
        }
        if let Some(v) = obj.get("overlay_enabled") {
            s.overlay_enabled = coerce_bool(v, s.overlay_enabled);
        }
        if let Some(v) = obj.get("overlay_show_text") {
            s.overlay_show_text = coerce_bool(v, s.overlay_show_text);
        }
        if let Some(v) = obj.get("overlay_monitor") {
            let m = coerce_string(v, &s.overlay_monitor).trim().to_ascii_lowercase();
            if OVERLAY_MONITORS.contains(&m.as_str()) {
                s.overlay_monitor = m;
            }
        }
        if let Some(v) = obj.get("overlay_opacity") {
            s.overlay_opacity = coerce_f64(v, s.overlay_opacity);
        }
        if let Some(v) = obj.get("overlay_font_size") {
            s.overlay_font_size = coerce_i32(v, s.overlay_font_size);
        }
        if let Some(v) = obj.get("overlay_position") {
            s.overlay_position = coerce_string(v, &s.overlay_position);
        }
        if let Some(v) = obj.get("overlay_show_level") {
            s.overlay_show_level = coerce_bool(v, s.overlay_show_level);
        }
        if let Some(v) = obj.get("overlay_pill_scale") {
            s.overlay_pill_scale = coerce_f64(v, s.overlay_pill_scale);
        }
        if let Some(v) = obj.get("overlay_panel_opacity") {
            s.overlay_panel_opacity = coerce_f64(v, s.overlay_panel_opacity);
        } else if obj.contains_key("overlay_opacity") {
            s.overlay_panel_opacity = s.overlay_opacity;
        }
        if let Some(v) = obj.get("volume_duck") {
            s.volume_duck = coerce_string(v, &s.volume_duck);
        } else {
            s.volume_duck = "mute".to_owned();
        }
        if let Some(v) = obj.get("start_with_windows") {
            s.start_with_windows = coerce_bool(v, s.start_with_windows);
        }
        if let Some(v) = obj.get("theme") {
            s.theme = coerce_string(v, &s.theme);
        }
        if let Some(v) = obj.get("ui_scale") {
            s.ui_scale = coerce_f64(v, s.ui_scale);
        }
        if let Some(v) = obj.get("history_enabled") {
            s.history_enabled = coerce_bool(v, s.history_enabled);
        }
        if let Some(v) = obj.get("initial_prompt") {
            s.initial_prompt = coerce_string(v, &s.initial_prompt);
        }
        if let Some(v) = obj.get("replacements") {
            s.replacements = coerce_string_map(v);
        }
        if let Some(v) = obj.get("voice_commands") {
            s.voice_commands = coerce_bool(v, s.voice_commands);
        }
        if let Some(v) = obj.get("sound_feedback") {
            s.sound_feedback = coerce_bool(v, s.sound_feedback);
        }
        if let Some(v) = obj.get("format_cleanup") {
            s.format_cleanup = coerce_bool(v, s.format_cleanup);
        }
        if let Some(v) = obj.get("remove_fillers") {
            s.remove_fillers = coerce_bool(v, s.remove_fillers);
        }
        if let Some(v) = obj.get("engine_backend") {
            s.engine_backend = coerce_string(v, &s.engine_backend);
        }
        if let Some(v) = obj.get("vad_sensitivity") {
            s.vad_sensitivity = coerce_f64(v, s.vad_sensitivity);
        }
        if let Some(v) = obj.get("paste_rules") {
            s.paste_rules = coerce_string_map(v);
        }
        if let Some(v) = obj.get("onboarding_done") {
            s.onboarding_done = coerce_bool(v, s.onboarding_done);
        } else {
            s.onboarding_done = true;
        }
        if let Some(v) = obj.get("slow_hint_shown_for") {
            s.slow_hint_shown_for = coerce_string(v, &s.slow_hint_shown_for);
        }
        if let Some(v) = obj.get("delivery_mode") {
            let raw = coerce_string(v, &s.delivery_mode);
            s.delivery_mode = normalize_delivery_mode(&raw).to_owned();
        }
        if let Some(v) = obj.get("overlay_controls_enabled") {
            s.overlay_controls_enabled = coerce_bool(v, s.overlay_controls_enabled);
        }
        if let Some(v) = obj.get("overlay_controls_pinned") {
            s.overlay_controls_pinned = coerce_bool(v, s.overlay_controls_pinned);
        }
        if let Some(v) = obj.get("recent_languages") {
            s.recent_languages = coerce_recent_languages(v);
        }
        if let Some(v) = obj.get("history_trash_days") {
            s.history_trash_days = coerce_i32(v, s.history_trash_days);
        }

        s.clamp_ranges();
        s
    }

    /// Normalize a dictation delivery mode string (`"live"` -> `"live"`,
    /// anything else -> `"batch"`), case-insensitively.
    pub fn normalize_delivery_mode(raw: &str) -> &'static str {
        normalize_delivery_mode(raw)
    }

    /// Record a Whisper language code in the MRU `recent_languages` list.
    ///
    /// Ignores invalid codes not present in [`LANGUAGE_TIERS`], moves `code`
    /// to the front, deduplicates, and truncates to [`MAX_RECENT_LANGUAGES`].
    pub fn record_recent_language(&mut self, code: &str) {
        let normalized = code.trim().to_ascii_lowercase();
        if supported_language_name(&normalized).is_none() {
            return;
        }
        self.recent_languages.retain(|c| c != &normalized);
        self.recent_languages.insert(0, normalized);
        self.recent_languages.truncate(MAX_RECENT_LANGUAGES);
    }

    pub fn clamp_ranges(&mut self) {
        self.overlay_pill_scale = self.overlay_pill_scale.clamp(0.6, 1.4);
        self.overlay_opacity = self.overlay_opacity.clamp(0.3, 1.0);
        self.overlay_panel_opacity = self.overlay_panel_opacity.clamp(0.3, 1.0);
        self.vad_sensitivity = self.vad_sensitivity.clamp(0.5, 2.0);
        self.ui_scale = self.ui_scale.clamp(1.0, 2.0);
        self.history_trash_days = self.history_trash_days.clamp(0, 365);
    }

    /// Atomically save settings to `path` via a temp file in the same directory,
    /// `sync_all()`, and up to 10 rename retries.
    pub fn save_to_path(&self, path: &Path) -> Result<(), SettingsError> {
        let dir = path.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(dir)?;

        let json = serde_json::to_string_pretty(self)?;
        let seq = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp_name = format!("settings_{}_{}.tmp", std::process::id(), seq);
        let tmp_path = dir.join(tmp_name);

        let write_res = (|| -> Result<(), std::io::Error> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&tmp_path)?;
            file.write_all(json.as_bytes())?;
            file.flush()?;
            file.sync_all()?;
            Ok(())
        })();

        if let Err(err) = write_res {
            let _ = fs::remove_file(&tmp_path);
            return Err(SettingsError::Io(err));
        }

        let mut last_err = None;
        for attempt in 0..10 {
            match fs::rename(&tmp_path, path) {
                Ok(()) => return Ok(()),
                Err(err) => {
                    last_err = Some(err);
                    if attempt < 9 {
                        let delay_ms = (20.0 * 1.5_f64.powi(attempt)) as u64;
                        thread::sleep(Duration::from_millis(delay_ms));
                    }
                }
            }
        }

        for _ in 0..3 {
            if fs::remove_file(&tmp_path).is_ok() {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }

        Err(SettingsError::Io(last_err.unwrap_or_else(|| {
            std::io::Error::other("rename failed after retries")
        })))
    }

    pub fn resolve_model_dir(&self) -> PathBuf {
        if !self.model_dir.is_empty() {
            PathBuf::from(&self.model_dir)
        } else {
            default_model_dir()
        }
    }
}

fn backup_corrupt_file(path: &Path) {
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        let bad_path = path.with_file_name(format!("{name}.bad"));
        let _ = fs::rename(path, bad_path);
    }
}

fn coerce_bool(val: &Value, default: bool) -> bool {
    match val {
        Value::Bool(b) => *b,
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                match i {
                    1 => true,
                    0 => false,
                    _ => default,
                }
            } else if let Some(f) = n.as_f64() {
                if f == 1.0 {
                    true
                } else if f == 0.0 {
                    false
                } else {
                    default
                }
            } else {
                default
            }
        }
        Value::String(s) => match s.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" | "on" => true,
            "false" | "0" | "no" | "off" => false,
            _ => default,
        },
        _ => default,
    }
}

fn coerce_i32(val: &Value, default: i32) -> i32 {
    match val {
        Value::Bool(_) => default,
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i32::try_from(i).unwrap_or(default)
            } else if let Some(f) = n.as_f64() {
                if f.is_finite() && f >= f64::from(i32::MIN) && f <= f64::from(i32::MAX) {
                    f as i32
                } else {
                    default
                }
            } else {
                default
            }
        }
        Value::String(s) => {
            let trimmed = s.trim();
            if let Ok(i) = trimmed.parse::<i64>() {
                i32::try_from(i).unwrap_or(default)
            } else if let Ok(f) = trimmed.parse::<f64>() {
                if f.is_finite() && f >= f64::from(i32::MIN) && f <= f64::from(i32::MAX) {
                    f as i32
                } else {
                    default
                }
            } else {
                default
            }
        }
        _ => default,
    }
}

fn coerce_opt_i32(val: &Value) -> Option<i32> {
    match val {
        Value::Null | Value::Bool(_) => None,
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i32::try_from(i).ok()
            } else if let Some(f) = n.as_f64() {
                if f.is_finite() && f >= f64::from(i32::MIN) && f <= f64::from(i32::MAX) {
                    Some(f as i32)
                } else {
                    None
                }
            } else {
                None
            }
        }
        Value::String(s) => s.trim().parse::<i32>().ok(),
        _ => None,
    }
}

fn coerce_f64(val: &Value, default: f64) -> f64 {
    match val {
        Value::Bool(_) => default,
        Value::Number(n) => n.as_f64().filter(|f| f.is_finite()).unwrap_or(default),
        Value::String(s) => s
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|f| f.is_finite())
            .unwrap_or(default),
        _ => default,
    }
}

fn coerce_string(val: &Value, default: &str) -> String {
    match val {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => default.to_owned(),
    }
}

fn coerce_string_map(val: &Value) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    if let Value::Object(map) = val {
        for (k, v) in map {
            if let Value::String(s) = v {
                out.insert(k.clone(), s.clone());
            } else if let Value::Number(n) = v {
                out.insert(k.clone(), n.to_string());
            }
        }
    }
    out
}

fn coerce_recent_languages(val: &Value) -> Vec<String> {
    let mut out = Vec::new();
    if let Value::Array(items) = val {
        for item in items {
            if let Value::String(raw) = item {
                let code = raw.trim().to_ascii_lowercase();
                if supported_language_name(&code).is_some() && !out.contains(&code) {
                    out.push(code);
                    if out.len() >= MAX_RECENT_LANGUAGES {
                        break;
                    }
                }
            }
        }
    }
    out
}

/// Resolve the per-user config directory (`Eqho`), respecting `EQHO_CONFIG_DIR`.
pub fn resolve_config_dir() -> PathBuf {
    if let Ok(env_dir) = std::env::var("EQHO_CONFIG_DIR")
        && !env_dir.is_empty()
    {
        return PathBuf::from(env_dir);
    }

    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = std::env::var("APPDATA")
            && !appdata.is_empty()
        {
            return PathBuf::from(appdata).join(DEFAULT_CONFIG_DIR_NAME);
        }
        if let Ok(profile) = std::env::var("USERPROFILE")
            && !profile.is_empty()
        {
            return PathBuf::from(profile)
                .join("AppData")
                .join("Roaming")
                .join(DEFAULT_CONFIG_DIR_NAME);
        }
        PathBuf::from(".").join(DEFAULT_CONFIG_DIR_NAME)
    }

    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME")
            && !home.is_empty()
        {
            return PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join(DEFAULT_CONFIG_DIR_NAME);
        }
        PathBuf::from(".").join(DEFAULT_CONFIG_DIR_NAME)
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME")
            && !xdg.is_empty()
        {
            return PathBuf::from(xdg).join(DEFAULT_CONFIG_DIR_NAME);
        }
        if let Ok(home) = std::env::var("HOME")
            && !home.is_empty()
        {
            return PathBuf::from(home)
                .join(".config")
                .join(DEFAULT_CONFIG_DIR_NAME);
        }
        PathBuf::from(".").join(DEFAULT_CONFIG_DIR_NAME)
    }
}

pub fn default_model_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Ok(local) = std::env::var("LOCALAPPDATA")
            && !local.is_empty()
        {
            return PathBuf::from(local).join("Eqho").join("models");
        }
        PathBuf::from(".").join("Eqho").join("models")
    }

    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME")
            && !home.is_empty()
        {
            return PathBuf::from(home)
                .join("Library")
                .join("Caches")
                .join("Eqho")
                .join("models");
        }
        PathBuf::from(".").join("Eqho").join("models")
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        if let Ok(xdg) = std::env::var("XDG_CACHE_HOME")
            && !xdg.is_empty()
        {
            return PathBuf::from(xdg).join("Eqho").join("models");
        }
        if let Ok(home) = std::env::var("HOME")
            && !home.is_empty()
        {
            return PathBuf::from(home).join(".cache").join("Eqho").join("models");
        }
        PathBuf::from(".").join("Eqho").join("models")
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::collections::HashSet;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("eqho_core_settings_{name}_{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn language_tiers_have_exact_counts_and_unique_codes() {
        assert_eq!(LANGUAGE_TIERS.len(), 99);
        let tuned = LANGUAGE_TIERS
            .iter()
            .filter(|e| e.tier == LanguageTier::Tuned)
            .count();
        let strong = LANGUAGE_TIERS
            .iter()
            .filter(|e| e.tier == LanguageTier::Strong)
            .count();
        let exp = LANGUAGE_TIERS
            .iter()
            .filter(|e| e.tier == LanguageTier::Experimental)
            .count();
        assert_eq!(tuned, 13);
        assert_eq!(strong, 20);
        assert_eq!(exp, 66);

        let codes: HashSet<&str> = LANGUAGE_TIERS.iter().map(|e| e.code).collect();
        assert_eq!(codes.len(), 99);
        assert_eq!(supported_language_name("en"), Some("English"));
        assert_eq!(supported_language_name("mi"), Some("Māori"));
        assert_eq!(supported_language_name("xx"), None);
    }

    #[test]
    fn english_only_model_detection() {
        assert!(is_english_only_model("distil-large-v3"));
        assert!(is_english_only_model("distil-small.en"));
        assert!(is_english_only_model("base.en"));
        assert!(!is_english_only_model("large-v3-turbo"));
        assert!(!is_english_only_model("small"));
    }

    #[test]
    fn bom_tolerance_and_upgrade_defaults() {
        let dir = temp_dir("bom");
        let path = dir.join("settings.json");
        let content = "\u{FEFF}{\"language\": \"de\", \"overlay_opacity\": 0.55, \"unknown_key\": 42}";
        fs::write(&path, content).unwrap();

        let s = Settings::load_from_path(&path);
        assert_eq!(s.language, "de");
        assert!(s.onboarding_done);
        assert_eq!(s.volume_duck, "mute");
        assert!((s.overlay_opacity - 0.55).abs() < 1e-9);
        assert!((s.overlay_panel_opacity - 0.55).abs() < 1e-9);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_settings_renamed_to_bad() {
        let dir = temp_dir("corrupt");
        let path = dir.join("settings.json");
        fs::write(&path, "{not valid json").unwrap();

        let s = Settings::load_from_path(&path);
        assert!(s.onboarding_done);
        assert!(!path.exists());
        assert!(dir.join("settings.json.bad").exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn non_object_json_treated_as_corrupt() {
        let dir = temp_dir("array");
        let path = dir.join("settings.json");
        fs::write(&path, "[1, 2, 3]").unwrap();

        let s = Settings::load_from_path(&path);
        assert!(s.onboarding_done);
        assert!(dir.join("settings.json.bad").exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn coercion_clamping_and_nan_rejection() {
        let raw: Value = serde_json::from_str(
            r#"{
                "auto_paste": "yes",
                "history_enabled": 0,
                "overlay_font_size": "18.7",
                "audio_device": "3",
                "overlay_pill_scale": 9.9,
                "overlay_opacity": 0.1,
                "overlay_panel_opacity": "NaN",
                "vad_sensitivity": "inf",
                "ui_scale": 5.0,
                "onboarding_done": false
            }"#,
        )
        .unwrap();
        let s = Settings::from_json_object(raw.as_object().unwrap());
        assert!(s.auto_paste);
        assert!(!s.history_enabled);
        assert_eq!(s.overlay_font_size, 18);
        assert_eq!(s.audio_device, Some(3));
        assert!((s.overlay_pill_scale - 1.4).abs() < 1e-9);
        assert!((s.overlay_opacity - 0.3).abs() < 1e-9);
        assert!((s.overlay_panel_opacity - 0.78).abs() < 1e-9);
        assert!((s.vad_sensitivity - 1.0).abs() < 1e-9);
        assert!((s.ui_scale - 2.0).abs() < 1e-9);
        assert!(!s.onboarding_done);
    }

    #[test]
    fn v0811_full_schema_roundtrip() {
        let dir = temp_dir("v0811_full");
        let path = dir.join("settings.json");
        let v0811_json = "\u{FEFF}{\n\
            \"audio_device\": 2,\n\
            \"auto_paste\": true,\n\
            \"engine_backend\": \"vulkan\",\n\
            \"format_cleanup\": true,\n\
            \"history_enabled\": true,\n\
            \"hotkey\": \"alt+q\",\n\
            \"hotkey_backend\": \"auto\",\n\
            \"hotkey_mode\": \"toggle\",\n\
            \"initial_prompt\": \"Eqho, Kubernetes\",\n\
            \"language\": \"en\",\n\
            \"model_dir\": \"\",\n\
            \"model_size\": \"distil-large-v3\",\n\
            \"onboarding_done\": true,\n\
            \"overlay_enabled\": true,\n\
            \"overlay_font_size\": \"16\",\n\
            \"overlay_opacity\": \"0.85\",\n\
            \"overlay_panel_opacity\": 0.8,\n\
            \"overlay_pill_scale\": 0.9,\n\
            \"overlay_position\": \"top-center\",\n\
            \"overlay_show_level\": true,\n\
            \"paste_rules\": {\"notepad.exe\": \"typing\"},\n\
            \"remove_fillers\": true,\n\
            \"replacements\": {\"c plus plus\": \"C++\"},\n\
            \"slow_hint_shown_for\": \"medium\",\n\
            \"sound_feedback\": true,\n\
            \"start_with_windows\": false,\n\
            \"theme\": \"dark\",\n\
            \"ui_scale\": 1.25,\n\
            \"vad_sensitivity\": 1.1,\n\
            \"voice_commands\": true,\n\
            \"volume_duck\": \"25%\"\n\
        }";
        fs::write(&path, v0811_json).unwrap();

        let loaded = Settings::load_from_path(&path);
        assert_eq!(loaded.audio_device, Some(2));
        assert!(loaded.auto_paste);
        assert_eq!(loaded.engine_backend, "vulkan");
        assert_eq!(loaded.hotkey, "alt+q");
        assert_eq!(loaded.hotkey_mode, "toggle");
        assert_eq!(loaded.initial_prompt, "Eqho, Kubernetes");
        assert_eq!(loaded.overlay_font_size, 16);
        assert!((loaded.overlay_opacity - 0.85).abs() < 1e-9);
        assert_eq!(loaded.overlay_position, "top-center");
        assert_eq!(loaded.paste_rules.get("notepad.exe").map(String::as_str), Some("typing"));
        assert_eq!(loaded.replacements.get("c plus plus").map(String::as_str), Some("C++"));
        assert_eq!(loaded.slow_hint_shown_for, "medium");
        assert_eq!(loaded.volume_duck, "25%");
        assert!(loaded.onboarding_done);
        assert_eq!(loaded.delivery_mode, "batch");
        assert!(loaded.overlay_controls_enabled);
        assert!(!loaded.overlay_controls_pinned);
        assert!(loaded.recent_languages.is_empty());

        loaded.save_to_path(&path).unwrap();
        assert_eq!(Settings::load_from_path(&path), loaded);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn delivery_mode_overlay_controls_and_recent_languages_defaults_and_coercion() {
        let def = Settings::default();
        assert_eq!(def.delivery_mode, "batch");
        assert!(def.overlay_controls_enabled);
        assert!(!def.overlay_controls_pinned);
        assert!(def.recent_languages.is_empty());

        assert_eq!(normalize_delivery_mode("live"), "live");
        assert_eq!(normalize_delivery_mode("LIVE"), "live");
        assert_eq!(normalize_delivery_mode(" Live "), "live");
        assert_eq!(normalize_delivery_mode("batch"), "batch");
        assert_eq!(normalize_delivery_mode("streaming"), "batch");
        assert_eq!(Settings::normalize_delivery_mode("LIVE"), "live");
        assert_eq!(Settings::normalize_delivery_mode(""), "batch");

        // Case-insensitive valid delivery_mode + coercion of overlay controls + dedup/filter/cap recent_languages.
        let raw: Value = serde_json::from_str(
            r#"{
                "delivery_mode": "LIVE",
                "overlay_controls_enabled": "off",
                "overlay_controls_pinned": 1,
                "recent_languages": ["de", "xx", "fr", "DE", 123, null, "es", "ja", "ko", "it", "pt", "nl", "pl", "tr"]
            }"#,
        )
        .unwrap();
        let s = Settings::from_json_object(raw.as_object().unwrap());
        assert_eq!(s.delivery_mode, "live");
        assert!(!s.overlay_controls_enabled);
        assert!(s.overlay_controls_pinned);
        assert_eq!(
            s.recent_languages,
            vec!["de", "fr", "es", "ja", "ko", "it", "pt", "nl"]
        );
        assert_eq!(s.recent_languages.len(), MAX_RECENT_LANGUAGES);

        // Invalid delivery_mode and non-array recent_languages fall back cleanly.
        let invalid: Value = serde_json::from_str(
            r#"{
                "delivery_mode": "instant-magic",
                "recent_languages": "not-an-array"
            }"#,
        )
        .unwrap();
        let s_inv = Settings::from_json_object(invalid.as_object().unwrap());
        assert_eq!(s_inv.delivery_mode, "batch");
        assert!(s_inv.recent_languages.is_empty());
    }

    #[test]
    fn record_recent_language_maintains_mru_order_and_cap() {
        let mut s = Settings::default();
        // Invalid codes are ignored.
        s.record_recent_language("xx");
        s.record_recent_language("");
        assert!(s.recent_languages.is_empty());

        for code in ["en", "de", "fr", "es", "ja", "ko", "it", "pt", "nl"] {
            s.record_recent_language(code);
        }
        // "en" was pushed out once the 9th unique code arrived.
        assert_eq!(s.recent_languages.len(), MAX_RECENT_LANGUAGES);
        assert_eq!(
            s.recent_languages,
            vec!["nl", "pt", "it", "ko", "ja", "es", "fr", "de"]
        );

        // Re-recording an existing code moves it to the front without duplicating.
        s.record_recent_language("fr");
        assert_eq!(s.recent_languages.len(), MAX_RECENT_LANGUAGES);
        assert_eq!(
            s.recent_languages,
            vec!["fr", "nl", "pt", "it", "ko", "ja", "es", "de"]
        );

        // Invalid code after populating does not alter list.
        s.record_recent_language("klingon");
        assert_eq!(
            s.recent_languages,
            vec!["fr", "nl", "pt", "it", "ko", "ja", "es", "de"]
        );
    }

    #[test]
    fn save_and_reload_roundtrip() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("settings.json");
        let mut s = Settings {
            language: "fr".to_owned(),
            onboarding_done: true,
            volume_duck: "25%".to_owned(),
            ..Settings::default()
        };
        s.replacements.insert("eqho".to_owned(), "Eqho".to_owned());
        s.save_to_path(&path).unwrap();

        let loaded = Settings::load_from_path(&path);
        assert_eq!(loaded, s);

        let _ = fs::remove_dir_all(&dir);
    }

    proptest! {
        #[test]
        fn arbitrary_json_object_never_panics_and_stays_clamped(
            scale in -100.0_f64..100.0,
            opacity in -100.0_f64..100.0,
            panel_opacity in -100.0_f64..100.0,
            sens in -100.0_f64..100.0,
            ui in -100.0_f64..100.0,
            junk in ".*"
        ) {
            let mut map = serde_json::Map::new();
            if let Some(n) = serde_json::Number::from_f64(scale) {
                map.insert("overlay_pill_scale".to_owned(), Value::Number(n));
            }
            if let Some(n) = serde_json::Number::from_f64(opacity) {
                map.insert("overlay_opacity".to_owned(), Value::Number(n));
            }
            if let Some(n) = serde_json::Number::from_f64(panel_opacity) {
                map.insert("overlay_panel_opacity".to_owned(), Value::Number(n));
            }
            if let Some(n) = serde_json::Number::from_f64(sens) {
                map.insert("vad_sensitivity".to_owned(), Value::Number(n));
            }
            if let Some(n) = serde_json::Number::from_f64(ui) {
                map.insert("ui_scale".to_owned(), Value::Number(n));
            }
            map.insert("auto_paste".to_owned(), Value::String(junk.clone()));
            map.insert("overlay_font_size".to_owned(), Value::String(junk.clone()));
            map.insert("audio_device".to_owned(), Value::String(junk));

            let s = Settings::from_json_object(&map);
            prop_assert!((0.6..=1.4).contains(&s.overlay_pill_scale));
            prop_assert!((0.3..=1.0).contains(&s.overlay_opacity));
            prop_assert!((0.3..=1.0).contains(&s.overlay_panel_opacity));
            prop_assert!((0.5..=2.0).contains(&s.vad_sensitivity));
            prop_assert!((1.0..=2.0).contains(&s.ui_scale));
        }
    }
}
