//! Adaptive VAD, bounded phrase segmenter, block assembler, voice features,
//! waveform downsampling, and hallucination gating.

use std::collections::VecDeque;
use std::f32::consts::PI;

pub const SAMPLE_RATE: usize = 16_000;
pub const CHUNK_DURATION_S: f64 = 0.5;
pub const CHUNK_SAMPLES: usize = 8_000;
pub const WAVE_BLOCK_SAMPLES: usize = 800;
pub const WAVE_POINTS: usize = 48;

pub const SILENCE_THRESHOLD: f32 = 0.003;
pub const VAD_THRESHOLD_MIN: f32 = 0.0009;
pub const VAD_NOISE_MULTIPLIER: f32 = 3.5;
pub const SILENCE_TIMEOUT_S: f64 = 1.2;
pub const MIN_PHRASE_DURATION_S: f64 = 0.4;
pub const PARTIAL_INTERVAL_S: f64 = 1.0;
pub const PARTIAL_TAIL_SECONDS: f64 = 10.0;
pub const PARTIAL_MAX_INFER_S: f64 = 1.5;

pub const NEAR_SILENCE_FACTOR: f32 = 1.5;
pub const NO_SPEECH_PROB_MAX: f32 = 0.6;
pub const AVG_LOGPROB_MIN: f32 = -1.0;

pub const HALLUCINATION_BLOCKLIST: &[&str] = &[
    "thank you",
    "thank you very much",
    "thanks for watching",
    "thank you for watching",
    "you",
    "bye",
    "bye-bye",
    "the end",
    "so",
    "subtitles by the amara.org community",
    "1",
    "2",
];

const FFT_N: usize = 1024;

/// Reassembles small mic blocks (e.g. 50 ms / 800 samples) into exact
/// `chunk_samples` (default 8000 / 0.5 s) chunks without losing residual samples.
#[derive(Debug, Clone)]
pub struct BlockAssembler {
    chunk_samples: usize,
    buf: Vec<f32>,
}

impl Default for BlockAssembler {
    fn default() -> Self {
        Self::new(CHUNK_SAMPLES)
    }
}

impl BlockAssembler {
    pub fn new(chunk_samples: usize) -> Self {
        let cap = chunk_samples.max(1);
        Self {
            chunk_samples: cap,
            buf: Vec::with_capacity(cap * 2),
        }
    }

    pub fn push(&mut self, block: &[f32]) -> Vec<Vec<f32>> {
        self.buf.extend_from_slice(block);
        let mut chunks = Vec::new();
        while self.buf.len() >= self.chunk_samples {
            let chunk: Vec<f32> = self.buf.drain(..self.chunk_samples).collect();
            chunks.push(chunk);
        }
        chunks
    }

    pub fn flush(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.buf)
    }

    pub fn buffered_len(&self) -> usize {
        self.buf.len()
    }
}

/// Adaptive energy-based VAD threshold tracker.
#[derive(Debug, Clone)]
pub struct AdaptiveVad {
    noise_floor: Option<f32>,
    sensitivity: f32,
    threshold: f32,
}

impl Default for AdaptiveVad {
    fn default() -> Self {
        Self::new(1.0)
    }
}

impl AdaptiveVad {
    pub fn new(sensitivity: f32) -> Self {
        let sens = sanitize_sensitivity(sensitivity);
        Self {
            noise_floor: None,
            sensitivity: sens,
            threshold: SILENCE_THRESHOLD / sens,
        }
    }

    pub fn set_sensitivity(&mut self, sensitivity: f32) {
        self.sensitivity = sanitize_sensitivity(sensitivity);
    }

    pub fn sensitivity(&self) -> f32 {
        self.sensitivity
    }

    pub fn noise_floor(&self) -> Option<f32> {
        self.noise_floor
    }

    pub fn threshold(&self) -> f32 {
        self.threshold
    }

    pub fn update(&mut self, rms: f32) -> f32 {
        let rms = if rms.is_finite() { rms.max(0.0) } else { 0.0 };
        let nf = match self.noise_floor {
            None => rms,
            Some(prev) if rms < prev => rms,
            Some(prev) => prev + (rms - prev) * 0.02,
        };
        self.noise_floor = Some(nf);
        let raw_gate = (nf * VAD_NOISE_MULTIPLIER).clamp(VAD_THRESHOLD_MIN, SILENCE_THRESHOLD);
        self.threshold = raw_gate / self.sensitivity;
        self.threshold
    }
}

