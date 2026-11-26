#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use eqho_core::history::TranscriptEntry;
use eqho_core::modelpick::HardwareClass;
use eqho_core::settings::Settings;
use eqho_ui::{
    DashboardWindow, EqhoTray, HistoryRowItem, Theme, apply_settings_to_window,
    build_history_rows, build_history_rows_with_trash_meta, build_model_cards,
    dashboard_window_title, engine_backend_index, engine_backend_key, filter_history_entries,
    format_active_driver_display, format_active_engine_display, format_hotkey_display,
    format_paste_rules_text,
    format_replacements_text, history_trash_days_index, history_trash_days_value,
    language_warning_text, parse_paste_rules_text, parse_replacements_text, split_model_cards,
    status_summary_title, wordmark_image,
};
use slint::{ComponentHandle, Model, ModelRc, VecModel};

fn init_testing() {
    i_slint_backend_testing::init_no_event_loop();
}

#[test]
fn tabs_and_theme_switching_work_without_panic() {
    init_testing();
    let win = DashboardWindow::new().unwrap();
    apply_settings_to_window(&win, &Settings::default(), HardwareClass::Gpu, true);

    for tab in ["general", "models", "history", "general"] {
        win.set_active_tab(tab.into());
        assert_eq!(win.get_active_tab().as_str(), tab);
    }

    // Settings modal & interactive tour properties + callbacks
    assert!(!win.get_settings_open());
    win.set_settings_open(true);
    win.set_settings_section("overlay".into());
    assert!(win.get_settings_open());
    assert_eq!(win.get_settings_section().as_str(), "overlay");
    win.set_settings_open(false);

    win.set_tour_active(true);
    win.set_tour_step(2);
    assert!(win.get_tour_active());
    assert_eq!(win.get_tour_step(), 2);
    win.set_tour_active(false);

    win.global::<Theme>().set_dark(false);
    assert!(!win.global::<Theme>().get_dark());
    win.global::<Theme>().set_dark(true);
    assert!(win.global::<Theme>().get_dark());

    // Pure-vector 4x HiDPI wordmarks render cleanly for both themes
    let dark_wm = wordmark_image(true);
    let light_wm = wordmark_image(false);
    assert_eq!(dark_wm.size().width, 288);
    assert_eq!(dark_wm.size().height, 124);
    assert_eq!(light_wm.size().width, 288);
    assert_eq!(light_wm.size().height, 124);
}

#[test]
fn onboarding_steps_and_callbacks_work() {
    init_testing();
    let win = DashboardWindow::new().unwrap();
    win.set_onboarding_active(true);
    win.set_onboarding_step(0);

    let skipped = Rc::new(RefCell::new(false));
    let skipped_c = Rc::clone(&skipped);
    win.on_onboarding_skip(move || {
        *skipped_c.borrow_mut() = true;
    });

    let finished = Rc::new(RefCell::new(false));
    let finished_c = Rc::clone(&finished);
    win.on_onboarding_finish(move || {
        *finished_c.borrow_mut() = true;
    });

    for step in 0..=5 {
        win.set_onboarding_step(step);
        assert_eq!(win.get_onboarding_step(), step);
    }

    win.invoke_onboarding_finish();
    assert!(*finished.borrow());

    win.invoke_onboarding_skip();
    assert!(*skipped.borrow());
}

