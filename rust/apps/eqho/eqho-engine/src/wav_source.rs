//! Pure-Rust RIFF/WAVE reader and `EQHO_TEST_AUDIO` stream feeder.

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use eqho_core::vad::{SAMPLE_RATE, WAVE_BLOCK_SAMPLES};
use eqho_platform::{AudioBlockCallback, AudioStreamHandle, PlatformError};

const WAVE_FORMAT_PCM: u16 = 1;
const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;

/// Decode a RIFF/WAVE file (PCM16, PCM24, or IEEE float32; mono or multi-channel)
/// into 16 kHz mono `f32` samples in `[-1.0, 1.0]`.
pub fn load_wav_16k_mono(path: &Path) -> Result<Vec<f32>, String> {
    let bytes = fs::read(path).map_err(|e| format!("failed to read WAV {}: {e}", path.display()))?;
    decode_wav_bytes(&bytes)
}

fn decode_wav_bytes(bytes: &[u8]) -> Result<Vec<f32>, String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("not a valid RIFF/WAVE file".into());
    }

    let mut fmt_tag: Option<u16> = None;
    let mut channels: u16 = 0;
    let mut sample_rate: u32 = 0;
    let mut bits_per_sample: u16 = 0;
    let mut data_slice: Option<&[u8]> = None;

    let mut pos = 12_usize;
    while pos + 8 <= bytes.len() {
        let chunk_id = &bytes[pos..pos + 4];
        let chunk_size = u32::from_le_bytes([
            bytes[pos + 4],
            bytes[pos + 5],
            bytes[pos + 6],
            bytes[pos + 7],
        ]) as usize;
        pos += 8;

        let end = pos.saturating_add(chunk_size).min(bytes.len());
        let payload = &bytes[pos..end];

        if chunk_id == b"fmt " {
            if payload.len() < 16 {
                return Err("WAV fmt chunk too short".into());
            }
            let raw_tag = u16::from_le_bytes([payload[0], payload[1]]);
            channels = u16::from_le_bytes([payload[2], payload[3]]);
            sample_rate = u32::from_le_bytes([payload[4], payload[5], payload[6], payload[7]]);
            bits_per_sample = u16::from_le_bytes([payload[14], payload[15]]);

            let resolved_tag = if raw_tag == WAVE_FORMAT_EXTENSIBLE && payload.len() >= 26 {
                u16::from_le_bytes([payload[24], payload[25]])
            } else {
                raw_tag
            };
            fmt_tag = Some(resolved_tag);
        } else if chunk_id == b"data" {
            data_slice = Some(payload);
        }

        // RIFF chunks are padded to an even byte boundary.
        pos = end + (chunk_size & 1);
    }

    let tag = fmt_tag.ok_or_else(|| "WAV missing fmt chunk".to_owned())?;
    let data = data_slice.ok_or_else(|| "WAV missing data chunk".to_owned())?;
    if channels == 0 || sample_rate == 0 {
        return Err("WAV has zero channels or sample rate".into());
    }

    let mono = match (tag, bits_per_sample) {
        (WAVE_FORMAT_PCM, 16) => decode_pcm16_mono(data, channels as usize),
        (WAVE_FORMAT_PCM, 24) => decode_pcm24_mono(data, channels as usize),
        (WAVE_FORMAT_IEEE_FLOAT, 32) => decode_f32_mono(data, channels as usize),
        (other_tag, other_bits) => {
            return Err(format!(
                "unsupported WAV format (tag={other_tag}, bits_per_sample={other_bits})"
            ));
        }
    };

    Ok(resample_linear(&mono, sample_rate, SAMPLE_RATE as u32))
}

fn decode_pcm16_mono(data: &[u8], channels: usize) -> Vec<f32> {
    let frame_bytes = channels * 2;
    let frame_count = data.len() / frame_bytes;
    let mut out = Vec::with_capacity(frame_count);
    let inv_ch = 1.0 / (channels as f32 * 32768.0);

    for frame in data.chunks_exact(frame_bytes) {
        let mut sum = 0.0_f32;
        for ch in frame.chunks_exact(2) {
            let sample = i16::from_le_bytes([ch[0], ch[1]]);
            sum += f32::from(sample);
        }
        out.push((sum * inv_ch).clamp(-1.0, 1.0));
    }
    out
}

fn decode_pcm24_mono(data: &[u8], channels: usize) -> Vec<f32> {
    let frame_bytes = channels * 3;
    let frame_count = data.len() / frame_bytes;
    let mut out = Vec::with_capacity(frame_count);
    let inv_ch = 1.0 / (channels as f32 * 8_388_608.0);

    for frame in data.chunks_exact(frame_bytes) {
        let mut sum = 0.0_f32;
        for ch in frame.chunks_exact(3) {
            let sign = if ch[2] & 0x80 != 0 { 0xFF } else { 0x00 };
            let sample = i32::from_le_bytes([ch[0], ch[1], ch[2], sign]);
            sum += sample as f32;
        }
        out.push((sum * inv_ch).clamp(-1.0, 1.0));
    }
    out
}

fn decode_f32_mono(data: &[u8], channels: usize) -> Vec<f32> {
    let frame_bytes = channels * 4;
    let frame_count = data.len() / frame_bytes;
    let mut out = Vec::with_capacity(frame_count);
    let inv_ch = 1.0 / channels as f32;

    for frame in data.chunks_exact(frame_bytes) {
        let mut sum = 0.0_f32;
        for ch in frame.chunks_exact(4) {
            let sample = f32::from_le_bytes([ch[0], ch[1], ch[2], ch[3]]);
            if sample.is_finite() {
                sum += sample;
            }
        }
        out.push((sum * inv_ch).clamp(-1.0, 1.0));
    }
    out
}

