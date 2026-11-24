//! Embedded PNG asset decoding and dynamic tray icon rendering.

use std::io::Cursor;

use png::{ColorType, Decoder, Transformations};

pub const LOGO_32: &[u8] = include_bytes!("../../../../../assets/logo_32.png");
pub const LOGO_32_DARK: &[u8] = include_bytes!("../../../../../assets/logo_32_dark.png");
pub const LOGO_32_WHITE: &[u8] = include_bytes!("../../../../../assets/logo_32_white.png");
pub const LOGO_HORIZONTAL_DARK: &[u8] =
    include_bytes!("../../../../../assets/logo_horizontal_dark.png");
pub const LOGO_HORIZONTAL_LIGHT: &[u8] =
    include_bytes!("../../../../../assets/logo_horizontal_light.png");

/// Decode an embedded PNG into `(width, height, rgba8_bytes)`.
pub fn decode_png_rgba(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), String> {
    let mut decoder = Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(Transformations::EXPAND | Transformations::STRIP_16);

    let mut reader = decoder
        .read_info()
        .map_err(|e| format!("PNG header decode failed: {e}"))?;
    let buf_len = reader
        .output_buffer_size()
        .ok_or_else(|| "PNG output buffer size overflow".to_owned())?;
    let mut raw = vec![0u8; buf_len];
    let info = reader
        .next_frame(&mut raw)
        .map_err(|e| format!("PNG frame decode failed: {e}"))?;
    raw.truncate(info.buffer_size());

    let rgba = match info.color_type {
        ColorType::Rgba => raw,
        ColorType::Rgb => {
            let mut out = Vec::with_capacity((raw.len() / 3) * 4);
            for px in raw.chunks_exact(3) {
                out.extend_from_slice(&[px[0], px[1], px[2], 255]);
            }
            out
        }
        ColorType::GrayscaleAlpha => {
            let mut out = Vec::with_capacity((raw.len() / 2) * 4);
            for px in raw.chunks_exact(2) {
                out.extend_from_slice(&[px[0], px[0], px[0], px[1]]);
            }
            out
        }
        ColorType::Grayscale => {
            let mut out = Vec::with_capacity(raw.len() * 4);
            for &g in &raw {
                out.extend_from_slice(&[g, g, g, 255]);
            }
            out
        }
        other => {
            return Err(format!("unsupported PNG color type after expand: {other:?}"));
        }
    };

    Ok((info.width, info.height, rgba))
}

/// Build the 32x32 RGBA8 tray icon for the current taskbar polarity and dictation state.
///
/// Selects `logo_32_dark.png` on a light taskbar or `logo_32_white.png` on a dark taskbar,
/// and draws a crisp accent status dot (`#EF4444`) in the bottom-right corner when
/// `active_dictating` is `true`.
pub fn build_tray_icon_rgba(
    light_taskbar: bool,
    active_dictating: bool,
) -> Result<(u32, u32, Vec<u8>), String> {
    let png_bytes = if light_taskbar {
        LOGO_32_DARK
    } else {
        LOGO_32_WHITE
    };
    let (w, h, mut rgba) = decode_png_rgba(png_bytes)?;
    if active_dictating && w >= 12 && h >= 12 {
        draw_status_dot(&mut rgba, w, h);
    }
    Ok((w, h, rgba))
}

/// Convert decoded RGBA8 pixel data into a `slint::Image`.
#[must_use]
pub fn rgba_to_slint_image(width: u32, height: u32, rgba: &[u8]) -> slint::Image {
    let buffer =
        slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(rgba, width, height);
    slint::Image::from_rgba8(buffer)
}

fn draw_status_dot(rgba: &mut [u8], width: u32, height: u32) {
    let cx = width as f32 - 6.5;
    let cy = height as f32 - 6.5;
    let outer_r = 5.5_f32;
    let inner_r = 4.2_f32;
    let border_color: [u8; 4] = [0x11, 0x16, 0x1D, 0xFF];
    let dot_color: [u8; 4] = [0xEF, 0x44, 0x44, 0xFF];

    for y in 0..height {
        for x in 0..width {
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            let dist = (dx * dx + dy * dy).sqrt();
            let idx = ((y * width + x) * 4) as usize;
            if idx + 3 >= rgba.len() {
                continue;
            }
            if dist <= inner_r {
                rgba[idx..idx + 4].copy_from_slice(&dot_color);
            } else if dist <= outer_r {
                rgba[idx..idx + 4].copy_from_slice(&border_color);
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn decodes_all_four_embedded_pngs() {
        for (name, bytes) in [
            ("logo_32_dark", LOGO_32_DARK),
            ("logo_32_white", LOGO_32_WHITE),
            ("logo_horizontal_dark", LOGO_HORIZONTAL_DARK),
            ("logo_horizontal_light", LOGO_HORIZONTAL_LIGHT),
        ] {
            let (w, h, rgba) = decode_png_rgba(bytes).unwrap();
            assert!(w > 0 && h > 0, "{name} should have non-zero dimensions");
            assert_eq!(rgba.len(), (w * h * 4) as usize, "{name} RGBA buffer mismatch");
        }
    }

    #[test]
    fn tray_icon_draws_distinct_dot_when_active() {
        let (w_idle, h_idle, idle) = build_tray_icon_rgba(false, false).unwrap();
        let (w_act, h_act, active) = build_tray_icon_rgba(false, true).unwrap();
        assert_eq!((w_idle, h_idle), (32, 32));
        assert_eq!((w_act, h_act), (32, 32));
        assert_ne!(idle, active);

        // Verify the bottom-right dot center pixel is the #EF4444 accent color.
        let cx = 25usize;
        let cy = 25usize;
        let idx = (cy * 32 + cx) * 4;
        assert_eq!(&active[idx..idx + 4], &[0xEF, 0x44, 0x44, 0xFF]);

        // Light taskbar uses dark logo and also differs when active.
        let (_, _, light_idle) = build_tray_icon_rgba(true, false).unwrap();
        let (_, _, light_active) = build_tray_icon_rgba(true, true).unwrap();
        assert_ne!(light_idle, light_active);
        assert_ne!(idle, light_idle);
    }
}
