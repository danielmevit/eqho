//! Pure-Rust iridescent pill renderer (1:1 port of `src/pillfx.py`), plus
//! premultiplied BGRA8 output for Win32 `UpdateLayeredWindow`.

use std::f32::consts::PI;

pub fn hex_rgb01(hex_color: &str) -> Option<(f32, f32, f32)> {
    let hex = hex_color.strip_prefix('#').unwrap_or(hex_color);
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some((
        f32::from(r) / 255.0,
        f32::from(g) / 255.0,
        f32::from(b) / 255.0,
    ))
}

pub fn pill_size(scale: f64) -> (u32, u32) {
    let s = if scale.is_finite() {
        scale.clamp(0.6, 1.4)
    } else {
        0.8
    };
    let w = (280.0 * s).round().max(1.0) as u32;
    let h = (51.0 * s).round().max(1.0) as u32;
    (w, h)
}

#[derive(Debug, Clone)]
pub struct VoiceFollower {
    pub attack: f32,
    pub release: f32,
    pub stale: f32,
    target: (f32, f32),
    age: f32,
    pub loud: f32,
    pub tone: f32,
    pub phase: f32,
}

impl Default for VoiceFollower {
    fn default() -> Self {
        Self::new(0.18, 0.50, 0.3)
    }
}

impl VoiceFollower {
    pub fn new(attack: f32, release: f32, stale: f32) -> Self {
        Self {
            attack,
            release,
            stale,
            target: (0.0, 0.5),
            age: 1e9,
            loud: 0.0,
            tone: 0.5,
            phase: 0.0,
        }
    }

    pub fn feed(&mut self, loud: f32, tone: f32) {
        self.target = (loud, tone);
        self.age = 0.0;
    }

    pub fn reset(&mut self) {
        self.target = (0.0, 0.5);
        self.age = 1e9;
        self.loud = 0.0;
        self.tone = 0.5;
    }

    pub fn step(&mut self, dt: f32) -> (f32, f32, f32) {
        let dt = dt.clamp(0.0, 0.1);
        self.age += dt;
        let (loud, tone) = if self.age <= self.stale {
            self.target
        } else {
            (0.0, self.tone)
        };
        let tau = if loud > self.loud {
            self.attack
        } else {
            self.release
        };
        if tau > 0.0 {
            self.loud += (loud - self.loud) * (1.0 - (-dt / tau).exp());
        } else {
            self.loud = loud;
        }
        if loud > 0.08 {
            self.tone += (tone - self.tone) * (1.0 - (-dt / 0.30).exp());
        }
        self.phase += dt * (0.5 + 1.7 * self.loud);
        (self.loud, self.tone, self.phase)
    }
}

pub fn voice_curves(w: usize, loud: f32, tone: f32, phase: f32) -> (Vec<f32>, Vec<f32>) {
    if w == 0 {
        return (Vec::new(), Vec::new());
    }
    let loud = loud.clamp(0.0, 1.0);
    let tone = tone.clamp(0.0, 1.0);
    let e = loud * loud * (3.0 - 2.0 * loud);
    let amp = 0.05 + 0.50 * e;
    let k = 1.0 + 0.55 * tone;
    let p = phase;

    let mut c1 = Vec::with_capacity(w);
    let mut c2 = Vec::with_capacity(w);

    for i in 0..w {
        let xn = if w == 1 {
            -1.0_f32
        } else {
            -1.0 + 2.0 * (i as f32 / (w - 1) as f32)
        };
        let taper = ((xn + 1.0) * (PI / 2.0)).sin().clamp(0.0, 1.0).powf(0.8);
        let main = ((xn * (PI * 1.10 * k) + p).sin()
            + 0.45 * (xn * (PI * 1.85 * k) - p * 1.30 + 0.8).sin()
            + 0.20 * (xn * (PI * 2.70 * k) + p * 0.70 + 2.1).sin())
            / 1.65;
        let second = ((xn * (PI * 1.30 * k) + p * 0.85 + 1.9).sin()
            + 0.40 * (xn * (PI * 2.05 * k) - p * 1.10 + 3.0).sin())
            / 1.40;
        c1.push(main * taper * amp);
        c2.push(second * taper * (amp * 0.6));
    }

    (c1, c2)
}

