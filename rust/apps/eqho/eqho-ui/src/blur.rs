//! Fast pure-Rust downscaled separable Gaussian-like blur for the Settings modal backdrop.
//!
//! Captures the live dashboard framebuffer via `Window::take_snapshot()`, downscales 3x with
//! box-area averaging, and runs 3 separable horizontal+vertical box-blur passes (~0.5 ms on a
//! 940x560 window, zero external dependencies).

use slint::{Image, Rgba8Pixel, SharedPixelBuffer};

/// Downscale and smoothly blur an RGBA snapshot buffer into a Slint `Image` suitable for
/// rendering behind the Settings modal dialog.
#[must_use]
pub fn blur_snapshot(src: &SharedPixelBuffer<Rgba8Pixel>) -> Image {
    blur_snapshot_with_radius(src, 4)
}

/// Downscale and heavily blur an RGBA snapshot buffer into a creamy frosted-glass `Image`
/// suitable for floating translucent surfaces like the interactive guide card.
#[must_use]
pub fn blur_snapshot_strong(src: &SharedPixelBuffer<Rgba8Pixel>) -> Image {
    blur_snapshot_with_radius(src, 18)
}

fn blur_snapshot_with_radius(src: &SharedPixelBuffer<Rgba8Pixel>, base_radius: usize) -> Image {
    let sw = src.width() as usize;
    let sh = src.height() as usize;
    if sw == 0 || sh == 0 {
        return Image::default();
    }

    // 3x area-averaged downscale keeps memory tiny (~230 KB) and gives clean pre-filtering.
    let scale = if sw >= 240 && sh >= 180 { 3usize } else { 1usize };
    let dw = (sw / scale).max(1);
    let dh = (sh / scale).max(1);
    let src_bytes = src.as_bytes();

    let mut buf = vec![0u8; dw * dh * 4];
    for dy in 0..dh {
        let sy0 = dy * scale;
        let sy1 = ((dy + 1) * scale).min(sh);
        for dx in 0..dw {
            let sx0 = dx * scale;
            let sx1 = ((dx + 1) * scale).min(sw);
            let mut r = 0u32;
            let mut g = 0u32;
            let mut b = 0u32;
            let mut count = 0u32;
            for sy in sy0..sy1 {
                let row = sy * sw;
                for sx in sx0..sx1 {
                    let off = (row + sx) * 4;
                    r += u32::from(src_bytes[off]);
                    g += u32::from(src_bytes[off + 1]);
                    b += u32::from(src_bytes[off + 2]);
                    count += 1;
                }
            }
            let out = (dy * dw + dx) * 4;
            let denom = count.max(1);
            buf[out] = (r / denom) as u8;
            buf[out + 1] = (g / denom) as u8;
            buf[out + 2] = (b / denom) as u8;
            buf[out + 3] = 255;
        }
    }

    // 3 separable box-blur passes closely approximate a silky Gaussian blur.
    let radius = if dw >= 16 && dh >= 16 {
        base_radius.min(dw.min(dh) / 2).max(1)
    } else {
        1usize
    };
    let mut scratch = vec![0u8; buf.len()];
    for _ in 0..3 {
        box_blur_h(&buf, &mut scratch, dw, dh, radius);
        box_blur_v(&scratch, &mut buf, dw, dh, radius);
    }

    let out_buf = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&buf, dw as u32, dh as u32);
    Image::from_rgba8(out_buf)
}

fn box_blur_h(src: &[u8], dst: &mut [u8], w: usize, h: usize, radius: usize) {
    let win = (radius * 2 + 1) as u32;
    let last_x = w.saturating_sub(1);
    for y in 0..h {
        let row = y * w;
        let mut r = 0u32;
        let mut g = 0u32;
        let mut b = 0u32;
        for k in -(radius as isize)..=(radius as isize) {
            let sx = k.clamp(0, last_x as isize) as usize;
            let off = (row + sx) * 4;
            r += u32::from(src[off]);
            g += u32::from(src[off + 1]);
            b += u32::from(src[off + 2]);
        }
        for x in 0..w {
            let out = (row + x) * 4;
            dst[out] = (r / win) as u8;
            dst[out + 1] = (g / win) as u8;
            dst[out + 2] = (b / win) as u8;
            dst[out + 3] = 255;

            let rem_x = (x as isize - radius as isize).clamp(0, last_x as isize) as usize;
            let add_x = (x + radius + 1).min(last_x);
            let roff = (row + rem_x) * 4;
            let aoff = (row + add_x) * 4;
            r = r + u32::from(src[aoff]) - u32::from(src[roff]);
            g = g + u32::from(src[aoff + 1]) - u32::from(src[roff + 1]);
            b = b + u32::from(src[aoff + 2]) - u32::from(src[roff + 2]);
        }
    }
}

