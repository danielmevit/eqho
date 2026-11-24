//! Framed stdin/stdout IPC protocol for `Eqho.exe --model-host`.
//!
//! Control messages are newline-delimited JSON (`HostRequest` / `HostResponse`).
//! `HostRequest::Transcribe` is immediately followed on the wire by
//! `sample_count * 4` little-endian IEEE-754 `f32` PCM bytes so 30 s audio
//! buffers avoid JSON float serialization overhead.

use std::io::{BufRead, Write};

use serde::{Deserialize, Serialize};

use crate::error::HostError;

/// Maximum audio frame accepted over IPC: 120 seconds at 16 kHz.
pub const MAX_AUDIO_SAMPLES: usize = 1_920_000;

/// Request sent from the parent process to the child model host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostRequest {
    Load {
        model_key: String,
        model_dir: String,
        dll_dir: String,
        use_gpu: bool,
        threads: u32,
    },
    Transcribe {
        sample_count: usize,
        language: String,
        beam_size: u32,
        initial_prompt: String,
    },
    Shutdown,
}

/// Per-segment transcription output with whisper.cpp confidence metrics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostSegment {
    pub text: String,
    pub no_speech_prob: f32,
    pub avg_logprob: f32,
}

impl From<HostSegment> for eqho_core::vad::Segment {
    fn from(seg: HostSegment) -> Self {
        Self {
            text: seg.text,
            no_speech_prob: seg.no_speech_prob,
            avg_logprob: seg.avg_logprob,
        }
    }
}

impl From<&HostSegment> for eqho_core::vad::Segment {
    fn from(seg: &HostSegment) -> Self {
        Self {
            text: seg.text.clone(),
            no_speech_prob: seg.no_speech_prob,
            avg_logprob: seg.avg_logprob,
        }
    }
}

/// Response sent from the child model host to the parent process.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HostResponse {
    Progress {
        message: String,
        fraction: Option<f32>,
    },
    Ready {
        backend: String,
        model: String,
        device: String,
        whisper_version: String,
    },
    Segments {
        segments: Vec<HostSegment>,
    },
    Error {
        message: String,
        is_oom: bool,
    },
}

/// Write a [`HostRequest`] (plus optional raw `f32` LE PCM payload for `Transcribe`)
/// and flush the writer.
pub fn write_request<W: Write>(
    writer: &mut W,
    req: &HostRequest,
    audio: Option<&[f32]>,
) -> Result<(), HostError> {
    if let HostRequest::Transcribe { sample_count, .. } = req {
        if *sample_count > MAX_AUDIO_SAMPLES {
            return Err(HostError::OversizedAudio {
                samples: *sample_count,
                max_samples: MAX_AUDIO_SAMPLES,
            });
        }
        let samples = audio.ok_or_else(|| {
            HostError::InvalidFrame("Transcribe request requires audio slice".into())
        })?;
        if samples.len() != *sample_count {
            return Err(HostError::InvalidFrame(format!(
                "Transcribe sample_count ({sample_count}) does not match slice len ({})",
                samples.len()
            )));
        }
        serde_json::to_writer(&mut *writer, req)?;
        writer.write_all(b"\n")?;
        write_f32_le_samples(writer, samples)?;
    } else {
        if audio.is_some() {
            return Err(HostError::InvalidFrame(
                "non-Transcribe request must not include audio payload".into(),
            ));
        }
        serde_json::to_writer(&mut *writer, req)?;
        writer.write_all(b"\n")?;
    }
    writer.flush()?;
    Ok(())
}

/// Read the next [`HostRequest`] (and any trailing `f32` LE PCM payload for `Transcribe`).
/// Returns `Ok(None)` on clean EOF.
pub fn read_request<R: BufRead>(
    reader: &mut R,
) -> Result<Option<(HostRequest, Vec<f32>)>, HostError> {
    let mut line = String::new();
    let n = reader.read_line(&mut line)?;
    if n == 0 {
        return Ok(None);
    }
    let req: HostRequest = serde_json::from_str(line.trim())?;
    match &req {
        HostRequest::Transcribe { sample_count, .. } => {
            if *sample_count > MAX_AUDIO_SAMPLES {
                return Err(HostError::OversizedAudio {
                    samples: *sample_count,
                    max_samples: MAX_AUDIO_SAMPLES,
                });
            }
            let audio = read_f32_le_samples(reader, *sample_count)?;
            Ok(Some((req, audio)))
        }
        HostRequest::Load { .. } | HostRequest::Shutdown => Ok(Some((req, Vec::new()))),
    }
}