#[inline]
fn gs(x: f32, s: f32) -> f32 {
    (-(x * x) / (2.0 * s * s)).exp()
}

#[inline]
fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[inline]
fn mix3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [mix(a[0], b[0], t), mix(a[1], b[1], t), mix(a[2], b[2], t)]
}

#[inline]
fn paint3(col: [f32; 3], target: [f32; 3], factor: [f32; 3]) -> [f32; 3] {
    [
        col[0] + (target[0] - col[0]) * factor[0],
        col[1] + (target[1] - col[1]) * factor[1],
        col[2] + (target[2] - col[2]) * factor[2],
    ]
}

fn render_pixels<F>(
    w: u32,
    h: u32,
    t: f32,
    level: f32,
    light: f32,
    voice: Option<(f32, f32, f32)>,
    mut emit: F,
) where
    F: FnMut([f32; 3], f32),
{
    if w == 0 || h == 0 {
        return;
    }
    let wf = w as f32;
    let hf = h as f32;
    let aspect = wf / hf;
    let lvl = level.clamp(0.0, 1.0);
    let light = light.clamp(0.0, 1.0);

    let r = 0.36 + 0.015 * (t * 1.6).sin() + 0.045 * lvl;
    let hw = (aspect * 0.5 - r - 0.05).max(0.0);

    let body = mix3([0.035, 0.060, 0.140], [0.955, 0.970, 0.995], light);
    let tint = mix3([0.080, 0.170, 0.420], [0.800, 0.890, 0.990], light);

    let amp = r * (0.19 + 0.22 * lvl);
    let fr = r * (0.050 + 0.045 * lvl);

    let (curves, th) = if let Some((loud, tone, vph)) = voice {
        (
            Some(voice_curves(w as usize, loud, tone, vph)),
            r * (0.17 + 0.08 * loud),
        )
    } else {
        (None, r * (0.17 + 0.10 * lvl))
    };

    let ld_len = (0.45_f32 * 0.45 + 0.60 * 0.60 + 0.66 * 0.66).sqrt();
    let ld = [-0.45 / ld_len, -0.60 / ld_len, 0.66 / ld_len];
    let sheen_tint = mix(1.0, 0.95, light);

    let aa = 1.5 / hf;
    let bw = 3.0 / hf;

    for y in 0..h {
        let uvy = y as f32 / hf - 0.5;
        for x in 0..w {
            let uvx = (x as f32 / wf - 0.5) * aspect;
            let px = uvx - uvx.clamp(-hw, hw);
            let py = uvy;
            let d = (px * px + py * py).sqrt() - r;

            let qx = px / r;
            let qy = py / r;
            let nz = (1.0 - (qx * qx + qy * qy)).max(0.0).sqrt();
            let nz_clamped = nz.max(1e-3);
            let nlen = (qx * qx + qy * qy + nz_clamped * nz_clamped).sqrt();
            let n_x = qx / nlen;
            let n_y = qy / nlen;
            let n_z = nz_clamped / nlen;

            let wx = uvx + 0.22 * (1.7 * uvy + t * 0.33).sin();
            let wy = uvy + 0.22 * (1.9 * uvx + t * 0.27).sin();
            let drift = 0.5 + 0.5 * (2.1 * wx + 2.6 * wy + t * 0.21).sin();
            let mut col = mix3(body, tint, drift * 0.55);

            let (c1, c2) = if let Some((ref m1, ref m2)) = curves {
                let xi = x as usize;
                (m1[xi] * r, m2[xi] * r - r * 0.16)
            } else {
                (
                    amp * (uvx * 2.6 - t * 1.15).sin() + amp * 0.45 * (uvx * 4.9 + t * 0.70).sin(),
                    amp * 0.75 * (uvx * 3.4 + t * 0.90 + 1.7).sin() - r * 0.16,
                )
            };

            let y1 = uvy - c1;
            let b1 = [gs(y1 - fr, th), gs(y1, th), gs(y1 + fr, th)];
            let dark_scale1 = (1.0 - light) * 0.80;
            col[0] += b1[0] * 0.250 * dark_scale1;
            col[1] += b1[1] * 0.550 * dark_scale1;
            col[2] += b1[2] * 1.000 * dark_scale1;
            let light_scale1 = light * 0.78;
            col = paint3(
                col,
                [0.100, 0.420, 0.850],
                [b1[0] * light_scale1, b1[1] * light_scale1, b1[2] * light_scale1],
            );

            let y2 = uvy - c2;
            let th2 = th * 0.55;
            let b2 = [gs(y2 + fr, th2), gs(y2, th2), gs(y2 - fr, th2)];
            let dark_scale2 = (1.0 - light) * 0.35;
            col[0] += b2[0] * 0.450 * dark_scale2;
            col[1] += b2[1] * 0.850 * dark_scale2;
            col[2] += b2[2] * 1.000 * dark_scale2;
            let light_scale2 = light * 0.60;
            col = paint3(
                col,
                [0.150, 0.620, 0.920],
                [b2[0] * light_scale2, b2[1] * light_scale2, b2[2] * light_scale2],
            );

            let ad = d + r * 0.24;
            let aw = r * 0.085;
            let lower_raw = ((qy - 0.05) / (0.6 - 0.05)).clamp(0.0, 1.0);
            let lower = lower_raw * lower_raw * (3.0 - 2.0 * lower_raw);
            let pulse = 0.6 + 0.4 * (t * 0.8 + uvx * 1.7).sin();
            let lp = lower * pulse;
            let arc = [gs(ad - fr, aw), gs(ad, aw), gs(ad + fr, aw)];
            let arc_dark = lp * (1.0 - light) * 0.85;
            col[0] += arc[0] * 0.250 * arc_dark;
            col[1] += arc[1] * 0.450 * arc_dark;
            col[2] += arc[2] * 1.000 * arc_dark;
            let arc_light = lp * light * 0.42;
            col = paint3(
                col,
                [0.250, 0.500, 0.920],
                [arc[0] * arc_light, arc[1] * arc_light, arc[2] * arc_light],
            );

            let fres = (1.0 - n_z.clamp(0.0, 1.0)).powf(2.3);
            let fres_dark = fres * (1.0 - light) * 0.65 * (0.7 + 0.5 * lvl);
            col[0] += 0.300 * fres_dark;
            col[1] += 0.550 * fres_dark;
            col[2] += 1.000 * fres_dark;
            col = mix3(col, [0.450, 0.620, 0.920], fres * light * 0.55);

            let ndl = (n_x * ld[0] + n_y * ld[1] + n_z * ld[2]).max(0.0);
            let sheen = (ndl.powf(64.0) * 0.42 + ndl.powf(6.0) * 0.12) * sheen_tint;
            col[0] += sheen * 1.00;
            col[1] += sheen * 0.97;
            col[2] += sheen * 0.98;

            col = mix3(col, [0.975, 0.975, 0.990], light * 0.06);

            let mapped = [
                1.0 - (-col[0] * 1.25).exp(),
                1.0 - (-col[1] * 1.25).exp(),
                1.0 - (-col[2] * 1.25).exp(),
            ];
            let clipped = [
                col[0].clamp(0.0, 1.0),
                col[1].clamp(0.0, 1.0),
                col[2].clamp(0.0, 1.0),
            ];
            col = [
                mix(mapped[0], clipped[0], light),
                mix(mapped[1], clipped[1], light),
                mix(mapped[2], clipped[2], light),
            ];

            let stroke = ((d + bw) / aa).clamp(0.0, 1.0) * (-d / aa).clamp(0.0, 1.0);
            col = mix3(col, [0.995, 0.995, 1.000], stroke * light * 0.85);

            let mask = (1.0 - ((d + aa) / (2.0 * aa)).clamp(0.0, 1.0)).clamp(0.0, 1.0);
            emit(col, mask);
        }
    }
}