fn sanitize_sensitivity(sensitivity: f32) -> f32 {
    if sensitivity.is_finite() {
        sensitivity.clamp(0.5, 2.0)
    } else {
        1.0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum VadEvent {
    PartialReady(Vec<f32>),
    PhraseReady {
        audio: Vec<f32>,
        peak_rms: f32,
        threshold: f32,
    },
}

/// Bounded speech/silence phrase segmenter with pre-roll ring buffer and max
/// phrase duration cap.
#[derive(Debug, Clone)]
pub struct PhraseSegmenter {
    vad: AdaptiveVad,
    max_preroll_chunks: usize,
    max_phrase_samples: usize,
    preroll: VecDeque<Vec<f32>>,
    chunks: Vec<Vec<f32>>,
    total_samples: usize,
    utterance_peak: f32,
    is_speaking: bool,
    silence_s: f64,
    since_partial_s: f64,
}

impl Default for PhraseSegmenter {
    fn default() -> Self {
        Self::new(1.0, 2, 30.0)
    }
}

impl PhraseSegmenter {
    pub fn new(sensitivity: f32, max_preroll_chunks: usize, max_phrase_s: f64) -> Self {
        let max_phrase_samples = ((max_phrase_s.max(1.0)) * SAMPLE_RATE as f64).round() as usize;
        Self {
            vad: AdaptiveVad::new(sensitivity),
            max_preroll_chunks,
            max_phrase_samples,
            preroll: VecDeque::with_capacity(max_preroll_chunks + 1),
            chunks: Vec::new(),
            total_samples: 0,
            utterance_peak: 0.0,
            is_speaking: false,
            silence_s: 0.0,
            since_partial_s: 0.0,
        }
    }

    pub fn set_sensitivity(&mut self, sensitivity: f32) {
        self.vad.set_sensitivity(sensitivity);
    }

    pub fn noise_floor(&self) -> Option<f32> {
        self.vad.noise_floor()
    }

    pub fn threshold(&self) -> f32 {
        self.vad.threshold()
    }

    pub fn is_speaking(&self) -> bool {
        self.is_speaking
    }

    pub fn buffered_samples(&self) -> usize {
        if self.is_speaking {
            self.total_samples
        } else {
            self.preroll.iter().map(Vec::len).sum()
        }
    }

    pub fn push_chunk(&mut self, chunk: &[f32]) -> Vec<VadEvent> {
        let mut events = Vec::new();
        if chunk.is_empty() {
            return events;
        }

        let rms = chunk_rms(chunk);
        let threshold = self.vad.update(rms);
        let chunk_dur = chunk.len() as f64 / SAMPLE_RATE as f64;

        if rms > threshold {
            self.silence_s = 0.0;
            if !self.is_speaking {
                self.is_speaking = true;
                self.since_partial_s = 0.0;
                while let Some(pre) = self.preroll.pop_front() {
                    let pre_rms = chunk_rms(&pre);
                    self.utterance_peak = self.utterance_peak.max(pre_rms);
                    self.total_samples += pre.len();
                    self.chunks.push(pre);
                }
            }

            self.chunks.push(chunk.to_vec());
            self.total_samples += chunk.len();
            self.utterance_peak = self.utterance_peak.max(rms);
            self.since_partial_s += chunk_dur;

            if self.total_samples >= self.max_phrase_samples {
                if let Some(ev) = self.emit_phrase(threshold) {
                    events.push(ev);
                }
                return events;
            }

            let buf_duration = self.total_samples as f64 / SAMPLE_RATE as f64;
            if buf_duration > 1.0 && self.since_partial_s >= PARTIAL_INTERVAL_S {
                self.since_partial_s = 0.0;
                let tail_chunks = ((PARTIAL_TAIL_SECONDS / CHUNK_DURATION_S).round() as usize).max(1);
                let start = self.chunks.len().saturating_sub(tail_chunks);
                let partial_audio = concat_chunks(&self.chunks[start..]);
                events.push(VadEvent::PartialReady(partial_audio));
            }
        } else if self.is_speaking {
            self.chunks.push(chunk.to_vec());
            self.total_samples += chunk.len();
            self.utterance_peak = self.utterance_peak.max(rms);
            self.silence_s += chunk_dur;

            if self.total_samples >= self.max_phrase_samples || self.silence_s > SILENCE_TIMEOUT_S
            {
                let buf_duration = self.total_samples as f64 / SAMPLE_RATE as f64;
                if buf_duration >= MIN_PHRASE_DURATION_S {
                    if let Some(ev) = self.emit_phrase(threshold) {
                        events.push(ev);
                    }
                } else {
                    self.reset_phrase();
                }
            }
        } else {
            if self.max_preroll_chunks > 0 {
                if self.preroll.len() == self.max_preroll_chunks {
                    self.preroll.pop_front();
                }
                self.preroll.push_back(chunk.to_vec());
            }
        }

        events
    }

    pub fn flush_on_stop(&mut self) -> Option<VadEvent> {
        let threshold = self.vad.threshold();
        if self.is_speaking {
            let buf_duration = self.total_samples as f64 / SAMPLE_RATE as f64;
            if buf_duration >= MIN_PHRASE_DURATION_S {
                return self.emit_phrase(threshold);
            }
            self.reset_phrase();
            return None;
        }

        if self.preroll.is_empty() {
            return None;
        }
        let pre_chunks: Vec<Vec<f32>> = self.preroll.drain(..).collect();
        let audio = concat_chunks(&pre_chunks);
        let buf_duration = audio.len() as f64 / SAMPLE_RATE as f64;
        if buf_duration < MIN_PHRASE_DURATION_S {
            return None;
        }
        let peak_rms = pre_chunks.iter().map(|c| chunk_rms(c)).fold(0.0_f32, f32::max);
        Some(VadEvent::PhraseReady {
            audio,
            peak_rms,
            threshold,
        })
    }

    fn emit_phrase(&mut self, threshold: f32) -> Option<VadEvent> {
        if self.chunks.is_empty() {
            self.reset_phrase();
            return None;
        }
        let audio = concat_chunks(&self.chunks);
        let peak_rms = self.utterance_peak;
        self.reset_phrase();
        Some(VadEvent::PhraseReady {
            audio,
            peak_rms,
            threshold,
        })
    }

    fn reset_phrase(&mut self) {
        self.chunks.clear();
        self.total_samples = 0;
        self.utterance_peak = 0.0;
        self.is_speaking = false;
        self.silence_s = 0.0;
        self.since_partial_s = 0.0;
    }
}

fn concat_chunks(chunks: &[Vec<f32>]) -> Vec<f32> {
    let total: usize = chunks.iter().map(Vec::len).sum();
    let mut out = Vec::with_capacity(total);
    for c in chunks {
        out.extend_from_slice(c);
    }
    out
}

fn chunk_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f64 = samples.iter().map(|&x| f64::from(x) * f64::from(x)).sum();
    (sum_sq / samples.len() as f64).sqrt() as f32
}

#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub text: String,
    pub no_speech_prob: f32,
    pub avg_logprob: f32,
}