#[test]
fn model_cards_and_history_rows_populate_and_fire_callbacks() {
    init_testing();
    let win = DashboardWindow::new().unwrap();

    // Regression test: if the selected model is currently downloading or not yet cached,
    // it must NEVER be flagged `is_selected` simultaneously.
    let dl_selected = build_model_cards(
        "distil-large-v3",
        &["tiny"],
        Some("distil-large-v3"),
        None,
        0.25,
        "Downloading 25%",
        None,
        HardwareClass::Gpu,
        "en",
    );
    let dl_v3 = dl_selected
        .iter()
        .find(|c| c.key == "distil-large-v3")
        .unwrap();
    assert!(dl_v3.is_downloading);
    assert!(!dl_v3.is_selected);

    // Regression test: an already-downloaded model that is loading into GPU/CPU memory
    // must NEVER say "Downloading…"; it stays selected with `is_loading = true` and `is_downloading = false`.
    let loading_cached = build_model_cards(
        "distil-large-v3",
        &["distil-large-v3", "tiny"],
        None,
        Some("distil-large-v3"),
        1.0,
        "loading whisper.cpp model (gpu)",
        None,
        HardwareClass::Gpu,
        "en",
    );
    let load_v3 = loading_cached
        .iter()
        .find(|c| c.key == "distil-large-v3")
        .unwrap();
    assert!(load_v3.is_selected);
    assert!(load_v3.is_cached);
    assert!(load_v3.is_loading);
    assert!(!load_v3.is_downloading);
    assert_eq!(load_v3.status_text.as_str(), "Loading…");

    let cards = build_model_cards(
        "distil-large-v3",
        &["distil-large-v3", "tiny"],
        Some("small"),
        None,
        0.42,
        "Downloading 42% · 12.4 MB/s",
        Some("base"),
        HardwareClass::Gpu,
        "en",
    );
    assert_eq!(cards.len(), 13);
    let active_card = cards.iter().find(|c| c.key == "distil-large-v3").unwrap();
    assert!(active_card.is_selected);
    assert!(active_card.is_cached);
    let dl_card = cards.iter().find(|c| c.key == "small").unwrap();
    assert_eq!(dl_card.download_detail.as_str(), "Downloading 42% · 12.4 MB/s");
    let (en_cards, ml_cards) = split_model_cards(cards);
    assert_eq!(en_cards.len(), 4);
    assert_eq!(ml_cards.len(), 9);

    win.set_english_models(ModelRc::new(VecModel::from(en_cards)));
    win.set_multilingual_models(ModelRc::new(VecModel::from(ml_cards)));
    assert_eq!(win.get_english_models().row_count(), 4);
    assert_eq!(win.get_multilingual_models().row_count(), 9);

    let selected_model = Rc::new(RefCell::new(String::new()));
    let selected_c = Rc::clone(&selected_model);
    win.on_select_model(move |k| {
        *selected_c.borrow_mut() = k.to_string();
    });
    win.invoke_select_model("tiny".into());
    assert_eq!(selected_model.borrow().as_str(), "tiny");

    let entries = vec![
        TranscriptEntry {
            ts: 1_700_000_100.0,
            text: "Hello world".into(),
            duration: 1.5,
            model: "distil-large-v3".into(),
            lang: "en".into(),
            paste_failed: true,
            deleted_at: None,
        },
        TranscriptEntry {
            ts: 1_700_000_000.0,
            text: "Second entry".into(),
            duration: 2.0,
            model: "tiny".into(),
            lang: "en".into(),
            paste_failed: false,
            deleted_at: None,
        },
    ];
    let rows: Vec<HistoryRowItem> = build_history_rows(&entries);
    assert!(rows[0].paste_failed);
    assert!(!rows[1].paste_failed);
    win.set_history_rows(ModelRc::new(VecModel::from(rows)));
    assert_eq!(win.get_history_rows().row_count(), 2);

    let copied = Rc::new(RefCell::new((String::new(), String::new())));
    let copied_c = Rc::clone(&copied);
    win.on_copy_history_entry(move |ts, t| {
        *copied_c.borrow_mut() = (ts.to_string(), t.to_string());
    });
    win.invoke_copy_history_entry("1700000100.000000".into(), "Hello world".into());
    assert_eq!(
        *copied.borrow(),
        ("1700000100.000000".to_owned(), "Hello world".to_owned())
    );

    let deleted_ts = Rc::new(RefCell::new(String::new()));
    let deleted_c = Rc::clone(&deleted_ts);
    win.on_delete_history_entry(move |ts| {
        *deleted_c.borrow_mut() = ts.to_string();
    });
    win.invoke_delete_history_entry("1700000100.000000".into());
    assert_eq!(deleted_ts.borrow().as_str(), "1700000100.000000");

    // Trash callbacks & expiry labels
    let trashed = vec![TranscriptEntry {
        ts: 1_700_000_000.0,
        text: "Trashed entry".into(),
        duration: 1.0,
        model: "tiny".into(),
        lang: "en".into(),
        paste_failed: false,
        deleted_at: Some(1_700_000_000.0),
    }];
    let trash_rows = build_history_rows_with_trash_meta(&trashed, 30, 1_700_000_000.0 + 5.0 * 86_400.0);
    assert_eq!(trash_rows[0].trash_expiry_text.as_str(), "Auto-deletes in 25 days");

    let trash_toggled = Rc::new(RefCell::new(false));
    let tt_c = Rc::clone(&trash_toggled);
    win.on_toggle_trash_view(move |v| {
        *tt_c.borrow_mut() = v;
    });
    win.invoke_toggle_trash_view(true);
    assert!(*trash_toggled.borrow());

    let cancelled_model = Rc::new(RefCell::new(String::new()));
    let cm_c = Rc::clone(&cancelled_model);
    win.on_cancel_download_model(move |k| {
        *cm_c.borrow_mut() = k.to_string();
    });
    win.invoke_cancel_download_model("small.en-q5".into());
    assert_eq!(cancelled_model.borrow().as_str(), "small.en-q5");

    let opened_trash = Rc::new(RefCell::new(false));
    let ot_c = Rc::clone(&opened_trash);
    win.on_open_trash(move || {
        *ot_c.borrow_mut() = true;
    });
    win.invoke_open_trash();
    assert!(*opened_trash.borrow());
}

