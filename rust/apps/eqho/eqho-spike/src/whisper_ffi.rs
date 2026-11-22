//! Exact C ABI bindings to `rust/vendor/whisper-1.8.4/whisper.h` via `libloading`.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::path::Path;
use std::time::Instant;

use libloading::{Library, Symbol};
use serde::Serialize;
use thiserror::Error;

use crate::wav::{TARGET_SAMPLE_RATE, load_wav_file};

pub type WhisperToken = i32;

/// Opaque C struct `whisper_context`.
#[repr(C)]
#[derive(Debug)]
pub struct WhisperContext {
    _private: [u8; 0],
}

/// Opaque C struct `whisper_state`.
#[repr(C)]
#[derive(Debug)]
pub struct WhisperState {
    _private: [u8; 0],
}

/// Opaque C struct `whisper_grammar_element`.
#[repr(C)]
#[derive(Debug)]
pub struct WhisperGrammarElement {
    pub r#type: c_int,
    pub value: u32,
}

/// `enum whisper_alignment_heads_preset`
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum WhisperAlignmentHeadsPreset {
    None = 0,
    NTopMost = 1,
    Custom = 2,
    TinyEn = 3,
    Tiny = 4,
    BaseEn = 5,
    Base = 6,
    SmallEn = 7,
    Small = 8,
    MediumEn = 9,
    Medium = 10,
    LargeV1 = 11,
    LargeV2 = 12,
    LargeV3 = 13,
    LargeV3Turbo = 14,
}

/// `struct whisper_ahead`
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct WhisperAhead {
    pub n_text_layer: c_int,
    pub n_head: c_int,
}

/// `struct whisper_aheads`
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct WhisperAheads {
    pub n_heads: usize,
    pub heads: *const WhisperAhead,
}

/// `struct whisper_context_params`
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct WhisperContextParams {
    pub use_gpu: bool,
    pub flash_attn: bool,
    pub gpu_device: c_int,
    pub dtw_token_timestamps: bool,
    pub dtw_aheads_preset: WhisperAlignmentHeadsPreset,
    pub dtw_n_top: c_int,
    pub dtw_aheads: WhisperAheads,
    pub dtw_mem_size: usize,
}

/// `struct whisper_token_data`
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct WhisperTokenData {
    pub id: WhisperToken,
    pub tid: WhisperToken,
    pub p: f32,
    pub plog: f32,
    pub pt: f32,
    pub ptsum: f32,
    pub t0: i64,
    pub t1: i64,
    pub t_dtw: i64,
    pub vlen: f32,
}

/// `struct whisper_vad_params`
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct WhisperVadParams {
    pub threshold: f32,
    pub min_speech_duration_ms: c_int,
    pub min_silence_duration_ms: c_int,
    pub max_speech_duration_s: f32,
    pub speech_pad_ms: c_int,
    pub samples_overlap: f32,
}

/// `enum whisper_sampling_strategy`
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhisperSamplingStrategy {
    Greedy = 0,
    BeamSearch = 1,
}

/// Nested `greedy` struct in `whisper_full_params`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct WhisperGreedyParams {
    pub best_of: c_int,
}

/// Nested `beam_search` struct in `whisper_full_params`.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct WhisperBeamSearchParams {
    pub beam_size: c_int,
    pub patience: f32,
}

pub type WhisperNewSegmentCallback = Option<
    unsafe extern "C" fn(
        ctx: *mut WhisperContext,
        state: *mut WhisperState,
        n_new: c_int,
        user_data: *mut c_void,
    ),
>;

pub type WhisperProgressCallback = Option<
    unsafe extern "C" fn(
        ctx: *mut WhisperContext,
        state: *mut WhisperState,
        progress: c_int,
        user_data: *mut c_void,
    ),
>;

pub type WhisperEncoderBeginCallback = Option<
    unsafe extern "C" fn(
        ctx: *mut WhisperContext,
        state: *mut WhisperState,
        user_data: *mut c_void,
    ) -> bool,
>;

pub type GgmlAbortCallback = Option<unsafe extern "C" fn(user_data: *mut c_void) -> bool>;

pub type WhisperLogitsFilterCallback = Option<
    unsafe extern "C" fn(
        ctx: *mut WhisperContext,
        state: *mut WhisperState,
        tokens: *const WhisperTokenData,
        n_tokens: c_int,
        logits: *mut f32,
        user_data: *mut c_void,
    ),
>;