/// Apply near-silence gate, segment confidence gate, and short-clip
/// hallucination blocklist.
pub fn filter_segments(
    segments: &[Segment],
    audio_duration_s: f64,
    peak_rms: Option<f32>,
    threshold: f32,
) -> Option<String> {
    if let Some(peak) = peak_rms
        && peak < threshold * NEAR_SILENCE_FACTOR
    {
        return None;
    }

    let mut kept = Vec::new();
    for s in segments {
        let t = s.text.trim();
        if t.is_empty() {
            continue;
        }
        if s.no_speech_prob > NO_SPEECH_PROB_MAX && s.avg_logprob < AVG_LOGPROB_MIN {
            continue;
        }
        kept.push(t);
    }

    let text = kept.join(" ");
    if text.is_empty() {
        return None;
    }

    if audio_duration_s < 2.0 {
        let norm = text
            .to_lowercase()
            .trim_matches(|c: char| matches!(c, ' ' | '.' | '!' | '?' | '"'))
            .to_owned();
        if HALLUCINATION_BLOCKLIST.contains(&norm.as_str()) {
            return None;
        }
    }

    Some(text)
}

/// Normalized mic level for the overlay's audio indicator (calibrated so normal
/// speaking volume lands around mid-bar).
pub fn level_meter(rms: f32) -> f32 {
    if !rms.is_finite() || rms <= 0.0 {
        return 0.0;
    }
    (rms / 0.012).powf(0.6).min(1.0)
}

