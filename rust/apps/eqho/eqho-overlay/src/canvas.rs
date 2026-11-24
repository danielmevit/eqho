//! Pure-Rust premultiplied-BGRA8 antialiased 2D software canvas for rendering Eqho's under-pill
//! micro-bar and searchable 99-language popup directly into Win32 `UpdateLayeredWindow` DIBs.

use crate::underbar::{LocalRect, Rgba};

/// Premultiplied BGRA8 pixel canvas (`[b, g, r, a]` per pixel, row-major top-down).
#[derive(Debug, Clone)]
pub(crate) struct PremulCanvas {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl PremulCanvas {
    /// Allocate a transparent canvas of `width × height` pixels.
    #[must_use]
    pub(crate) fn new(width: u32, height: u32) -> Self {
        let len = (width as usize) * (height as usize) * 4;
        Self {
            width,
            height,
            pixels: vec![0u8; len],
        }
    }

    /// Ensure dimensions match `width × height` and clear to transparent.
    pub(crate) fn reset(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        let len = (width as usize) * (height as usize) * 4;
        self.pixels.resize(len, 0);
        self.pixels.fill(0);
    }

    /// Blend a straight-alpha `color` modulated by coverage `cov` (`0.0..=1.0`) onto pixel `(x, y)`
    /// using Porter-Duff `source-over` in premultiplied BGRA8.
    #[inline]
    pub(crate) fn blend_pixel(&mut self, x: i32, y: i32, color: Rgba, cov: f32) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let a = (f32::from(color.a) / 255.0) * cov.clamp(0.0, 1.0);
        if a <= 0.0005 {
            return;
        }
        let idx = ((y as usize) * (self.width as usize) + (x as usize)) * 4;
        let inv = 1.0 - a;

        let sr = (f32::from(color.r) * a).clamp(0.0, 255.0);
        let sg = (f32::from(color.g) * a).clamp(0.0, 255.0);
        let sb = (f32::from(color.b) * a).clamp(0.0, 255.0);
        let sa = (255.0 * a).clamp(0.0, 255.0);

        let db = f32::from(self.pixels[idx]);
        let dg = f32::from(self.pixels[idx + 1]);
        let dr = f32::from(self.pixels[idx + 2]);
        let da = f32::from(self.pixels[idx + 3]);

        self.pixels[idx] = (sb + db * inv).round().clamp(0.0, 255.0) as u8;
        self.pixels[idx + 1] = (sg + dg * inv).round().clamp(0.0, 255.0) as u8;
        self.pixels[idx + 2] = (sr + dr * inv).round().clamp(0.0, 255.0) as u8;
        self.pixels[idx + 3] = (sa + da * inv).round().clamp(0.0, 255.0) as u8;
    }

    /// Blend a premultiplied-BGRA pixel `[b, g, r, a]` scaled by `alpha_mul` onto `(x, y)`.
    #[inline]
    pub(crate) fn blend_premul_pixel(&mut self, x: i32, y: i32, bgra: [u8; 4], alpha_mul: f32) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let m = alpha_mul.clamp(0.0, 1.0);
        let sa_m = f32::from(bgra[3]) * m;
        if sa_m <= 0.05 {
            return;
        }
        let inv = 1.0 - (sa_m / 255.0);
        let idx = ((y as usize) * (self.width as usize) + (x as usize)) * 4;
        let db = f32::from(self.pixels[idx]);
        let dg = f32::from(self.pixels[idx + 1]);
        let dr = f32::from(self.pixels[idx + 2]);
        let da = f32::from(self.pixels[idx + 3]);