/// `struct whisper_full_params`
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct WhisperFullParams {
    pub strategy: WhisperSamplingStrategy,
    pub n_threads: c_int,
    pub n_max_text_ctx: c_int,
    pub offset_ms: c_int,
    pub duration_ms: c_int,
    pub translate: bool,
    pub no_context: bool,
    pub no_timestamps: bool,
    pub single_segment: bool,
    pub print_special: bool,
    pub print_progress: bool,
    pub print_realtime: bool,
    pub print_timestamps: bool,
    pub token_timestamps: bool,
    pub thold_pt: f32,
    pub thold_ptsum: f32,
    pub max_len: c_int,
    pub split_on_word: bool,
    pub max_tokens: c_int,
    pub debug_mode: bool,
    pub audio_ctx: c_int,
    pub tdrz_enable: bool,
    pub suppress_regex: *const c_char,
    pub initial_prompt: *const c_char,
    pub carry_initial_prompt: bool,
    pub prompt_tokens: *const WhisperToken,
    pub prompt_n_tokens: c_int,
    pub language: *const c_char,
    pub detect_language: bool,
    pub suppress_blank: bool,
    pub suppress_nst: bool,
    pub temperature: f32,
    pub max_initial_ts: f32,
    pub length_penalty: f32,
    pub temperature_inc: f32,
    pub entropy_thold: f32,
    pub logprob_thold: f32,
    pub no_speech_thold: f32,
    pub greedy: WhisperGreedyParams,
    pub beam_search: WhisperBeamSearchParams,
    pub new_segment_callback: WhisperNewSegmentCallback,
    pub new_segment_callback_user_data: *mut c_void,
    pub progress_callback: WhisperProgressCallback,
    pub progress_callback_user_data: *mut c_void,
    pub encoder_begin_callback: WhisperEncoderBeginCallback,
    pub encoder_begin_callback_user_data: *mut c_void,
    pub abort_callback: GgmlAbortCallback,
    pub abort_callback_user_data: *mut c_void,
    pub logits_filter_callback: WhisperLogitsFilterCallback,
    pub logits_filter_callback_user_data: *mut c_void,
    pub grammar_rules: *const *const WhisperGrammarElement,
    pub n_grammar_rules: usize,
    pub i_start_rule: usize,
    pub grammar_penalty: f32,
    pub vad: bool,
    pub vad_model_path: *const c_char,
    pub vad_params: WhisperVadParams,
}

#[derive(Debug, Error)]
pub enum WhisperError {
    #[error("failed to load whisper library from {path}: {source}")]
    LoadLibrary {
        path: String,
        source: libloading::Error,
    },
    #[error("missing symbol {symbol}: {source}")]
    MissingSymbol {
        symbol: &'static str,
        source: libloading::Error,
    },
    #[error("whisper_init_from_file_with_params returned null for model {0}")]
    InitModelFailed(String),
    #[error("whisper_full failed with error code {0}")]
    InferenceFailed(c_int),
    #[error("default params pointer returned null")]
    NullDefaultParams,
    #[error("invalid C string argument: {0}")]
    InvalidCString(#[from] std::ffi::NulError),
    #[error("WAV error: {0}")]
    Wav(#[from] crate::wav::WavError),
    #[error("Windows DLL directory setup failed: {0}")]
    #[cfg_attr(not(windows), allow(dead_code))]
    DllDirSetup(String),
}

#[derive(Debug, Serialize)]
pub struct SegmentOutput {
    pub text: String,
    pub no_speech_prob: f32,
    pub avg_logprob: f32,
}

#[derive(Debug, Serialize)]
pub struct TranscribeOutput {
    pub ok: bool,
    pub whisper_version: String,
    pub system_info: String,
    pub use_gpu: bool,
    pub load_ms: f64,
    pub warmup_ms: f64,
    pub infer_ms: f64,
    pub audio_sec: f64,
    pub rtf: f64,
    pub text: String,
    pub segments: Vec<SegmentOutput>,
}

/// Configuration for the `transcribe` subcommand.
#[derive(Debug)]
pub struct TranscribeArgs {
    pub dll_dir: std::path::PathBuf,
    pub model: std::path::PathBuf,
    pub wav: std::path::PathBuf,
    pub gpu: bool,
    pub lang: String,
    pub threads: i32,
    pub beam: i32,
}

struct WhisperApi {
    _lib: Library,
    whisper_version: unsafe extern "C" fn() -> *const c_char,
    whisper_print_system_info: unsafe extern "C" fn() -> *const c_char,
    whisper_context_default_params_by_ref: unsafe extern "C" fn() -> *mut WhisperContextParams,
    whisper_free_context_params: unsafe extern "C" fn(params: *mut WhisperContextParams),
    whisper_init_from_file_with_params:
        unsafe extern "C" fn(path_model: *const c_char, params: WhisperContextParams) -> *mut WhisperContext,
    whisper_free: unsafe extern "C" fn(ctx: *mut WhisperContext),
    whisper_full_default_params_by_ref:
        unsafe extern "C" fn(strategy: WhisperSamplingStrategy) -> *mut WhisperFullParams,
    whisper_free_params: unsafe extern "C" fn(params: *mut WhisperFullParams),
    whisper_full: unsafe extern "C" fn(
        ctx: *mut WhisperContext,
        params: WhisperFullParams,
        samples: *const f32,
        n_samples: c_int,
    ) -> c_int,
    whisper_full_n_segments: unsafe extern "C" fn(ctx: *mut WhisperContext) -> c_int,
    whisper_full_get_segment_text:
        unsafe extern "C" fn(ctx: *mut WhisperContext, i_segment: c_int) -> *const c_char,
    whisper_full_get_segment_no_speech_prob:
        unsafe extern "C" fn(ctx: *mut WhisperContext, i_segment: c_int) -> f32,
    whisper_full_n_tokens:
        unsafe extern "C" fn(ctx: *mut WhisperContext, i_segment: c_int) -> c_int,
    whisper_full_get_token_data: unsafe extern "C" fn(
        ctx: *mut WhisperContext,
        i_segment: c_int,
        i_token: c_int,
    ) -> WhisperTokenData,
    whisper_token_eot: unsafe extern "C" fn(ctx: *mut WhisperContext) -> WhisperToken,
}

unsafe fn load_sym<T: Copy>(lib: &Library, name: &'static str) -> Result<T, WhisperError> {
    let mut bytes = Vec::with_capacity(name.len() + 1);
    bytes.extend_from_slice(name.as_bytes());
    bytes.push(0);
    // SAFETY: Caller ensures `T` matches the exact C ABI signature of `name` in whisper.h.
    let sym: Symbol<'_, T> = unsafe { lib.get(&bytes) }.map_err(|source| WhisperError::MissingSymbol {
        symbol: name,
        source,
    })?;
    Ok(*sym)
}