/// Signed peak-preserving downsample: per bin, the sample with the largest
/// magnitude.
pub fn downsample_wave(block: &[f32], points: usize) -> Vec<f32> {
    if points == 0 {
        return Vec::new();
    }
    let n = (block.len() / points) * points;
    if n == 0 {
        return vec![0.0; points];
    }
    let bin_len = n / points;
    let mut out = Vec::with_capacity(points);
    for i in 0..points {
        let bin = &block[i * bin_len..(i + 1) * bin_len];
        let mut best = 0.0_f32;
        let mut best_abs = -1.0_f32;
        for &s in bin {
            let a = s.abs();
            if a > best_abs {
                best_abs = a;
                best = s;
            }
        }
        out.push(best);
    }
    out
}

/// Compute `(loud, tone)` voice features for one mic block.
/// - `loud`: RMS on a dB scale, -60 dB -> 0.0 .. -24 dB -> 1.0
/// - `tone`: Hann-windowed 1024-pt FFT spectral centroid (80..=5000 Hz) log-mapped
///   250 Hz -> 0.0 .. 3000 Hz -> 1.0; `0.5` when the block is too quiet (`rms <= 1e-3`).
pub fn voice_features(block: &[f32]) -> (f32, f32) {
    if block.is_empty() {
        return (0.0, 0.5);
    }
    let rms = chunk_rms(block);
    let loud = ((20.0 * (rms + 1e-9).log10() + 60.0) / 36.0).clamp(0.0, 1.0);
    if rms <= 1e-3 {
        return (loud, 0.5);
    }

    let m = block.len().min(FFT_N);
    let mut re = [0.0_f32; FFT_N];
    let mut im = [0.0_f32; FFT_N];

    if m == 1 {
        re[0] = block[0];
    } else {
        let denom = (m - 1) as f32;
        for (i, &sample) in block[..m].iter().enumerate() {
            let win = 0.5 * (1.0 - (2.0 * PI * i as f32 / denom).cos());
            re[i] = sample * win;
        }
    }

    fft_1024(&mut re, &mut im);

    let bin_hz = SAMPLE_RATE as f32 / FFT_N as f32;
    let mut weighted_sum = 0.0_f32;
    let mut mag_sum = 0.0_f32;

    for k in 0..=(FFT_N / 2) {
        let hz = k as f32 * bin_hz;
        if (80.0..=5000.0).contains(&hz) {
            let mag = (re[k] * re[k] + im[k] * im[k]).sqrt();
            weighted_sum += mag * hz;
            mag_sum += mag;
        }
    }

    let tone = if mag_sum > 0.0 {
        let centroid = weighted_sum / mag_sum;
        ((centroid / 250.0).log2() / 12.0_f32.log2()).clamp(0.0, 1.0)
    } else {
        0.5
    };

    (loud, tone)
}