        self.pixels[idx] = (f32::from(bgra[0]) * m + db * inv).round().clamp(0.0, 255.0) as u8;
        self.pixels[idx + 1] = (f32::from(bgra[1]) * m + dg * inv).round().clamp(0.0, 255.0) as u8;
        self.pixels[idx + 2] = (f32::from(bgra[2]) * m + dr * inv).round().clamp(0.0, 255.0) as u8;
        self.pixels[idx + 3] = (sa_m + da * inv).round().clamp(0.0, 255.0) as u8;
    }

    /// Fill an antialiased rounded rectangle with an optional crisp inner `1px` border.
    pub(crate) fn fill_rounded_rect(
        &mut self,
        rect: LocalRect,
        radius: f32,
        fill: Rgba,
        border: Option<Rgba>,
    ) {
        if rect.width == 0 || rect.height == 0 {
            return;
        }
        let hw = (rect.width as f32) * 0.5;
        let hh = (rect.height as f32) * 0.5;
        let cx = (rect.x as f32) + hw;
        let cy = (rect.y as f32) + hh;
        let r = radius.min(hw).min(hh).max(0.0);

        let x0 = (rect.x - 1).max(0);
        let y0 = (rect.y - 1).max(0);
        let x1 = (rect.right() + 1).min(self.width as i32);
        let y1 = (rect.bottom() + 1).min(self.height as i32);

        for py in y0..y1 {
            let fy = (py as f32) + 0.5 - cy;
            for px in x0..x1 {
                let fx = (px as f32) + 0.5 - cx;
                let dist = sdf_rounded_box(fx, fy, hw, hh, r);
                let outer_cov = (0.5 - dist).clamp(0.0, 1.0);
                if outer_cov <= 0.0 {
                    continue;
                }
                if let Some(border_color) = border {
                    // Inner fill starts 1px inside the border; outer 1px band receives border color.
                    let inner_cov = (-0.5 - dist).clamp(0.0, 1.0);
                    let border_cov = (outer_cov - inner_cov).clamp(0.0, 1.0);
                    if inner_cov > 0.0 {
                        self.blend_pixel(px, py, fill, inner_cov);
                    }
                    if border_cov > 0.0 {
                        self.blend_pixel(px, py, border_color, border_cov);
                    }
                } else {
                    self.blend_pixel(px, py, fill, outer_cov);
                }
            }
        }
    }

    /// Draw a crisp 1px axis-aligned rectangle (e.g. hairline divider).
    pub(crate) fn fill_rect(&mut self, rect: LocalRect, color: Rgba) {
        let x0 = rect.x.max(0);
        let y0 = rect.y.max(0);
        let x1 = rect.right().min(self.width as i32);
        let y1 = rect.bottom().min(self.height as i32);
        for py in y0..y1 {
            for px in x0..x1 {
                self.blend_pixel(px, py, color, 1.0);
            }
        }
    }

    /// Draw an antialiased rounded-cap line segment between `(x0, y0)` and `(x1, y1)`.
    pub(crate) fn stroke_segment(
        &mut self,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        thickness: f32,
        color: Rgba,
    ) {
        let half_w = thickness * 0.5;
        let pad = half_w + 1.5;
        let min_x = (x0.min(x1) - pad).floor().max(0.0) as i32;
        let max_x = (x0.max(x1) + pad).ceil().min(self.width as f32) as i32;
        let min_y = (y0.min(y1) - pad).floor().max(0.0) as i32;
        let max_y = (y0.max(y1) + pad).ceil().min(self.height as f32) as i32;

        for py in min_y..max_y {
            let fy = (py as f32) + 0.5;
            for px in min_x..max_x {
                let fx = (px as f32) + 0.5;
                let d = dist_to_segment(fx, fy, x0, y0, x1, y1) - half_w;
                let cov = (0.5 - d).clamp(0.0, 1.0);
                if cov > 0.0 {
                    self.blend_pixel(px, py, color, cov);
                }
            }
        }
    }

    /// Draw an antialiased stroked circle (used for the magnifying glass icon).
    pub(crate) fn stroke_circle(
        &mut self,
        cx: f32,
        cy: f32,
        radius: f32,
        thickness: f32,
        color: Rgba,
    ) {
        let half_w = thickness * 0.5;
        let pad = radius + half_w + 1.5;
        let min_x = (cx - pad).floor().max(0.0) as i32;
        let max_x = (cx + pad).ceil().min(self.width as f32) as i32;
        let min_y = (cy - pad).floor().max(0.0) as i32;
        let max_y = (cy + pad).ceil().min(self.height as f32) as i32;

        for py in min_y..max_y {
            let dy = (py as f32) + 0.5 - cy;
            for px in min_x..max_x {
                let dx = (px as f32) + 0.5 - cx;
                let d = ((dx * dx + dy * dy).sqrt() - radius).abs() - half_w;
                let cov = (0.5 - d).clamp(0.0, 1.0);
                if cov > 0.0 {
                    self.blend_pixel(px, py, color, cov);
                }
            }
        }
    }

    /// Draw a crisp vector `✕` close/cancel icon centered at `(cx, cy)`.
    pub(crate) fn draw_icon_x(&mut self, cx: f32, cy: f32, half_size: f32, thickness: f32, color: Rgba) {
        self.stroke_segment(
            cx - half_size,
            cy - half_size,
            cx + half_size,
            cy + half_size,
            thickness,
            color,
        );
        self.stroke_segment(
            cx - half_size,
            cy + half_size,
            cx + half_size,
            cy - half_size,
            thickness,
            color,
        );
    }

    /// Draw a crisp vector `✓` checkmark icon centered at `(cx, cy)`.
    pub(crate) fn draw_icon_check(&mut self, cx: f32, cy: f32, scale: f32, thickness: f32, color: Rgba) {
        let x0 = cx - 3.6 * scale;
        let y0 = cy + 0.2 * scale;
        let x1 = cx - 1.1 * scale;
        let y1 = cy + 2.8 * scale;
        let x2 = cx + 3.9 * scale;
        let y2 = cy - 2.7 * scale;
        self.stroke_segment(x0, y0, x1, y1, thickness, color);
        self.stroke_segment(x1, y1, x2, y2, thickness, color);
    }

    /// Draw a crisp vector down or up chevron centered at `(cx, cy)`.
    pub(crate) fn draw_icon_chevron(&mut self, cx: f32, cy: f32, up: bool, color: Rgba) {
        let dir = if up { -1.0 } else { 1.0 };
        self.stroke_segment(cx - 2.8, cy - 1.3 * dir, cx, cy + 1.5 * dir, 1.25, color);
        self.stroke_segment(cx, cy + 1.5 * dir, cx + 2.8, cy - 1.3 * dir, 1.25, color);
    }

    /// Draw a crisp vector tilted pushpin centered at `(cx, cy)`.
    ///
    /// Colors ONLY the pushpin strokes/body itself (never a background circle), matching
    /// Eqho's owner-verified spec.
    pub(crate) fn draw_icon_pushpin(&mut self, cx: f32, cy: f32, color: Rgba) {
        // Top cap (tilted 45°)
        self.stroke_segment(cx + 0.6, cy - 4.4, cx + 4.4, cy - 0.6, 1.45, color);
        // Pin body connecting cap to collar
        self.stroke_segment(cx + 2.0, cy - 2.0, cx - 0.5, cy + 0.5, 2.7, color);
        // Collar flange perpendicular to needle
        self.stroke_segment(cx - 2.9, cy - 1.1, cx + 1.1, cy + 2.9, 1.45, color);
        // Sharp needle tip pointing down-left
        self.stroke_segment(cx - 1.0, cy + 1.0, cx - 4.2, cy + 4.2, 1.25, color);
    }

    /// Draw a crisp vector magnifying glass icon centered at `(cx, cy)`.
    pub(crate) fn draw_icon_search(&mut self, cx: f32, cy: f32, color: Rgba) {
        self.stroke_circle(cx - 1.2, cy - 1.2, 3.6, 1.25, color);
        self.stroke_segment(cx + 1.6, cy + 1.6, cx + 4.3, cy + 4.3, 1.35, color);
    }

    /// Downsample and blit a straight-RGBA8 flag bitmap (`src_w × src_h`) into `dst` with
    /// crisp box-filter averaging, `2px` rounded corners, and a subtle hairline rim.
    pub(crate) fn blit_flag_rgba(
        &mut self,
        dst: LocalRect,
        src_w: u32,
        src_h: u32,
        src_rgba: &[u8],
        rim_color: Rgba,
    ) {
        let expected = (src_w as usize).saturating_mul(src_h as usize).saturating_mul(4);
        if dst.width == 0 || dst.height == 0 || src_w == 0 || src_h == 0 || src_rgba.len() < expected
        {
            return;
        }

        let hw = (dst.width as f32) * 0.5;
        let hh = (dst.height as f32) * 0.5;
        let cx = (dst.x as f32) + hw;
        let cy = (dst.y as f32) + hh;
        let radius = 2.0_f32.min(hw).min(hh);

        let scale_x = (src_w as f32) / (dst.width as f32);
        let scale_y = (src_h as f32) / (dst.height as f32);

        for dy in 0..dst.height {
            let py = dst.y + dy as i32;
            let sy0 = ((dy as f32) * scale_y).floor().max(0.0) as u32;
            let sy1 = (((dy + 1) as f32) * scale_y)
                .ceil()
                .max((sy0 + 1) as f32)
                .min(src_h as f32) as u32;
            let fy = (py as f32) + 0.5 - cy;

            for dx in 0..dst.width {
                let px = dst.x + dx as i32;
                let fx = (px as f32) + 0.5 - cx;
                let dist = sdf_rounded_box(fx, fy, hw, hh, radius);
                let shape_cov = (0.5 - dist).clamp(0.0, 1.0);
                if shape_cov <= 0.0 {
                    continue;
                }

                let sx0 = ((dx as f32) * scale_x).floor().max(0.0) as u32;
                let sx1 = (((dx + 1) as f32) * scale_x)
                    .ceil()
                    .max((sx0 + 1) as f32)
                    .min(src_w as f32) as u32;

                let mut sum_r = 0.0_f32;
                let mut sum_g = 0.0_f32;
                let mut sum_b = 0.0_f32;
                let mut sum_a = 0.0_f32;
                let mut count = 0.0_f32;

                for sy in sy0..sy1 {
                    let row_off = (sy as usize) * (src_w as usize) * 4;
                    for sx in sx0..sx1 {
                        let off = row_off + (sx as usize) * 4;
                        let a = f32::from(src_rgba[off + 3]) / 255.0;
                        sum_r += f32::from(src_rgba[off]) * a;
                        sum_g += f32::from(src_rgba[off + 1]) * a;
                        sum_b += f32::from(src_rgba[off + 2]) * a;
                        sum_a += a;
                        count += 1.0;
                    }
                }

                if count > 0.0 && sum_a > 0.001 {
                    let avg_a = (sum_a / count).clamp(0.0, 1.0);
                    let r = (sum_r / sum_a).round().clamp(0.0, 255.0) as u8;
                    let g = (sum_g / sum_a).round().clamp(0.0, 255.0) as u8;
                    let b = (sum_b / sum_a).round().clamp(0.0, 255.0) as u8;
                    let a_u8 = (avg_a * 255.0).round().clamp(0.0, 255.0) as u8;
                    self.blend_pixel(px, py, Rgba::new(r, g, b, a_u8), shape_cov);

                    // Subtle hairline rim around flag perimeter so white flags stay crisp on light bg.
                    let inner_cov = (-0.35 - dist).clamp(0.0, 1.0);
                    let rim_cov = (shape_cov - inner_cov).clamp(0.0, 1.0);
                    if rim_cov > 0.0 {
                        self.blend_pixel(px, py, rim_color, rim_cov);
                    }
                }
            }
        }
    }

    /// Composite a child premultiplied canvas onto `self` at `(dst_x, dst_y)` with optional
    /// smooth left/right horizontal fade masks (for the peeking 3.5-chip recent-language strip).
    pub(crate) fn composite_with_edge_fade(
        &mut self,
        src: &PremulCanvas,
        dst_x: i32,
        dst_y: i32,
        fade_left_px: u32,
        fade_right_px: u32,
    ) {
        let w = src.width as i32;
        let h = src.height as i32;
        for sy in 0..h {
            let row_off = (sy as usize) * (src.width as usize) * 4;
            for sx in 0..w {
                let mut mul = 1.0_f32;
                if fade_left_px > 0 && sx < fade_left_px as i32 {
                    mul *= ((sx as f32) + 0.5) / (fade_left_px as f32);
                }
                if fade_right_px > 0 {
                    let dist_right = w - 1 - sx;
                    if dist_right < fade_right_px as i32 {
                        mul *= ((dist_right as f32) + 0.5) / (fade_right_px as f32);
                    }
                }
                let off = row_off + (sx as usize) * 4;
                let bgra = [
                    src.pixels[off],
                    src.pixels[off + 1],
                    src.pixels[off + 2],
                    src.pixels[off + 3],
                ];
                self.blend_premul_pixel(dst_x + sx, dst_y + sy, bgra, mul);
            }
        }
    }
}