impl WhisperApi {
    fn load(dll_dir: &Path) -> Result<Self, WhisperError> {
        let lib = load_whisper_library(dll_dir)?;
        // SAFETY: Every symbol name and signature matches `rust/vendor/whisper-1.8.4/whisper.h`.
        unsafe {
            Ok(Self {
                whisper_version: load_sym(&lib, "whisper_version")?,
                whisper_print_system_info: load_sym(&lib, "whisper_print_system_info")?,
                whisper_context_default_params_by_ref: load_sym(
                    &lib,
                    "whisper_context_default_params_by_ref",
                )?,
                whisper_free_context_params: load_sym(&lib, "whisper_free_context_params")?,
                whisper_init_from_file_with_params: load_sym(
                    &lib,
                    "whisper_init_from_file_with_params",
                )?,
                whisper_free: load_sym(&lib, "whisper_free")?,
                whisper_full_default_params_by_ref: load_sym(
                    &lib,
                    "whisper_full_default_params_by_ref",
                )?,
                whisper_free_params: load_sym(&lib, "whisper_free_params")?,
                whisper_full: load_sym(&lib, "whisper_full")?,
                whisper_full_n_segments: load_sym(&lib, "whisper_full_n_segments")?,
                whisper_full_get_segment_text: load_sym(&lib, "whisper_full_get_segment_text")?,
                whisper_full_get_segment_no_speech_prob: load_sym(
                    &lib,
                    "whisper_full_get_segment_no_speech_prob",
                )?,
                whisper_full_n_tokens: load_sym(&lib, "whisper_full_n_tokens")?,
                whisper_full_get_token_data: load_sym(&lib, "whisper_full_get_token_data")?,
                whisper_token_eot: load_sym(&lib, "whisper_token_eot")?,
                _lib: lib,
            })
        }
    }
}

#[cfg(windows)]
fn load_whisper_library(dll_dir: &Path) -> Result<Library, WhisperError> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::System::LibraryLoader::SetDllDirectoryW;
    use windows::core::PCWSTR;

    let abs_dir = std::fs::canonicalize(dll_dir).unwrap_or_else(|_| dll_dir.to_path_buf());
    let wide_dir: Vec<u16> = abs_dir
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    // SAFETY: `wide_dir` is a valid null-terminated UTF-16 path.
    unsafe {
        SetDllDirectoryW(PCWSTR(wide_dir.as_ptr()))
            .map_err(|e| WhisperError::DllDirSetup(e.to_string()))?;
    }

    let dll_path = abs_dir.join("whisper.dll");
    let path_str = dll_path.display().to_string();
    // SAFETY: Loading `whisper.dll` from the validated `--dll-dir` directory.
    unsafe { Library::new(&dll_path) }.map_err(|source| WhisperError::LoadLibrary {
        path: path_str,
        source,
    })
}

