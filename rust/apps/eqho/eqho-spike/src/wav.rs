//! Pure-Rust RIFF/WAVE reader for 16-bit PCM and 32-bit IEEE float audio.

use thiserror::Error;

/// Target sample rate expected by whisper.cpp (16 kHz).
pub const TARGET_SAMPLE_RATE: u32 = 16_000;

/// Decoded mono 16 kHz float audio.
#[derive(Debug, Clone)]
pub struct WavAudio {
    pub samples: Vec<f32>,
    #[cfg_attr(not(test), allow(dead_code))]
    pub source_sample_rate: u32,
    #[cfg_attr(not(test), allow(dead_code))]
    pub source_channels: u16,
    pub duration_sec: f64,
}

#[derive(Debug, Error)]
pub enum WavError {
    #[error("I/O error reading WAV file: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid RIFF/WAVE header")]
    InvalidHeader,
    #[error("missing fmt or data chunk")]
    MissingChunk,
    #[error("unsupported WAV format tag {format_tag} with {bits_per_sample} bits/sample")]
    UnsupportedFormat {
        format_tag: u16,
        bits_per_sample: u16,
    },
    #[error("unsupported channel count: {0} (expected 1 or 2)")]
    UnsupportedChannels(u16),
    #[error("invalid sample rate: {0}")]
    InvalidSampleRate(u32),
    #[error("truncated WAV data chunk")]
    TruncatedData,
}

/// Read a WAV file from disk and return mono 16 kHz `f32` samples.
pub fn load_wav_file(path: &std::path::Path) -> Result<WavAudio, WavError> {
    let bytes = std::fs::read(path)?;
    parse_wav_bytes(&bytes)
}

/// Parse an in-memory RIFF/WAVE buffer into mono 16 kHz `f32` samples.
pub fn parse_wav_bytes(data: &[u8]) -> Result<WavAudio, WavError> {
    if data.len() < 12 || &data[0..4] != b"RIFF" || &data[8..12] != b"WAVE" {
        return Err(WavError::InvalidHeader);
    }

    let mut format_tag: Option<u16> = None;
    let mut channels: Option<u16> = None;
    let mut sample_rate: Option<u32> = None;
    let mut bits_per_sample: Option<u16> = None;
    let mut pcm_bytes: Option<&[u8]> = None;

    let mut offset = 12usize;
    while offset + 8 <= data.len() {
        let chunk_id = &data[offset..offset + 4];
        let chunk_size = u32::from_le_bytes([
            data[offset + 4],
            data[offset + 5],
            data[offset + 6],
            data[offset + 7],
        ]) as usize;
        let payload_start = offset + 8;
        let payload_end = payload_start.saturating_add(chunk_size).min(data.len());

        if chunk_id == b"fmt " {
            if payload_end - payload_start < 16 {
                return Err(WavError::InvalidHeader);
            }
            let fmt = &data[payload_start..payload_end];
            let mut tag = u16::from_le_bytes([fmt[0], fmt[1]]);
            let ch = u16::from_le_bytes([fmt[2], fmt[3]]);
            let sr = u32::from_le_bytes([fmt[4], fmt[5], fmt[6], fmt[7]]);
            let bps = u16::from_le_bytes([fmt[14], fmt[15]]);

            // Handle WAVE_FORMAT_EXTENSIBLE (0xFFFE) when subformat GUID is present.
            if tag == 0xFFFE && fmt.len() >= 40 {
                tag = u16::from_le_bytes([fmt[24], fmt[25]]);
            }

            format_tag = Some(tag);
            channels = Some(ch);
            sample_rate = Some(sr);
            bits_per_sample = Some(bps);
        } else if chunk_id == b"data" {
            if payload_start + chunk_size > data.len() {
                return Err(WavError::TruncatedData);
            }
            pcm_bytes = Some(&data[payload_start..payload_start + chunk_size]);
        }

        // RIFF chunks are word-aligned (padded to an even byte boundary).
        let padded = chunk_size + (chunk_size & 1);
        offset = payload_start.saturating_add(padded);
    }

    let (Some(tag), Some(ch), Some(sr), Some(bps), Some(raw)) =
        (format_tag, channels, sample_rate, bits_per_sample, pcm_bytes)
    else {
        return Err(WavError::MissingChunk);
    };

    if ch != 1 && ch != 2 {
        return Err(WavError::UnsupportedChannels(ch));
    }
    if sr == 0 {
        return Err(WavError::InvalidSampleRate(sr));
    }

    let interleaved: Vec<f32> = match (tag, bps) {
        (1, 16) => raw
            .chunks_exact(2)
            .map(|c| f32::from(i16::from_le_bytes([c[0], c[1]])) / 32768.0)
            .collect(),
        (3, 32) => raw
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect(),
        _ => {
            return Err(WavError::UnsupportedFormat {
                format_tag: tag,
                bits_per_sample: bps,
            });
        }
    };

    let mono: Vec<f32> = if ch == 2 {
        interleaved
            .chunks_exact(2)
            .map(|lr| 0.5 * (lr[0] + lr[1]))
            .collect()
    } else {
        interleaved
    };

    let samples = if sr == TARGET_SAMPLE_RATE {
        mono
    } else {
        resample_linear(&mono, sr, TARGET_SAMPLE_RATE)
    };
    let duration_sec = samples.len() as f64 / f64::from(TARGET_SAMPLE_RATE);

    Ok(WavAudio {
        samples,
        source_sample_rate: sr,
        source_channels: ch,
        duration_sec,
    })
}

