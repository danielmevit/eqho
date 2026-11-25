#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::PathBuf;
use std::rc::Rc;

use eqho_core::history::TranscriptEntry;
use eqho_core::modelpick::HardwareClass;
use eqho_core::settings::Settings;
use eqho_ui::{
    DashboardWindow, HoverTooltip, Theme, apply_settings_to_window, blur_snapshot,
    build_history_rows, build_model_cards, format_active_engine_display, split_model_cards,
    wordmark_image,
};
use slint::platform::software_renderer::{
    MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType,
};
use slint::platform::{Platform, WindowAdapter};
use slint::{ComponentHandle, ModelRc, PhysicalSize, Rgba8Pixel, SharedPixelBuffer, VecModel};

fn maybe_save_png(dump_dir: Option<&PathBuf>, name: &str, rgba: &[u8], width: u32, height: u32) {
    if let Some(dir) = dump_dir {
        fs::create_dir_all(dir).unwrap();
        let bytes = encode_png(rgba, width, height);
        fs::write(dir.join(format!("{name}.png")), bytes).unwrap();
    }
}

struct HeadlessSoftwarePlatform {
    window: Rc<MinimalSoftwareWindow>,
}

impl Platform for HeadlessSoftwarePlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.window.clone())
    }
}

fn render_dashboard_rgba(
    window_adapter: &Rc<MinimalSoftwareWindow>,
    win: &DashboardWindow,
    width: u32,
    height: u32,
    dark: bool,
) -> Vec<u8> {
    win.global::<Theme>().set_dark(dark);
    window_adapter.set_size(PhysicalSize::new(width, height));
    window_adapter.request_redraw();

    let mut buffer = vec![PremultipliedRgbaColor::default(); (width * height) as usize];
    window_adapter.draw_if_needed(|renderer| {
        renderer.render(&mut buffer, width as usize);
    });

    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for px in buffer {
        rgba.push(px.red);
        rgba.push(px.green);
        rgba.push(px.blue);
        rgba.push(px.alpha);
    }
    rgba
}

fn encode_png(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(rgba).unwrap();
    drop(writer);
    out
}

fn assert_non_uniform_and_valid_png(rgba: &[u8], width: u32, height: u32) {
    assert_eq!(rgba.len(), (width * height * 4) as usize);
    let first = &rgba[0..4];
    let has_variation = rgba.chunks_exact(4).any(|px| px != first);
    assert!(
        has_variation,
        "rendered frame at {width}x{height} was completely uniform"
    );

    let png_bytes = encode_png(rgba, width, height);
    assert!(png_bytes.starts_with(&[0x89, b'P', b'N', b'G']));
}

