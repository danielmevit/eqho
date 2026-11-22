#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::cell::RefCell;
use std::rc::Rc;

use eqho_core::modelpick::HardwareClass;
use eqho_core::settings::Settings;
use eqho_ui::{DashboardWindow, Theme, apply_settings_to_window, build_model_cards, split_model_cards};
use slint::platform::software_renderer::{MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType};
use slint::platform::{Platform, WindowAdapter};
use slint::{ComponentHandle, ModelRc, PhysicalSize, VecModel};

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
    let window_adapter = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    let platform = HeadlessSoftwarePlatform {
        window: window_adapter.clone(),
    };
    slint::platform::set_platform(Box::new(platform)).unwrap();

    let win = DashboardWindow::new().unwrap();
    apply_settings_to_window(&win, &Settings::default(), HardwareClass::Gpu, true);
    let (en_cards, ml_cards) = split_model_cards(build_model_cards(
        "distil-large-v3",
        &["distil-large-v3"],
        None,
        0.0,
        None,
        HardwareClass::Gpu,
        "en",
    ));
    win.set_english_models(ModelRc::new(VecModel::from(en_cards)));
    win.set_multilingual_models(ModelRc::new(VecModel::from(ml_cards)));
    win.show().unwrap();

    let dark_wide = render_dashboard_rgba(&window_adapter, &win, 940, 560, true);
    assert_non_uniform_and_valid_png(&dark_wide, 940, 560);

    let light_wide = render_dashboard_rgba(&window_adapter, &win, 940, 560, false);
    assert_non_uniform_and_valid_png(&light_wide, 940, 560);
    assert_ne!(dark_wide, light_wide, "dark and light renders must differ");

    win.set_active_tab("models".into());
    let dark_narrow = render_dashboard_rgba(&window_adapter, &win, 620, 460, true);
    assert_non_uniform_and_valid_png(&dark_narrow, 620, 460);

    let light_narrow = render_dashboard_rgba(&window_adapter, &win, 620, 460, false);
    assert_non_uniform_and_valid_png(&light_narrow, 620, 460);

    // Also render onboarding step 0
    win.set_onboarding_active(true);
    let onboarding_frame = render_dashboard_rgba(&window_adapter, &win, 940, 560, true);
    assert_non_uniform_and_valid_png(&onboarding_frame, 940, 560);

    let _unused = RefCell::new(());
}