fn resample_linear(input: &[f32], src_rate: u32, dst_rate: u32) -> Vec<f32> {
    if input.is_empty() || src_rate == dst_rate {
        return input.to_vec();
    }
    let out_len = ((input.len() as u64 * u64::from(dst_rate)) / u64::from(src_rate)) as usize;
    if out_len == 0 {
        return Vec::new();
    }
    let ratio = src_rate as f64 / dst_rate as f64;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let pos = i as f64 * ratio;
        let idx = pos as usize;
        let frac = (pos - idx as f64) as f32;
        let s0 = input[idx.min(input.len() - 1)];
        let s1 = input[(idx + 1).min(input.len() - 1)];
        out.push(s0 + (s1 - s0) * frac);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_wav(format_tag: u16, channels: u16, sample_rate: u32, bps: u16, payload: &[u8]) -> Vec<u8> {
        let block_align = channels * (bps / 8);
        let byte_rate = sample_rate * u32::from(block_align);
        let riff_size = 4 + (8 + 16) + (8 + payload.len() as u32);
        let mut buf = Vec::with_capacity(8 + riff_size as usize);
        buf.extend_from_slice(b"RIFF");
        buf.extend_from_slice(&riff_size.to_le_bytes());
        buf.extend_from_slice(b"WAVE");
        buf.extend_from_slice(b"fmt ");
        buf.extend_from_slice(&16u32.to_le_bytes());
        buf.extend_from_slice(&format_tag.to_le_bytes());
        buf.extend_from_slice(&channels.to_le_bytes());
        buf.extend_from_slice(&sample_rate.to_le_bytes());
        buf.extend_from_slice(&byte_rate.to_le_bytes());
        buf.extend_from_slice(&block_align.to_le_bytes());
        buf.extend_from_slice(&bps.to_le_bytes());
        buf.extend_from_slice(b"data");
        buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        buf.extend_from_slice(payload);
        buf
    }

    #[test]
    fn roundtrip_pcm16_mono_16khz() -> Result<(), WavError> {
        let expected = [0.0f32, 0.5, -0.5, 0.25, -0.25];
        let mut payload = Vec::new();
        for &s in &expected {
            let v = (s * 32768.0).round() as i16;
            payload.extend_from_slice(&v.to_le_bytes());
        }
        let wav = build_wav(1, 1, 16_000, 16, &payload);
        let parsed = parse_wav_bytes(&wav)?;
        assert_eq!(parsed.source_sample_rate, 16_000);
        assert_eq!(parsed.source_channels, 1);
        assert_eq!(parsed.samples.len(), expected.len());
        for (a, b) in parsed.samples.iter().zip(expected.iter()) {
            assert!((a - b).abs() < 1e-4, "mismatch {a} vs {b}");
        }
        Ok(())
    }

    #[test]
    fn roundtrip_ieee_float_stereo_resampled() -> Result<(), WavError> {
        // 48 kHz stereo -> 16 kHz mono (3x decimation)
        let frames = 48usize;
        let mut payload = Vec::with_capacity(frames * 8);
        for i in 0..frames {
            let left = (i as f32 / frames as f32) * 0.8;
            let right = (i as f32 / frames as f32) * 0.4;
            payload.extend_from_slice(&left.to_le_bytes());
            payload.extend_from_slice(&right.to_le_bytes());
        }
        let wav = build_wav(3, 2, 48_000, 32, &payload);
        let parsed = parse_wav_bytes(&wav)?;
        assert_eq!(parsed.source_sample_rate, 48_000);
        assert_eq!(parsed.source_channels, 2);
        assert_eq!(parsed.samples.len(), 16);
        for (i, &s) in parsed.samples.iter().enumerate() {
            let expected = ((i * 3) as f32 / frames as f32) * 0.6;
            assert!((s - expected).abs() < 1e-5, "frame {i}: {s} vs {expected}");
        }
        Ok(())
    }
}