/// Signed distance to an axis-aligned rounded rectangle centered at `(0, 0)`.
#[inline]
fn sdf_rounded_box(px: f32, py: f32, hw: f32, hh: f32, r: f32) -> f32 {
    let qx = px.abs() - (hw - r);
    let qy = py.abs() - (hh - r);
    let ox = qx.max(0.0);
    let oy = qy.max(0.0);
    (ox * ox + oy * oy).sqrt() + qx.max(qy).min(0.0) - r
}

/// Euclidean distance from `(px, py)` to line segment `(x0, y0)-(x1, y1)`.
#[inline]
fn dist_to_segment(px: f32, py: f32, x0: f32, y0: f32, x1: f32, y1: f32) -> f32 {
    let vx = x1 - x0;
    let vy = y1 - y0;
    let wx = px - x0;
    let wy = py - y0;
    let len_sq = vx * vx + vy * vy;
    let t = if len_sq > 1e-6 {
        ((wx * vx + wy * vy) / len_sq).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let dx = wx - t * vx;
    let dy = wy - t * vy;
    (dx * dx + dy * dy).sqrt()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::underbar::UnderbarTheme;

    #[test]
    fn canvas_renders_rounded_capsule_icons_and_downsampled_flag() {
        let theme = UnderbarTheme::for_theme(true);
        let mut c = PremulCanvas::new(64, 26);
        c.fill_rounded_rect(
            LocalRect::new(0, 0, 64, 26),
            13.0,
            theme.bar_bg,
            Some(theme.bar_border),
        );
        // Corner pixel (0, 0) outside 13px capsule radius must remain transparent.
        assert_eq!(c.pixels[3], 0);
        // Center pixel must be filled with capsule alpha.
        let center_idx = (13 * 64 + 32) * 4;
        assert!(c.pixels[center_idx + 3] > 200);

        // Draw pushpin in bright accent blue without any background circle.
        c.draw_icon_pushpin(32.0, 13.0, theme.pin_active);
        // Draw checkmark, X, chevron, search icons (no panics/out-of-bounds).
        c.draw_icon_x(12.0, 13.0, 3.5, 1.3, theme.text_muted);
        c.draw_icon_check(52.0, 13.0, 1.0, 1.5, theme.done_fg);
        c.draw_icon_chevron(22.0, 13.0, false, theme.text_muted);
        c.draw_icon_search(42.0, 13.0, theme.text_muted);

        // Downsample an 84×56 red flag into 14×10.
        let mut flag = vec![0u8; 84 * 56 * 4];
        for px in flag.chunks_exact_mut(4) {
            px.copy_from_slice(&[220, 40, 50, 255]);
        }
        c.blit_flag_rgba(LocalRect::new(20, 8, 14, 10), 84, 56, &flag, theme.flag_rim);
        let flag_mid = (13 * 64 + 27) * 4;
        // Premultiplied BGRA -> red channel (offset +2) should dominate.
        assert!(c.pixels[flag_mid + 2] > 180);
    }

    #[test]
    fn canvas_edge_fade_attenuates_right_edge_smoothly() {
        let mut parent = PremulCanvas::new(80, 22);
        let mut strip = PremulCanvas::new(80, 22);
        strip.fill_rect(LocalRect::new(0, 0, 80, 22), Rgba::rgb(255, 255, 255));
        parent.composite_with_edge_fade(&strip, 0, 0, 0, 18);

        let solid_alpha = parent.pixels[(10 * 80 + 20) * 4 + 3];
        let mid_fade_alpha = parent.pixels[(10 * 80 + 71) * 4 + 3];
        let right_edge_alpha = parent.pixels[(10 * 80 + 79) * 4 + 3];
        assert_eq!(solid_alpha, 255);
        assert!(mid_fade_alpha < solid_alpha && mid_fade_alpha > right_edge_alpha);
        assert!(right_edge_alpha < 30);
    }
}