#[test]
fn recovery_banner_and_tray_callbacks_fire() {
    init_testing();
    let win = DashboardWindow::new().unwrap();
    win.set_recovery_text("Dictated text that failed to paste".into());
    assert_eq!(
        win.get_recovery_text().as_str(),
        "Dictated text that failed to paste"
    );

    let copied = Rc::new(RefCell::new(false));
    let copied_c = Rc::clone(&copied);
    win.on_copy_recovery(move || {
        *copied_c.borrow_mut() = true;
    });

    let viewed_history = Rc::new(RefCell::new(false));
    let vh_c = Rc::clone(&viewed_history);
    win.on_view_recovery_in_history(move || {
        *vh_c.borrow_mut() = true;
    });

    let dismissed = Rc::new(RefCell::new(false));
    let dismissed_c = Rc::clone(&dismissed);
    win.on_dismiss_recovery(move || {
        *dismissed_c.borrow_mut() = true;
    });

    win.invoke_copy_recovery();
    win.invoke_view_recovery_in_history();
    win.invoke_dismiss_recovery();
    assert!(*copied.borrow());
    assert!(*viewed_history.borrow());
    assert!(*dismissed.borrow());

    let tray = EqhoTray::new().unwrap();
    tray.set_has_recovery(true);
    tray.set_is_dictating(true);
    tray.set_dictation_allowed(false);
    tray.set_toggle_label("Stop Dictation".into());
    assert!(tray.get_has_recovery());
    assert!(tray.get_is_dictating());
    assert!(!tray.get_dictation_allowed());

    // Store UI properties default off (direct-download build has no Buy UI).
    assert!(!win.get_store_build());
    let bought = Rc::new(RefCell::new(false));
    let bought_c = Rc::clone(&bought);
    win.on_buy_clicked(move || {
        *bought_c.borrow_mut() = true;
    });
    win.set_store_build(true);
    win.set_license_state("trial-expired".into());
    win.invoke_buy_clicked();
    assert!(*bought.borrow());
}