#[test]
fn software_renders_dark_and_light_frames_at_wide_and_narrow_sizes() {
    let dump_dir = std::env::var_os("EQHO_DUMP_SCREENSHOTS").map(PathBuf::from);
    let window_adapter = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    let platform = HeadlessSoftwarePlatform {
        window: window_adapter.clone(),
    };
    slint::platform::set_platform(Box::new(platform)).unwrap();

    let win = DashboardWindow::new().unwrap();
    let settings = Settings {
        onboarding_done: true,
        ..Settings::default()
    };
    apply_settings_to_window(&win, &settings, HardwareClass::Gpu, true);
    win.set_active_engine_display(
        format_active_engine_display("auto", HardwareClass::Gpu, Some("NVIDIA GeForce RTX 3060"), 12)
            .into(),
    );
    win.set_mic_names(ModelRc::new(VecModel::from(vec![
        "System Default".into(),
        "Microphone Array (Realtek Audio)".into(),
    ])));
    // Exercise all model card states simultaneously: Active (distil-large-v3), Downloading
    // (small.en-q5), Cached/Ready (large-v3-turbo, tiny), and Not downloaded (distil-medium.en).
    let (en_cards, ml_cards) = split_model_cards(build_model_cards(
        "distil-large-v3",
        &["distil-large-v3", "large-v3-turbo", "tiny"],
        Some("small.en-q5"),
        None,
        0.42,
        "Downloading 42% · 76 MB / 181 MB · 14.2 MB/s",
        None,
        HardwareClass::Gpu,
        "en",
    ));
    win.set_english_models(ModelRc::new(VecModel::from(en_cards)));
    win.set_multilingual_models(ModelRc::new(VecModel::from(ml_cards)));

    let sample_entries = vec![
        TranscriptEntry {
            ts: 1_763_816_500.0,
            text: "Dictation couldn't find an active text box so Eqho kept the full transcript right here in History for one-click copy.".into(),
            lang: "en".into(),
            model: "distil-large-v3".into(),
            duration: 5.2,
            paste_failed: true,
            deleted_at: None,
        },
        TranscriptEntry {
            ts: 1_763_816_400.0,
            text: "Hello from Eqho in Rust running offline on Vulkan in under 300 milliseconds.".into(),
            lang: "en".into(),
            model: "large-v3-turbo".into(),
            duration: 3.4,
            paste_failed: false,
            deleted_at: None,
        },
        TranscriptEntry {
            ts: 1_763_816_100.0,
            text: "Remember to double check disk space on drive E before the final release packaging.".into(),
            lang: "en".into(),
            model: "large-v3-turbo".into(),
            duration: 4.1,
            paste_failed: false,
            deleted_at: None,
        },
    ];
    win.set_history_rows(ModelRc::new(VecModel::from(build_history_rows(
        &sample_entries,
    ))));
    win.set_trash_count(2);

    let dark_logo = wordmark_image(true);
    let light_logo = wordmark_image(false);
    win.show().unwrap();

    // 1. General tab (dark & light, wide 940x560 + interactive tour card + top-right hover tooltip)
    win.set_active_tab("general".into());
    win.set_tour_step(0);
    win.set_tour_active(true);
    win.set_logo_image(dark_logo.clone());
    let dark_wide = render_dashboard_rgba(&window_adapter, &win, 940, 560, true);
    assert_non_uniform_and_valid_png(&dark_wide, 940, 560);
    maybe_save_png(dump_dir.as_ref(), "01_general_dark_940x560", &dark_wide, 940, 560);

    // Light mode with floating "Dark mode" hover tooltip pill under top-right theme icon + tour card
    win.set_logo_image(light_logo.clone());
    let tip = win.global::<HoverTooltip>();
    tip.set_text("Dark mode".into());
    tip.set_anchor_x(864.0);
    tip.set_anchor_y(20.0);
    tip.set_anchor_w(28.0);
    tip.set_anchor_h(28.0);
    tip.set_visible(true);
    let light_wide = render_dashboard_rgba(&window_adapter, &win, 940, 560, false);
    assert_non_uniform_and_valid_png(&light_wide, 940, 560);
    assert_ne!(dark_wide, light_wide, "dark and light renders must differ");
    maybe_save_png(dump_dir.as_ref(), "02_general_light_940x560", &light_wide, 940, 560);
    tip.set_visible(false);
    win.set_tour_active(false);

    // 2. Models tab (wide dark, wide light, narrow dark & narrow light)
    win.set_active_tab("models".into());
    win.set_logo_image(dark_logo.clone());
    let models_dark = render_dashboard_rgba(&window_adapter, &win, 940, 560, true);
    assert_non_uniform_and_valid_png(&models_dark, 940, 560);
    maybe_save_png(dump_dir.as_ref(), "03_models_dark_940x560", &models_dark, 940, 560);

    win.set_logo_image(light_logo.clone());
    let models_light = render_dashboard_rgba(&window_adapter, &win, 940, 560, false);
    assert_non_uniform_and_valid_png(&models_light, 940, 560);
    maybe_save_png(dump_dir.as_ref(), "03b_models_light_940x560", &models_light, 940, 560);

    win.set_logo_image(dark_logo.clone());
    let dark_narrow = render_dashboard_rgba(&window_adapter, &win, 620, 460, true);
    assert_non_uniform_and_valid_png(&dark_narrow, 620, 460);
    maybe_save_png(dump_dir.as_ref(), "04_models_dark_620x460", &dark_narrow, 620, 460);

    win.set_logo_image(light_logo.clone());
    let light_narrow = render_dashboard_rgba(&window_adapter, &win, 620, 460, false);
    assert_non_uniform_and_valid_png(&light_narrow, 620, 460);

    // 3. History tab (dark & light wide + 3-line clamped recovery banner + yellow failed-paste row + Trash view)
    win.set_active_tab("history".into());
    win.set_logo_image(dark_logo.clone());
    win.set_recovery_text(
        "This is a longer sample dictation that couldn't be pasted automatically because no text input field had focus when you stopped speaking. It spans more than three lines inside the compact notification banner so we can verify the three-line clamp, trailing ellipsis cue, scrollable hover area, and fixed-height bottom actions cleanly across wide desktop layouts without stretching any button vertically.".into(),
    );
    let history_dark = render_dashboard_rgba(&window_adapter, &win, 940, 560, true);
    assert_non_uniform_and_valid_png(&history_dark, 940, 560);
    maybe_save_png(
        dump_dir.as_ref(),
        "05_history_recovery_dark_940x560",
        &history_dark,
        940,
        560,
    );

    win.set_logo_image(light_logo.clone());
    let history_light = render_dashboard_rgba(&window_adapter, &win, 940, 560, false);
    assert_non_uniform_and_valid_png(&history_light, 940, 560);
    maybe_save_png(
        dump_dir.as_ref(),
        "05b_history_recovery_light_940x560",
        &history_light,
        940,
        560,
    );
    win.set_recovery_text("".into());

    // Trash view dark
    win.set_showing_trash(true);
    win.set_logo_image(dark_logo.clone());
    let trash_entries = vec![TranscriptEntry {
        ts: 1_763_816_000.0,
        text: "Soft-deleted transcript kept safely in Trash until auto-clear.".into(),
        lang: "en".into(),
        model: "distil-large-v3".into(),
        duration: 2.8,
        paste_failed: false,
        deleted_at: Some(1_763_816_100.0),
    }];
    win.set_history_rows(ModelRc::new(VecModel::from(
        eqho_ui::build_history_rows_with_trash_meta(&trash_entries, 30, 1_763_816_100.0 + 3.0 * 86_400.0),
    )));
    let history_trash_dark = render_dashboard_rgba(&window_adapter, &win, 940, 560, true);
    assert_non_uniform_and_valid_png(&history_trash_dark, 940, 560);
    maybe_save_png(
        dump_dir.as_ref(),
        "05c_history_trash_dark_940x560",
        &history_trash_dark,
        940,
        560,
    );
    win.set_showing_trash(false);
    win.set_history_rows(ModelRc::new(VecModel::from(build_history_rows(
        &sample_entries,
    ))));

    // 4. Settings Modal popup with real Gaussian-blurred dashboard backdrop (dark & light)
    win.set_active_tab("general".into());
    win.set_logo_image(dark_logo.clone());
    let bg_dark_rgba = render_dashboard_rgba(&window_adapter, &win, 940, 560, true);
    let bg_dark_blur = blur_snapshot(&SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
        &bg_dark_rgba,
        940,
        560,
    ));
    win.set_logo_image(light_logo.clone());
    let bg_light_rgba = render_dashboard_rgba(&window_adapter, &win, 940, 560, false);
    let bg_light_blur = blur_snapshot(&SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(
        &bg_light_rgba,
        940,
        560,
    ));

    win.set_settings_open(true);
    win.set_settings_section("appearance".into());
    win.set_settings_backdrop(bg_dark_blur.clone());
    win.set_logo_image(dark_logo.clone());
    let settings_dark = render_dashboard_rgba(&window_adapter, &win, 940, 560, true);
    assert_non_uniform_and_valid_png(&settings_dark, 940, 560);
    maybe_save_png(
        dump_dir.as_ref(),
        "06_settings_modal_appearance_dark",
        &settings_dark,
        940,
        560,
    );

    win.set_settings_section("overlay".into());
    win.set_settings_backdrop(bg_light_blur.clone());
    win.set_logo_image(light_logo.clone());
    let settings_light = render_dashboard_rgba(&window_adapter, &win, 940, 560, false);
    assert_non_uniform_and_valid_png(&settings_light, 940, 560);
    maybe_save_png(
        dump_dir.as_ref(),
        "06b_settings_modal_overlay_light",
        &settings_light,
        940,
        560,
    );

    win.set_settings_section("data".into());
    win.set_settings_backdrop(bg_dark_blur.clone());
    win.set_logo_image(dark_logo.clone());
    let settings_data_dark = render_dashboard_rgba(&window_adapter, &win, 940, 560, true);
    assert_non_uniform_and_valid_png(&settings_data_dark, 940, 560);
    maybe_save_png(
        dump_dir.as_ref(),
        "06c_settings_modal_data_dark",
        &settings_data_dark,
        940,
        560,
    );

    win.set_store_build(true);
    win.set_license_state("trial".into());
    win.set_trial_days_left(14);
    win.set_trial_days_left_text("14 days left".into());
    win.set_settings_section("license".into());
    win.set_settings_backdrop(bg_light_blur);
    win.set_logo_image(light_logo.clone());
    let settings_license_light = render_dashboard_rgba(&window_adapter, &win, 940, 560, false);
    assert_non_uniform_and_valid_png(&settings_license_light, 940, 560);
    maybe_save_png(
        dump_dir.as_ref(),
        "06d_settings_modal_license_light",
        &settings_license_light,
        940,
        560,
    );

    win.set_store_build(false);
    win.set_settings_section("about".into());
    win.set_settings_backdrop(bg_dark_blur);
    win.set_logo_image(dark_logo.clone());
    let settings_about_dark = render_dashboard_rgba(&window_adapter, &win, 940, 560, true);
    assert_non_uniform_and_valid_png(&settings_about_dark, 940, 560);
    maybe_save_png(
        dump_dir.as_ref(),
        "06e_settings_modal_about_dark",
        &settings_about_dark,
        940,
        560,
    );
    win.set_settings_backdrop(slint::Image::default());
    win.set_settings_open(false);

    // 5. Onboarding steps
    win.set_onboarding_active(true);

    win.set_onboarding_step(1);
    let onboarding_lang = render_dashboard_rgba(&window_adapter, &win, 940, 560, true);
    assert_non_uniform_and_valid_png(&onboarding_lang, 940, 560);
    maybe_save_png(
        dump_dir.as_ref(),
        "07_onboarding_step2_lang_flags",
        &onboarding_lang,
        940,
        560,
    );

    win.set_onboarding_step(2);
    win.set_onboarding_mic_level(0.55);
    win.set_onboarding_mic_status(
        "2 microphones detected. Plugged in a new one? Click Refresh devices.".into(),
    );
    let onboarding_mic = render_dashboard_rgba(&window_adapter, &win, 940, 560, true);
    assert_non_uniform_and_valid_png(&onboarding_mic, 940, 560);
    maybe_save_png(
        dump_dir.as_ref(),
        "08_onboarding_step3_mic_refresh",
        &onboarding_mic,
        940,
        560,
    );

    win.set_onboarding_step(3);
    let onboarding_model = render_dashboard_rgba(&window_adapter, &win, 940, 560, true);
    assert_non_uniform_and_valid_png(&onboarding_model, 940, 560);
    maybe_save_png(
        dump_dir.as_ref(),
        "09_onboarding_step4_models_link",
        &onboarding_model,
        940,
        560,
    );

    win.set_onboarding_step(4);
    win.set_onboarding_duck_mute(true);
    win.set_bg_download_active(true);
    win.set_bg_download_model("Distil Large v3".into());
    win.set_bg_download_progress(0.42);
    win.set_bg_download_pct(42);
    win.set_bg_download_detail(
        "Downloading 42% · 640 MB / 1.51 GB · 12.4 MB/s · ~1m 08s left".into(),
    );
    let onboarding_sound = render_dashboard_rgba(&window_adapter, &win, 940, 560, true);
    assert_non_uniform_and_valid_png(&onboarding_sound, 940, 560);
    maybe_save_png(
        dump_dir.as_ref(),
        "10_onboarding_step5_checked_sound",
        &onboarding_sound,
        940,
        560,
    );

    win.set_onboarding_step(5);
    win.set_bg_download_active(false);
    win.set_bg_download_done(true);
    win.set_bg_download_detail("Ready".into());
    win.set_onboarding_model_ready(true);
    win.set_onboarding_model_cached(true);
    win.set_onboarding_hotkey_ok(true);
    let onboarding_try = render_dashboard_rgba(&window_adapter, &win, 940, 560, true);
    assert_non_uniform_and_valid_png(&onboarding_try, 940, 560);
    maybe_save_png(
        dump_dir.as_ref(),
        "11_onboarding_step6_checklist",
        &onboarding_try,
        940,
        560,
    );

    // 6. Microsoft Store trial states
    win.set_onboarding_active(false);
    win.set_active_tab("general".into());
    win.set_logo_image(dark_logo.clone());
    win.set_store_build(true);
    win.set_license_state("trial".into());
    win.set_trial_days_left(12);
    win.set_trial_days_left_text("12 days left in your trial".into());
    let store_trial_dark = render_dashboard_rgba(&window_adapter, &win, 940, 560, true);
    assert_non_uniform_and_valid_png(&store_trial_dark, 940, 560);
    maybe_save_png(
        dump_dir.as_ref(),
        "12_store_trial_footer_dark",
        &store_trial_dark,
        940,
        560,
    );

    win.set_logo_image(light_logo);
    win.set_trial_days_left(1);
    win.set_trial_days_left_text("1 day left in your trial".into());
    let store_trial_last_light = render_dashboard_rgba(&window_adapter, &win, 940, 560, false);
    assert_non_uniform_and_valid_png(&store_trial_last_light, 940, 560);
    maybe_save_png(
        dump_dir.as_ref(),
        "13_store_trial_last_day_light",
        &store_trial_last_light,
        940,
        560,
    );

    win.set_logo_image(dark_logo);
    win.set_license_state("trial-expired".into());
    win.set_trial_days_left(0);
    let store_expired_dark = render_dashboard_rgba(&window_adapter, &win, 940, 560, true);
    assert_non_uniform_and_valid_png(&store_expired_dark, 940, 560);
    assert_ne!(
        store_trial_dark, store_expired_dark,
        "trial-expired banner render must differ from active trial footer render"
    );
    maybe_save_png(
        dump_dir.as_ref(),
        "14_store_trial_ended_banner_dark",
        &store_expired_dark,
        940,
        560,
    );
}
