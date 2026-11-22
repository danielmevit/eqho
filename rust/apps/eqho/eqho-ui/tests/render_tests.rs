#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::PathBuf;
use std::rc::Rc;

use eqho_core::history::TranscriptEntry;
use eqho_core::modelpick::HardwareClass;
use eqho_core::settings::Settings;
use eqho_ui::{
    DashboardWindow, Theme, apply_settings_to_window, build_history_rows, build_model_cards,
    split_model_cards,
};
use slint::platform::software_renderer::{MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType};
use slint::platform::{Platform, WindowAdapter};
use slint::{ComponentHandle, Image, ModelRc, PhysicalSize, Rgba8Pixel, SharedPixelBuffer, VecModel};

const LOGO_DARK_PNG: &[u8] = include_bytes!("../../../../../assets/logo_horizontal_dark.png");
const LOGO_LIGHT_PNG: &[u8] = include_bytes!("../../../../../assets/logo_horizontal_light.png");

fn decode_logo(bytes: &[u8]) -> Image {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0u8; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    buf.truncate(info.buffer_size());
    let pb = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&buf, info.width, info.height);
    Image::from_rgba8(pb)
}

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
    assert!(has_variation, "rendered frame at {width}x{height} was completely uniform");

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
    win.set_mic_names(ModelRc::new(VecModel::from(vec![
        "System Default".into(),
        "Microphone Array (Realtek Audio)".into(),
    ])));
    let (en_cards, ml_cards) = split_model_cards(build_model_cards(
        "distil-large-v3",
        &["distil-large-v3", "large-v3-turbo", "tiny"],
        None,
        0.0,
        None,
        HardwareClass::Gpu,
        "en",
    ));
    win.set_english_models(ModelRc::new(VecModel::from(en_cards)));
    win.set_multilingual_models(ModelRc::new(VecModel::from(ml_cards)));

    let sample_entries = vec![
        TranscriptEntry {
            ts: 1_763_816_400.0,
            text: "Hello from Eqho in Rust running offline on Vulkan in under 300 milliseconds.".into(),
            lang: "en".into(),
            model: "large-v3-turbo".into(),
            duration: 3.4,
        },
        TranscriptEntry {
            ts: 1_763_816_100.0,
            text: "Remember to double check disk space on drive E before the final release packaging.".into(),
            lang: "en".into(),
            model: "large-v3-turbo".into(),
            duration: 4.1,
        },
    ];
    win.set_history_rows(ModelRc::new(VecModel::from(build_history_rows(
        &sample_entries,
    ))));

    let dark_logo = decode_logo(LOGO_DARK_PNG);
    let light_logo = decode_logo(LOGO_LIGHT_PNG);
    win.show().unwrap();

    // 1. General tab (dark & light, wide 940x560)
    win.set_active_tab("general".into());
    win.set_logo_image(dark_logo.clone());
    let dark_wide = render_dashboard_rgba(&window_adapter, &win, 940, 560, true);
    assert_non_uniform_and_valid_png(&dark_wide, 940, 560);
    maybe_save_png(dump_dir.as_ref(), "01_general_dark_940x560", &dark_wide, 940, 560);

    win.set_logo_image(light_logo.clone());
    let light_wide = render_dashboard_rgba(&window_adapter, &win, 940, 560, false);
    assert_non_uniform_and_valid_png(&light_wide, 940, 560);
    assert_ne!(dark_wide, light_wide, "dark and light renders must differ");
    maybe_save_png(dump_dir.as_ref(), "02_general_light_940x560", &light_wide, 940, 560);

    // 2. Models tab (wide dark & narrow light)
    win.set_active_tab("models".into());
    win.set_logo_image(dark_logo.clone());
    let models_dark = render_dashboard_rgba(&window_adapter, &win, 940, 560, true);
    assert_non_uniform_and_valid_png(&models_dark, 940, 560);
    maybe_save_png(dump_dir.as_ref(), "03_models_dark_940x560", &models_dark, 940, 560);

    let dark_narrow = render_dashboard_rgba(&window_adapter, &win, 620, 460, true);
    assert_non_uniform_and_valid_png(&dark_narrow, 620, 460);
    maybe_save_png(dump_dir.as_ref(), "04_models_dark_620x460", &dark_narrow, 620, 460);

    win.set_logo_image(light_logo);
    let light_narrow = render_dashboard_rgba(&window_adapter, &win, 620, 460, false);
    assert_non_uniform_and_valid_png(&light_narrow, 620, 460);

    // 3. History tab (dark wide + recovery banner)
    win.set_active_tab("history".into());
    win.set_logo_image(dark_logo.clone());
    win.set_recovery_text("Last dictation saved here if paste target wasn't focused.".into());
    let history_dark = render_dashboard_rgba(&window_adapter, &win, 940, 560, true);
    assert_non_uniform_and_valid_png(&history_dark, 940, 560);
    maybe_save_png(dump_dir.as_ref(), "05_history_recovery_dark_940x560", &history_dark, 940, 560);
    win.set_recovery_text("".into());

    // 4. Settings tab (dark wide)
    win.set_active_tab("settings".into());
    let settings_dark = render_dashboard_rgba(&window_adapter, &win, 940, 560, true);
    assert_non_uniform_and_valid_png(&settings_dark, 940, 560);
    maybe_save_png(dump_dir.as_ref(), "06_settings_dark_940x560", &settings_dark, 940, 560);

    // 5. Onboarding steps: Step 2 (Language w/ flags), Step 3 (Mic + Refresh + 3/6 blue bar),
    //    Step 4 (Trusted source URL + 4/6 blue bar), Step 5 (Other sounds + live 42% ETA bar + 5/6 blue bar)
    win.set_onboarding_active(true);

    win.set_onboarding_step(1);
    let onboarding_lang = render_dashboard_rgba(&window_adapter, &win, 940, 560, true);
    assert_non_uniform_and_valid_png(&onboarding_lang, 940, 560);
    maybe_save_png(dump_dir.as_ref(), "07_onboarding_step2_lang_flags", &onboarding_lang, 940, 560);

    win.set_onboarding_step(2);
    win.set_onboarding_mic_level(0.55);
    win.set_onboarding_mic_status("2 microphones detected. Plugged in a new one? Click Refresh devices.".into());
    let onboarding_mic = render_dashboard_rgba(&window_adapter, &win, 940, 560, true);
    assert_non_uniform_and_valid_png(&onboarding_mic, 940, 560);
    maybe_save_png(dump_dir.as_ref(), "08_onboarding_step3_mic_refresh", &onboarding_mic, 940, 560);

    win.set_onboarding_step(3);
    let onboarding_model = render_dashboard_rgba(&window_adapter, &win, 940, 560, true);
    assert_non_uniform_and_valid_png(&onboarding_model, 940, 560);
    maybe_save_png(dump_dir.as_ref(), "09_onboarding_step4_trusted_link", &onboarding_model, 940, 560);

    win.set_onboarding_step(4);
    win.set_bg_download_active(true);
    win.set_bg_download_model("Distil Large v3".into());
    win.set_bg_download_progress(0.42);
    win.set_bg_download_pct(42);
    win.set_bg_download_detail("Downloading 42% · 640 MB / 1.51 GB · 12.4 MB/s · ~1m 08s left".into());
    let onboarding_sound = render_dashboard_rgba(&window_adapter, &win, 940, 560, true);
    assert_non_uniform_and_valid_png(&onboarding_sound, 940, 560);
    maybe_save_png(dump_dir.as_ref(), "10_onboarding_step5_bg_download", &onboarding_sound, 940, 560);
}
