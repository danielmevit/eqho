//! Start/stop feedback chimes — G4 (392 Hz) + B4 (494 Hz) with Hann envelope.

use std::f32::consts::PI;

pub const CHIME_SAMPLE_RATE: u32 = 44_100;
pub const CHIME_VOLUME: f32 = 0.06;

pub fn synthesize_tone(freq_hz: f32, duration_s: f32) -> Vec<f32> {
    let n = (CHIME_SAMPLE_RATE as f32 * duration_s) as usize;
    if n == 0 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(n);
    let denom = if n > 1 { (n - 1) as f32 } else { 1.0 };
    for i in 0..n {
        let t = i as f32 / CHIME_SAMPLE_RATE as f32;
        let wave = (2.0 * PI * freq_hz * t).sin();
        let env = if n == 1 {
            1.0
        } else {
            0.5 * (1.0 - (2.0 * PI * i as f32 / denom).cos())
        };
        out.push(wave * env * CHIME_VOLUME);
    }
    out
}

pub fn start_chime() -> Vec<f32> {
    let mut out = synthesize_tone(392.0, 0.12);
    out.extend_from_slice(&synthesize_tone(494.0, 0.16));
    out
}

pub fn stop_chime() -> Vec<f32> {
    let mut out = synthesize_tone(494.0, 0.12);
    out.extend_from_slice(&synthesize_tone(392.0, 0.16));
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn chime_lengths_and_bounds() {
        let start = start_chime();
        let stop = stop_chime();
        let expected_len = (44_100.0 * 0.12_f32) as usize + (44_100.0 * 0.16_f32) as usize;
        assert_eq!(start.len(), expected_len);
        assert_eq!(stop.len(), expected_len);
        assert!(start.iter().all(|&s| s.abs() <= CHIME_VOLUME + 1e-6));
        assert!(start[0].abs() < 1e-6);
        assert!(start[start.len() - 1].abs() < 1e-4);
    }
}