pub fn render_rgb(
    w: u32,
    h: u32,
    t: f32,
    level: f32,
    light: f32,
    bg: [f32; 3],
    voice: Option<(f32, f32, f32)>,
) -> Vec<u8> {
    let mut out = Vec::with_capacity((w as usize) * (h as usize) * 3);
    render_pixels(w, h, t, level, light, voice, |col, mask| {
        for i in 0..3 {
            let v = bg[i] * (1.0 - mask) + col[i] * mask;
            out.push((v.clamp(0.0, 1.0) * 255.0) as u8);
        }
    });
    out
}

/// Render true per-pixel alpha premultiplied BGRA8 for Win32 `UpdateLayeredWindow`.
pub fn render_premul_bgra(
    w: u32,
    h: u32,
    t: f32,
    level: f32,
    light: f32,
    opacity: f32,
    voice: Option<(f32, f32, f32)>,
) -> Vec<u8> {
    let op = opacity.clamp(0.0, 1.0);
    let mut out = Vec::with_capacity((w as usize) * (h as usize) * 4);
    render_pixels(w, h, t, level, light, voice, |col, mask| {
        let alpha = (mask * op).clamp(0.0, 1.0);
        let a_u8 = (alpha * 255.0) as u8;
        let b_u8 = ((col[2].clamp(0.0, 1.0) * alpha * 255.0) as u8).min(a_u8);
        let g_u8 = ((col[1].clamp(0.0, 1.0) * alpha * 255.0) as u8).min(a_u8);
        let r_u8 = ((col[0].clamp(0.0, 1.0) * alpha * 255.0) as u8).min(a_u8);
        out.push(b_u8);
        out.push(g_u8);
        out.push(r_u8);
        out.push(a_u8);
    });
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn pill_size_clamps_scale() {
        assert_eq!(pill_size(1.0), (280, 51));
        assert_eq!(pill_size(0.8), (224, 41));
        assert_eq!(pill_size(0.1), pill_size(0.6));
        assert_eq!(pill_size(5.0), pill_size(1.4));
    }

    #[test]
    fn hex_rgb01_parses_colors() {
        let (r, g, b) = hex_rgb01("#58a6ff").unwrap();
        assert!((r - 88.0 / 255.0).abs() < 1e-6);
        assert!((g - 166.0 / 255.0).abs() < 1e-6);
        assert!((b - 1.0).abs() < 1e-6);
        assert!(hex_rgb01("bad").is_none());
    }

    #[test]
    fn premul_bgra_invariant_and_transparent_corners() {
        let (w, h) = (112, 21);
        let buf = render_premul_bgra(w, h, 0.5, 0.4, 0.0, 0.85, Some((0.5, 0.6, 1.2)));
        assert_eq!(buf.len(), (w as usize) * (h as usize) * 4);

        for px in buf.chunks_exact(4) {
            let (b, g, r, a) = (px[0], px[1], px[2], px[3]);
            assert!(b <= a, "B={b} > A={a}");
            assert!(g <= a, "G={g} > A={a}");
            assert!(r <= a, "R={r} > A={a}");
        }

        // Top-left corner (0, 0) is outside capsule -> alpha == 0
        assert_eq!(buf[3], 0);
        // Center pixel is inside capsule -> alpha > 0
        let center_idx = ((h / 2) * w + (w / 2)) as usize * 4;
        assert!(buf[center_idx + 3] > 180);
    }

    #[test]
    fn voice_follower_settles_after_stale() {
        let mut vf = VoiceFollower::default();
        vf.feed(0.9, 0.8);
        let (l1, _, _) = vf.step(0.05);
        assert!(l1 > 0.1);
        for _ in 0..20 {
            vf.step(0.1);
        }
        assert!(vf.loud < 0.1);
    }
}