fn resample_linear(input: &[f32], src_rate: u32, dst_rate: u32) -> Vec<f32> {
    if input.is_empty() || src_rate == dst_rate {
        return input.to_vec();
    }
    if input.len() == 1 {
        let out_len = ((dst_rate as f64 / src_rate as f64).ceil() as usize).max(1);
        return vec![input[0]; out_len];
    }

    let ratio = src_rate as f64 / dst_rate as f64;
    let out_len = ((input.len() as f64 * dst_rate as f64) / src_rate as f64).round() as usize;
    let mut out = Vec::with_capacity(out_len);
    let max_idx = input.len() - 1;

    for i in 0..out_len {
        let pos = i as f64 * ratio;
        let idx = (pos.floor() as usize).min(max_idx);
        let frac = (pos - idx as f64) as f32;
        let next = (idx + 1).min(max_idx);
        let sample = input[idx] + (input[next] - input[idx]) * frac;
        out.push(sample);
    }
    out
}

/// Test-only microphone stand-in used when `EQHO_TEST_AUDIO` is set.
/// Feeds `WAVE_BLOCK_SAMPLES` (50 ms) blocks from a WAV file, then silence until dropped.
#[derive(Debug)]
pub struct TestAudioStream {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl AudioStreamHandle for TestAudioStream {}

impl Drop for TestAudioStream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

impl TestAudioStream {
    /// Read `path` (16 kHz mono WAV via [`load_wav_16k_mono`]) and start feeding
    /// `WAVE_BLOCK_SAMPLES` blocks into `on_block`.
    pub fn start(path: &Path, on_block: AudioBlockCallback) -> Result<Self, PlatformError> {
        let samples = load_wav_16k_mono(path).map_err(PlatformError)?;
        let fast = std::env::var("EQHO_TEST_AUDIO_FAST")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let block_delay = if fast {
            Duration::from_millis(2)
        } else {
            Duration::from_millis(50)
        };

        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = Arc::clone(&stop);

        let thread = thread::Builder::new()
            .name("eqho-test-audio".into())
            .spawn(move || {
                for chunk in samples.chunks(WAVE_BLOCK_SAMPLES) {
                    if stop_thread.load(Ordering::Acquire) {
                        return;
                    }
                    if chunk.len() == WAVE_BLOCK_SAMPLES {
                        on_block(chunk);
                    } else {
                        let mut padded = vec![0.0_f32; WAVE_BLOCK_SAMPLES];
                        padded[..chunk.len()].copy_from_slice(chunk);
                        on_block(&padded);
                    }
                    thread::sleep(block_delay);
                }

                let silence = vec![0.0_f32; WAVE_BLOCK_SAMPLES];
                while !stop_thread.load(Ordering::Acquire) {
                    on_block(&silence);
                    thread::sleep(block_delay);
                }
            })
            .map_err(|e| PlatformError(format!("failed to spawn test-audio thread: {e}")))?;

        Ok(Self {
            stop,
            thread: Some(thread),
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn build_wav(format_tag: u16, channels: u16, sample_rate: u32, bits: u16, data: &[u8]) -> Vec<u8> {
        let block_align = channels * (bits / 8);
        let byte_rate = sample_rate * u32::from(block_align);
        let data_len = data.len() as u32;
        let riff_len = 36 + data_len;

        let mut out = Vec::with_capacity(44 + data.len());
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&riff_len.to_le_bytes());
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&format_tag.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&sample_rate.to_le_bytes());
        out.extend_from_slice(&byte_rate.to_le_bytes());
        out.extend_from_slice(&block_align.to_le_bytes());
        out.extend_from_slice(&bits.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        out.extend_from_slice(data);
        out
    }

    #[test]
    fn decodes_pcm16_pcm24_and_f32_with_downmix_and_resample() {
        // Stereo 8 kHz PCM16 -> 16 kHz mono
        let mut pcm16 = Vec::new();
        for s in [16384i16, -16384i16, 8192i16, 8192i16] {
            pcm16.extend_from_slice(&s.to_le_bytes());
        }
        let wav16 = build_wav(1, 2, 8_000, 16, &pcm16);
        let out16 = decode_wav_bytes(&wav16).unwrap();
        assert_eq!(out16.len(), 4);
        assert!(out16[0].abs() < 1e-4);

        // Mono 16 kHz PCM24
        let mut pcm24 = Vec::new();
        // +0.5 = 4_194_304 = 0x400000
        pcm24.extend_from_slice(&[0x00, 0x00, 0x40]);
        let wav24 = build_wav(1, 1, 16_000, 24, &pcm24);
        let out24 = decode_wav_bytes(&wav24).unwrap();
        assert_eq!(out24.len(), 1);
        assert!((out24[0] - 0.5).abs() < 1e-4);

        // Mono 16 kHz IEEE float32
        let mut f32_bytes = Vec::new();
        f32_bytes.extend_from_slice(&(-0.25_f32).to_le_bytes());
        let wav32 = build_wav(3, 1, 16_000, 32, &f32_bytes);
        let out32 = decode_wav_bytes(&wav32).unwrap();
        assert_eq!(out32.len(), 1);
        assert!((out32[0] + 0.25).abs() < 1e-6);
    }
}