fn box_blur_v(src: &[u8], dst: &mut [u8], w: usize, h: usize, radius: usize) {
    let win = (radius * 2 + 1) as u32;
    let last_y = h.saturating_sub(1);
    for x in 0..w {
        let mut r = 0u32;
        let mut g = 0u32;
        let mut b = 0u32;
        for k in -(radius as isize)..=(radius as isize) {
            let sy = k.clamp(0, last_y as isize) as usize;
            let off = (sy * w + x) * 4;
            r += u32::from(src[off]);
            g += u32::from(src[off + 1]);
            b += u32::from(src[off + 2]);
        }
        for y in 0..h {
            let out = (y * w + x) * 4;
            dst[out] = (r / win) as u8;
            dst[out + 1] = (g / win) as u8;
            dst[out + 2] = (b / win) as u8;
            dst[out + 3] = 255;

            let rem_y = (y as isize - radius as isize).clamp(0, last_y as isize) as usize;
            let add_y = (y + radius + 1).min(last_y);
            let roff = (rem_y * w + x) * 4;
            let aoff = (add_y * w + x) * 4;
            r = r + u32::from(src[aoff]) - u32::from(src[roff]);
            g = g + u32::from(src[aoff + 1]) - u32::from(src[roff + 1]);
            b = b + u32::from(src[aoff + 2]) - u32::from(src[roff + 2]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blur_snapshot_handles_empty_tiny_and_smooths_step_edge() {
        let empty = SharedPixelBuffer::<Rgba8Pixel>::new(0, 0);
        assert_eq!(blur_snapshot(&empty).size().width, 0);

        let tiny = SharedPixelBuffer::<Rgba8Pixel>::new(1, 1);
        let tiny_img = blur_snapshot(&tiny);
        assert_eq!(tiny_img.size().width, 1);
        assert_eq!(tiny_img.size().height, 1);

        // High-contrast vertical split (left black, right white) must produce smooth mid-tones.
        let (w, h) = (300u32, 180u32);
        let mut raw = vec![0u8; (w * h * 4) as usize];
        for y in 0..h as usize {
            for x in 0..w as usize {
                let off = (y * w as usize + x) * 4;
                let v = if x < (w as usize / 2) { 0u8 } else { 255u8 };
                raw[off] = v;
                raw[off + 1] = v;
                raw[off + 2] = v;
                raw[off + 3] = 255;
            }
        }
        let snap = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&raw, w, h);
        let img = blur_snapshot(&snap);
        assert_eq!(img.size().width, 100);
        assert_eq!(img.size().height, 60);
        let out = img.to_rgba8();
        assert!(out.is_some());
        if let Some(buf) = out {
            let bytes = buf.as_bytes();
            let mid = (30 * 100 + 50) * 4;
            assert!(
                (80..=180).contains(&bytes[mid]),
                "expected smooth mid-tone at step edge, got {}",
                bytes[mid]
            );
        }

        // Strong frosted-glass blur must spread further away from the step edge.
        let strong = blur_snapshot_strong(&snap);
        assert_eq!(strong.size().width, 100);
        assert_eq!(strong.size().height, 60);
        if let Some(buf) = strong.to_rgba8() {
            let bytes = buf.as_bytes();
            let offset_px = (30 * 100 + 42) * 4;
            assert!(
                bytes[offset_px] > 15,
                "expected strong blur to spread past 8 downscaled px from edge, got {}",
                bytes[offset_px]
            );
        }
    }
}