/// Pure-Rust in-place radix-2 Cooley-Tukey FFT for `FFT_N = 1024`.
fn fft_1024(re: &mut [f32; FFT_N], im: &mut [f32; FFT_N]) {
    let mut j = 0_usize;
    for i in 1..FFT_N {
        let mut bit = FFT_N >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }

    let mut len = 2_usize;
    while len <= FFT_N {
        let half = len / 2;
        let angle = -2.0 * PI / len as f32;
        let wlen_re = angle.cos();
        let wlen_im = angle.sin();

        let mut start = 0_usize;
        while start < FFT_N {
            let mut w_re = 1.0_f32;
            let mut w_im = 0.0_f32;
            for k in 0..half {
                let even = start + k;
                let odd = even + half;
                let t_re = w_re * re[odd] - w_im * im[odd];
                let t_im = w_re * im[odd] + w_im * re[odd];

                re[odd] = re[even] - t_re;
                im[odd] = im[even] - t_im;
                re[even] += t_re;
                im[even] += t_im;

                let next_w_re = w_re * wlen_re - w_im * wlen_im;
                w_im = w_re * wlen_im + w_im * wlen_re;
                w_re = next_w_re;
            }
            start += len;
        }
        len <<= 1;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn sine_wave(freq_hz: f32, amp: f32, len: usize) -> Vec<f32> {
        (0..len)
            .map(|i| amp * (2.0 * PI * freq_hz * i as f32 / SAMPLE_RATE as f32).sin())
            .collect()
    }

    #[test]
    fn voice_features_440hz_vs_2000hz_centroid_sanity() {
        let low = sine_wave(440.0, 0.05, WAVE_BLOCK_SAMPLES);
        let high = sine_wave(2000.0, 0.05, WAVE_BLOCK_SAMPLES);

        let (loud_low, tone_low) = voice_features(&low);
        let (loud_high, tone_high) = voice_features(&high);

        assert!(loud_low > 0.5);
        assert!(loud_high > 0.5);
        // log2(440/250)/log2(12) ≈ 0.227; log2(2000/250)/log2(12) ≈ 0.837
        assert!(tone_low < 0.35, "440 Hz tone was {tone_low}");
        assert!(tone_high > 0.75, "2000 Hz tone was {tone_high}");
        assert!(tone_high > tone_low + 0.4);
    }

    #[test]
    fn quiet_block_returns_neutral_tone() {
        let silence = vec![0.0001_f32; WAVE_BLOCK_SAMPLES];
        let (loud, tone) = voice_features(&silence);
        assert!(loud < 0.05);
        assert!((tone - 0.5).abs() < 1e-6);
    }

    #[test]
    fn bounded_preroll_and_max_phrase_cap() {
        let mut seg = PhraseSegmenter::new(1.0, 2, 2.0);
        let quiet = vec![0.0002_f32; CHUNK_SAMPLES];
        for _ in 0..20 {
            let evs = seg.push_chunk(&quiet);
            assert!(evs.is_empty());
        }
        assert!(!seg.is_speaking());
        assert_eq!(seg.buffered_samples(), 2 * CHUNK_SAMPLES);

        // Now feed 5 chunks of loud speech (2.5 s) -> should hit 2.0 s max_phrase_s cap at chunk 4 (2 chunks preroll + 2 chunks speech = 2.0s)
        let speech = sine_wave(300.0, 0.05, CHUNK_SAMPLES);
        let mut emitted = Vec::new();
        for _ in 0..5 {
            emitted.extend(seg.push_chunk(&speech));
        }
        assert!(
            emitted
                .iter()
                .any(|e| matches!(e, VadEvent::PhraseReady { .. }))
        );
    }

    #[test]
    fn hallucination_gating_rules() {
        let segs = vec![Segment {
            text: "Thank you.".to_owned(),
            no_speech_prob: 0.1,
            avg_logprob: -0.2,
        }];
        // Near-silence gate
        assert_eq!(filter_segments(&segs, 1.5, Some(0.001), 0.003), None);
        // Short clip blocklist
        assert_eq!(filter_segments(&segs, 1.5, Some(0.05), 0.003), None);
        // Long clip keeps it
        assert_eq!(
            filter_segments(&segs, 3.0, Some(0.05), 0.003),
            Some("Thank you.".to_owned())
        );

        // Low confidence dropped
        let low_conf = vec![Segment {
            text: "real words".to_owned(),
            no_speech_prob: 0.8,
            avg_logprob: -1.5,
        }];
        assert_eq!(filter_segments(&low_conf, 3.0, Some(0.05), 0.003), None);
    }

    #[test]
    fn downsample_wave_preserves_peaks() {
        let mut block = vec![0.0_f32; 800];
        block[5] = -0.9;
        let ds = downsample_wave(&block, WAVE_POINTS);
        assert_eq!(ds.len(), WAVE_POINTS);
        assert!((ds[0] - (-0.9)).abs() < 1e-6);
    }

    proptest! {
        #[test]
        fn block_assembler_conserves_sample_count(
            blocks in prop::collection::vec(prop::collection::vec(-1.0_f32..1.0, 0..1500), 0..30)
        ) {
            let mut asm = BlockAssembler::default();
            let mut input_total = 0_usize;
            let mut output_total = 0_usize;
            for b in &blocks {
                input_total += b.len();
                for chunk in asm.push(b) {
                    prop_assert_eq!(chunk.len(), CHUNK_SAMPLES);
                    output_total += chunk.len();
                }
            }
            let tail = asm.flush();
            output_total += tail.len();
            prop_assert_eq!(output_total, input_total);
            prop_assert_eq!(asm.buffered_len(), 0);
        }

        #[test]
        fn adaptive_vad_threshold_stays_within_bounds(
            sens in 0.1_f32..5.0,
            rms_seq in prop::collection::vec(0.0_f32..0.5, 1..50)
        ) {
            let mut vad = AdaptiveVad::new(sens);
            let clamped_sens = sens.clamp(0.5, 2.0);
            let min_t = VAD_THRESHOLD_MIN / clamped_sens;
            let max_t = SILENCE_THRESHOLD / clamped_sens;
            for rms in rms_seq {
                let t = vad.update(rms);
                prop_assert!(t >= min_t - 1e-7);
                prop_assert!(t <= max_t + 1e-7);
            }
        }
    }
}
