#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use eqho_core::history::TranscriptEntry;
use eqho_core::modelpick::HardwareClass;
use eqho_core::settings::Settings;
use eqho_ui::{
    DashboardWindow, EqhoTray, HistoryRowItem, Theme, apply_settings_to_window,
    build_history_rows, build_model_cards, filter_history_entries, format_hotkey_display,
    format_paste_rules_text, format_replacements_text, language_warning_text,
    parse_paste_rules_text, parse_replacements_text, split_model_cards, status_summary_title,
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

    for tab in ["general", "models", "history", "settings", "general"] {
        win.set_active_tab(tab.into());
        assert_eq!(win.get_active_tab().as_str(), tab);
    }

    win.global::<Theme>().set_dark(false);
    assert!(!win.global::<Theme>().get_dark());
    win.global::<Theme>().set_dark(true);
    assert!(win.global::<Theme>().get_dark());
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

    let cards = build_model_cards(
        "distil-large-v3",
        &["distil-large-v3", "tiny"],
        Some("small"),
        0.42,
        Some("base"),
        HardwareClass::Gpu,
        "en",
    );
    assert_eq!(cards.len(), 9);
    let (en_cards, ml_cards) = split_model_cards(cards);
    assert_eq!(en_cards.len(), 3);
    assert_eq!(ml_cards.len(), 6);

    win.set_english_models(ModelRc::new(VecModel::from(en_cards)));
    win.set_multilingual_models(ModelRc::new(VecModel::from(ml_cards)));
    assert_eq!(win.get_english_models().row_count(), 3);
    assert_eq!(win.get_multilingual_models().row_count(), 6);

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
        },
        TranscriptEntry {
            ts: 1_700_000_000.0,
            text: "Second entry".into(),
            duration: 2.0,
            model: "tiny".into(),
            lang: "en".into(),
        },
    ];
    let rows: Vec<HistoryRowItem> = build_history_rows(&entries);
    win.set_history_rows(ModelRc::new(VecModel::from(rows)));
    assert_eq!(win.get_history_rows().row_count(), 2);

    let copied = Rc::new(RefCell::new(String::new()));
    let copied_c = Rc::clone(&copied);
    win.on_copy_history_entry(move |t| {
        *copied_c.borrow_mut() = t.to_string();
    });
    win.invoke_copy_history_entry("Hello world".into());
    assert_eq!(copied.borrow().as_str(), "Hello world");

    let deleted_ts = Rc::new(RefCell::new(String::new()));
    let deleted_c = Rc::clone(&deleted_ts);
    win.on_delete_history_entry(move |ts| {
        *deleted_c.borrow_mut() = ts.to_string();
    });
    win.invoke_delete_history_entry("1700000100.000000".into());
    assert_eq!(deleted_ts.borrow().as_str(), "1700000100.000000");
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

    let dismissed = Rc::new(RefCell::new(false));
    let dismissed_c = Rc::clone(&dismissed);
    win.on_dismiss_recovery(move || {
        *dismissed_c.borrow_mut() = true;
    });

    win.invoke_copy_recovery();
    win.invoke_dismiss_recovery();
    assert!(*copied.borrow());
    assert!(*dismissed.borrow());

    let tray = EqhoTray::new().unwrap();
    tray.set_has_recovery(true);
    tray.set_is_dictating(true);
    tray.set_toggle_label("Stop Dictation".into());
    assert!(tray.get_has_recovery());
    assert!(tray.get_is_dictating());
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

    assert_eq!(format_hotkey_display("alt+q"), "Alt + Q");
    assert_eq!(format_hotkey_display("ctrl+shift+space"), "Ctrl + Shift + Space");

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
        },
        TranscriptEntry {
            ts: 99.0 * 86_400.0 + 60.0, // yesterday
            text: "Yesterday dictation".into(),
            duration: 1.0,
            model: "tiny".into(),
            lang: "en".into(),
        },
        TranscriptEntry {
            ts: 95.0 * 86_400.0 + 60.0, // 5 days ago
            text: "Week dictation".into(),
            duration: 1.0,
            model: "tiny".into(),
            lang: "en".into(),
        },
        TranscriptEntry {
            ts: 60.0 * 86_400.0 + 60.0, // 40 days ago
            text: "Old dictation".into(),
            duration: 1.0,
            model: "tiny".into(),
            lang: "en".into(),
        },
    ];

    assert_eq!(filter_history_entries(&entries, "", 0, now).len(), 4);
    assert_eq!(filter_history_entries(&entries, "", 1, now).len(), 1);
    assert_eq!(filter_history_entries(&entries, "", 2, now).len(), 1);
    assert_eq!(filter_history_entries(&entries, "", 3, now).len(), 3);
    assert_eq!(filter_history_entries(&entries, "", 4, now).len(), 3);
    assert_eq!(filter_history_entries(&entries, "yesterday", 0, now).len(), 1);
}