#[test]
fn viewmodel_parsers_formatters_and_history_filter() {
    let reps = parse_replacements_text("C++ => C Plus Plus\n\nbadline\n eqho => Eqho ");
    assert_eq!(reps.get("C++").map(String::as_str), Some("C Plus Plus"));
    assert_eq!(reps.get("eqho").map(String::as_str), Some("Eqho"));
    assert_eq!(reps.len(), 2);
    assert_eq!(
        format_replacements_text(&reps),
        "C++ => C Plus Plus\neqho => Eqho"
    );

    let rules = parse_paste_rules_text("Notepad.EXE = TYPING\ncode.exe = clipboard\nbad = nope");
    let mut expected = BTreeMap::new();
    expected.insert("code.exe".to_owned(), "clipboard".to_owned());
    expected.insert("notepad.exe".to_owned(), "typing".to_owned());
    assert_eq!(rules, expected);
    assert_eq!(
        format_paste_rules_text(&rules),
        "code.exe = clipboard\nnotepad.exe = typing"
    );

    assert_eq!(engine_backend_index("auto"), 0);
    assert_eq!(engine_backend_index("vulkan"), 1);
    assert_eq!(engine_backend_index("gpu"), 1);
    assert_eq!(engine_backend_index("cpu"), 2);
    assert_eq!(engine_backend_key(0), "auto");
    assert_eq!(engine_backend_key(1), "vulkan");
    assert_eq!(engine_backend_key(2), "cpu");

    assert_eq!(
        format_active_engine_display("auto", HardwareClass::Gpu, Some("NVIDIA RTX 4070"), 12),
        "GPU (Vulkan — NVIDIA RTX 4070) · whisper.cpp 1.8.4"
    );
    assert_eq!(
        format_active_engine_display("cpu", HardwareClass::Gpu, Some("NVIDIA RTX 4070"), 8),
        "CPU (8 threads) · whisper.cpp 1.8.4"
    );
    assert_eq!(
        format_active_engine_display("auto", HardwareClass::Cpu, None, 6),
        "CPU (6 threads) · whisper.cpp 1.8.4"
    );
    assert_eq!(
        format_active_driver_display("auto", HardwareClass::Gpu, Some("NVIDIA RTX 4070"), 12),
        "Vulkan · NVIDIA RTX 4070"
    );
    assert_eq!(
        format_active_driver_display("cpu", HardwareClass::Gpu, Some("NVIDIA RTX 4070"), 8),
        "CPU · 8 threads"
    );

    assert_eq!(history_trash_days_index(7), 0);
    assert_eq!(history_trash_days_index(30), 2);
    assert_eq!(history_trash_days_index(0), 4);
    assert_eq!(history_trash_days_value(0), 7);
    assert_eq!(history_trash_days_value(2), 30);
    assert_eq!(history_trash_days_value(4), 0);

    assert_eq!(
        dashboard_window_title("distil-large-v3", false),
        "Eqho — Distil Large v3"
    );
    assert_eq!(
        dashboard_window_title("distil-large-v3", true),
        "Eqho — Distil Large v3 · Listening…"
    );

    assert_eq!(format_hotkey_display("alt+q"), "Alt + Q");
    assert_eq!(
        format_hotkey_display("ctrl+shift+space"),
        "Ctrl + Shift + Space"
    );

    let warn = language_warning_text("de", "distil-large-v3", HardwareClass::Gpu);
    assert!(warn.contains("German"));
    assert!(warn.contains("Large v3 Turbo"));
    assert!(language_warning_text("en", "distil-large-v3", HardwareClass::Gpu).is_empty());
    assert!(language_warning_text("de", "large-v3-turbo", HardwareClass::Gpu).is_empty());

    assert_eq!(
        status_summary_title(false, "alt+q"),
        "Ready — press Alt + Q to dictate"
    );
    assert_eq!(
        status_summary_title(true, "alt+q"),
        "Listening… (press Alt + Q to stop)"
    );

    // History period filtering (UTC day boundaries)
    let now = 100.0 * 86_400.0 + 3600.0; // day 100, 01:00 UTC
    let entries = vec![
        TranscriptEntry {
            ts: 100.0 * 86_400.0 + 60.0, // today
            text: "Today dictation".into(),
            duration: 1.0,
            model: "tiny".into(),
            lang: "en".into(),
            paste_failed: false,
            deleted_at: None,
        },
        TranscriptEntry {
            ts: 99.0 * 86_400.0 + 60.0, // yesterday
            text: "Yesterday dictation".into(),
            duration: 1.0,
            model: "tiny".into(),
            lang: "en".into(),
            paste_failed: false,
            deleted_at: None,
        },
        TranscriptEntry {
            ts: 95.0 * 86_400.0 + 60.0, // 5 days ago
            text: "Week dictation".into(),
            duration: 1.0,
            model: "tiny".into(),
            lang: "en".into(),
            paste_failed: false,
            deleted_at: None,
        },
        TranscriptEntry {
            ts: 60.0 * 86_400.0 + 60.0, // 40 days ago
            text: "Old dictation".into(),
            duration: 1.0,
            model: "tiny".into(),
            lang: "en".into(),
            paste_failed: false,
            deleted_at: None,
        },
    ];

    assert_eq!(filter_history_entries(&entries, "", 0, now).len(), 4);
    assert_eq!(filter_history_entries(&entries, "", 1, now).len(), 1);
    assert_eq!(filter_history_entries(&entries, "", 2, now).len(), 1);
    assert_eq!(filter_history_entries(&entries, "", 3, now).len(), 3);
    assert_eq!(filter_history_entries(&entries, "", 4, now).len(), 3);
    assert_eq!(
        filter_history_entries(&entries, "yesterday", 0, now).len(),
        1
    );
}