/// Write a [`HostResponse`] JSON line and flush the writer.
pub fn write_response<W: Write>(writer: &mut W, resp: &HostResponse) -> Result<(), HostError> {
    serde_json::to_writer(&mut *writer, resp)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

/// Read the next [`HostResponse`] JSON line. Returns `Ok(None)` on clean EOF.
pub fn read_response<R: BufRead>(reader: &mut R) -> Result<Option<HostResponse>, HostError> {
    let mut line = String::new();
    let n = reader.read_line(&mut line)?;
    if n == 0 {
        return Ok(None);
    }
    let resp: HostResponse = serde_json::from_str(line.trim())?;
    Ok(Some(resp))
}

fn write_f32_le_samples<W: Write>(writer: &mut W, samples: &[f32]) -> Result<(), HostError> {
    const CHUNK_SAMPLES: usize = 1024;
    let mut buf = [0u8; CHUNK_SAMPLES * 4];
    for chunk in samples.chunks(CHUNK_SAMPLES) {
        let byte_len = chunk.len() * 4;
        for (dst, &sample) in buf[..byte_len].chunks_exact_mut(4).zip(chunk.iter()) {
            dst.copy_from_slice(&sample.to_le_bytes());
        }
        writer.write_all(&buf[..byte_len])?;
    }
    Ok(())
}

fn read_f32_le_samples<R: BufRead>(
    reader: &mut R,
    sample_count: usize,
) -> Result<Vec<f32>, HostError> {
    let mut out = Vec::with_capacity(sample_count);
    const CHUNK_SAMPLES: usize = 1024;
    let mut buf = [0u8; CHUNK_SAMPLES * 4];
    let mut remaining = sample_count;
    while remaining > 0 {
        let take = remaining.min(CHUNK_SAMPLES);
        let byte_len = take * 4;
        reader.read_exact(&mut buf[..byte_len])?;
        for bytes in buf[..byte_len].chunks_exact(4) {
            out.push(f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]));
        }
        remaining -= take;
    }
    Ok(out)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn request_round_trip_load_transcribe_shutdown() {
        let mut wire = Vec::new();
        let load = HostRequest::Load {
            model_key: "distil-large-v3".into(),
            model_dir: "/models".into(),
            dll_dir: "/dlls".into(),
            use_gpu: true,
            threads: 4,
        };
        write_request(&mut wire, &load, None).unwrap();

        let audio: Vec<f32> = (0..16_000)
            .map(|i| ((i as f32) * 0.01).sin() * 0.25)
            .collect();
        let transcribe = HostRequest::Transcribe {
            sample_count: audio.len(),
            language: "en".into(),
            beam_size: 1,
            initial_prompt: "Eqho".into(),
        };
        write_request(&mut wire, &transcribe, Some(&audio)).unwrap();

        write_request(&mut wire, &HostRequest::Shutdown, None).unwrap();

        let mut cursor = Cursor::new(wire);
        let (r1, a1) = read_request(&mut cursor).unwrap().unwrap();
        assert_eq!(r1, load);
        assert!(a1.is_empty());

        let (r2, a2) = read_request(&mut cursor).unwrap().unwrap();
        assert_eq!(r2, transcribe);
        assert_eq!(a2.len(), 16_000);
        assert_eq!(a2, audio);

        let (r3, a3) = read_request(&mut cursor).unwrap().unwrap();
        assert_eq!(r3, HostRequest::Shutdown);
        assert!(a3.is_empty());

        assert!(read_request(&mut cursor).unwrap().is_none());
    }

    #[test]
    fn response_round_trip_and_segment_conversion() {
        let mut wire = Vec::new();
        let resps = vec![
            HostResponse::Progress {
                message: "downloading".into(),
                fraction: Some(0.5),
            },
            HostResponse::Ready {
                backend: "whisper.cpp".into(),
                model: "base".into(),
                device: "gpu".into(),
                whisper_version: "1.8.4".into(),
            },
            HostResponse::Segments {
                segments: vec![HostSegment {
                    text: "hello world".into(),
                    no_speech_prob: 0.05,
                    avg_logprob: -0.15,
                }],
            },
            HostResponse::Error {
                message: "std::bad_alloc".into(),
                is_oom: true,
            },
        ];
        for r in &resps {
            write_response(&mut wire, r).unwrap();
        }

        let mut cursor = Cursor::new(wire);
        for expected in &resps {
            let actual = read_response(&mut cursor).unwrap().unwrap();
            assert_eq!(&actual, expected);
        }
        assert!(read_response(&mut cursor).unwrap().is_none());

        let seg = HostSegment {
            text: "hi".into(),
            no_speech_prob: 0.1,
            avg_logprob: -0.2,
        };
        let vad_seg: eqho_core::vad::Segment = seg.into();
        assert_eq!(vad_seg.text, "hi");
        assert_eq!(vad_seg.no_speech_prob, 0.1);
        assert_eq!(vad_seg.avg_logprob, -0.2);
    }

    #[test]
    fn rejects_oversized_audio_frame_on_write_and_read() {
        let too_many = MAX_AUDIO_SAMPLES + 1;
        let req = HostRequest::Transcribe {
            sample_count: too_many,
            language: "en".into(),
            beam_size: 1,
            initial_prompt: String::new(),
        };

        let mut wire = Vec::new();
        let err = write_request(&mut wire, &req, Some(&[])).unwrap_err();
        assert!(matches!(
            err,
            HostError::OversizedAudio {
                samples,
                max_samples
            } if samples == too_many && max_samples == MAX_AUDIO_SAMPLES
        ));

        let raw = format!(
            "{}\n",
            serde_json::to_string(&req).expect("serialize oversized req")
        );
        let mut cursor = Cursor::new(raw.into_bytes());
        let read_err = read_request(&mut cursor).unwrap_err();
        assert!(matches!(
            read_err,
            HostError::OversizedAudio {
                samples,
                max_samples
            } if samples == too_many && max_samples == MAX_AUDIO_SAMPLES
        ));
    }
}