#[cfg(not(windows))]
fn load_whisper_library(dll_dir: &Path) -> Result<Library, WhisperError> {
    let so_path = dll_dir.join("libwhisper.so");
    let path_str = so_path.display().to_string();
    // SAFETY: Loading `libwhisper.so` from the user-specified `--dll-dir`.
    unsafe { Library::new(&so_path) }.map_err(|source| WhisperError::LoadLibrary {
        path: path_str,
        source,
    })
}

struct ContextGuard<'a> {
    api: &'a WhisperApi,
    ctx: *mut WhisperContext,
}

impl Drop for ContextGuard<'_> {
    fn drop(&mut self) {
        if !self.ctx.is_null() {
            // SAFETY: `self.ctx` was returned by `whisper_init_from_file_with_params` and has not been freed.
            unsafe { (self.api.whisper_free)(self.ctx) };
        }
    }
}

fn cstr_to_string(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    // SAFETY: `ptr` is a valid null-terminated static/context C string returned by whisper.cpp.
    unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned()
}

/// Run the `transcribe` subcommand and return structured JSON output.
pub fn run_transcribe(args: &TranscribeArgs) -> Result<TranscribeOutput, WhisperError> {
    let api = WhisperApi::load(&args.dll_dir)?;
    let wav = load_wav_file(&args.wav)?;

    // SAFETY: `whisper_version` and `whisper_print_system_info` take no arguments and return static C strings.
    let whisper_version = unsafe { cstr_to_string((api.whisper_version)()) };
    // SAFETY: See above.
    let system_info = unsafe { cstr_to_string((api.whisper_print_system_info)()) };

    // SAFETY: `whisper_context_default_params_by_ref` allocates a `WhisperContextParams` freed below.
    let mut ctx_params = unsafe {
        let ptr = (api.whisper_context_default_params_by_ref)();
        if ptr.is_null() {
            return Err(WhisperError::NullDefaultParams);
        }
        let val = *ptr;
        (api.whisper_free_context_params)(ptr);
        val
    };
    ctx_params.use_gpu = args.gpu;
    ctx_params.flash_attn = args.gpu;

    let model_cstr = CString::new(args.model.to_string_lossy().as_bytes())?;
    let t_load = Instant::now();
    // SAFETY: `model_cstr` is a valid null-terminated UTF-8 path and `ctx_params` is initialized from defaults.
    let ctx_ptr = unsafe { (api.whisper_init_from_file_with_params)(model_cstr.as_ptr(), ctx_params) };
    let load_ms = t_load.elapsed().as_secs_f64() * 1000.0;
    if ctx_ptr.is_null() {
        return Err(WhisperError::InitModelFailed(args.model.display().to_string()));
    }
    let guard = ContextGuard {
        api: &api,
        ctx: ctx_ptr,
    };

    let strategy = if args.beam > 1 {
        WhisperSamplingStrategy::BeamSearch
    } else {
        WhisperSamplingStrategy::Greedy
    };

    // SAFETY: `whisper_full_default_params_by_ref` allocates a `WhisperFullParams` freed below.
    let mut full_params = unsafe {
        let ptr = (api.whisper_full_default_params_by_ref)(strategy);
        if ptr.is_null() {
            return Err(WhisperError::NullDefaultParams);
        }
        let val = *ptr;
        (api.whisper_free_params)(ptr);
        val
    };

    let lang_cstr = CString::new(args.lang.as_bytes())?;
    full_params.n_threads = args.threads.max(1);
    full_params.language = lang_cstr.as_ptr();
    full_params.print_progress = false;
    full_params.print_realtime = false;
    full_params.print_special = false;
    full_params.print_timestamps = false;
    full_params.no_timestamps = true;
    full_params.no_context = true;
    full_params.single_segment = false;
    full_params.suppress_blank = true;
    full_params.suppress_nst = true;
    if args.beam > 1 {
        full_params.beam_search.beam_size = args.beam;
    }

    // 0.5 s silence warm-up.
    let silence = vec![0.0f32; (TARGET_SAMPLE_RATE / 2) as usize];
    let t_warmup = Instant::now();
    // SAFETY: `guard.ctx` is valid, `lang_cstr` outlives the call, and `silence` has `silence.len()` samples.
    let warmup_rc = unsafe {
        (api.whisper_full)(
            guard.ctx,
            full_params,
            silence.as_ptr(),
            silence.len() as c_int,
        )
    };
    let warmup_ms = t_warmup.elapsed().as_secs_f64() * 1000.0;
    if warmup_rc != 0 {
        return Err(WhisperError::InferenceFailed(warmup_rc));
    }

    let t_infer = Instant::now();
    // SAFETY: `guard.ctx` is valid, `lang_cstr` outlives the call, and `wav.samples` is contiguous f32 PCM.
    let infer_rc = unsafe {
        (api.whisper_full)(
            guard.ctx,
            full_params,
            wav.samples.as_ptr(),
            wav.samples.len() as c_int,
        )
    };
    let infer_ms = t_infer.elapsed().as_secs_f64() * 1000.0;
    if infer_rc != 0 {
        return Err(WhisperError::InferenceFailed(infer_rc));
    }

    // SAFETY: `guard.ctx` is valid after a successful `whisper_full` call.
    let n_segments = unsafe { (api.whisper_full_n_segments)(guard.ctx) };
    // SAFETY: `guard.ctx` is valid.
    let eot_id = unsafe { (api.whisper_token_eot)(guard.ctx) };

    let mut segments = Vec::with_capacity(n_segments.max(0) as usize);
    let mut full_text = String::new();

    for i_seg in 0..n_segments {
        // SAFETY: `i_seg` is in `0..n_segments`.
        let seg_text = unsafe { cstr_to_string((api.whisper_full_get_segment_text)(guard.ctx, i_seg)) };
        // SAFETY: `i_seg` is in `0..n_segments`.
        let no_speech_prob =
            unsafe { (api.whisper_full_get_segment_no_speech_prob)(guard.ctx, i_seg) };
        // SAFETY: `i_seg` is in `0..n_segments`.
        let n_tokens = unsafe { (api.whisper_full_n_tokens)(guard.ctx, i_seg) };

        let mut logprob_sum = 0.0f64;
        let mut logprob_count = 0usize;
        for i_tok in 0..n_tokens {
            // SAFETY: `i_seg` and `i_tok` are within bounds.
            let tok = unsafe { (api.whisper_full_get_token_data)(guard.ctx, i_seg, i_tok) };
            if tok.id < eot_id && tok.plog.is_finite() {
                logprob_sum += f64::from(tok.plog);
                logprob_count += 1;
            }
        }
        let avg_logprob = if logprob_count > 0 {
            (logprob_sum / logprob_count as f64) as f32
        } else {
            0.0
        };

        full_text.push_str(&seg_text);
        segments.push(SegmentOutput {
            text: seg_text,
            no_speech_prob,
            avg_logprob,
        });
    }

    let rtf = if wav.duration_sec > 0.0 {
        (infer_ms / 1000.0) / wav.duration_sec
    } else {
        0.0
    };

    Ok(TranscribeOutput {
        ok: true,
        whisper_version,
        system_info,
        use_gpu: args.gpu,
        load_ms,
        warmup_ms,
        infer_ms,
        audio_sec: wav.duration_sec,
        rtf,
        text: full_text.trim().to_string(),
        segments,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{align_of, offset_of, size_of};

    #[test]
    fn abi_struct_layouts_are_non_zero_and_aligned() {
        assert_eq!(size_of::<WhisperAlignmentHeadsPreset>(), size_of::<c_int>());
        assert_eq!(size_of::<WhisperSamplingStrategy>(), size_of::<c_int>());
        assert_eq!(size_of::<WhisperAhead>(), 8);
        assert_eq!(size_of::<WhisperAheads>(), 2 * size_of::<usize>());
        assert!(size_of::<WhisperContextParams>() >= 24);
        assert!(size_of::<WhisperTokenData>() >= 48);
        assert_eq!(size_of::<WhisperVadParams>(), 24);
        assert!(size_of::<WhisperFullParams>() > 128);
        assert_eq!(align_of::<WhisperFullParams>(), align_of::<*const c_void>());

        // Verify key field offsets match C layout on 64-bit platforms.
        assert_eq!(offset_of!(WhisperContextParams, use_gpu), 0);
        assert_eq!(offset_of!(WhisperContextParams, flash_attn), 1);
        assert_eq!(offset_of!(WhisperContextParams, gpu_device), 4);
        assert_eq!(offset_of!(WhisperFullParams, strategy), 0);
        assert_eq!(offset_of!(WhisperFullParams, n_threads), 4);
        assert_eq!(offset_of!(WhisperTokenData, id), 0);
        assert_eq!(offset_of!(WhisperTokenData, tid), 4);
        assert_eq!(offset_of!(WhisperTokenData, p), 8);
        assert_eq!(offset_of!(WhisperTokenData, plog), 12);
    }
}
