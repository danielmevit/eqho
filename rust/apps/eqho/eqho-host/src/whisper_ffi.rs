//! Dynamic `whisper.cpp 1.8.4` C ABI bindings via `libloading`.
//!
//! Struct layouts and function signatures match `rust/vendor/whisper-1.8.4/whisper.h`
//! verbatim.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::fmt;
use std::path::Path;

use libloading::{Library, Symbol};

use crate::error::HostError;
use crate::protocol::HostSegment;

const TARGET_SAMPLE_RATE: usize = 16_000;

type WhisperToken = i32;

#[repr(C)]
struct WhisperContext {
    _private: [u8; 0],
}

#[repr(C)]
struct WhisperState {
    _private: [u8; 0],
}

#[repr(C)]
struct WhisperGrammarElement {
    r#type: c_int,
    value: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
enum WhisperAlignmentHeadsPreset {
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

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct WhisperAhead {
    n_text_layer: c_int,
    n_head: c_int,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct WhisperAheads {
    n_heads: usize,
    heads: *const WhisperAhead,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct WhisperContextParams {
    use_gpu: bool,
    flash_attn: bool,
    gpu_device: c_int,
    dtw_token_timestamps: bool,
    dtw_aheads_preset: WhisperAlignmentHeadsPreset,
    dtw_n_top: c_int,
    dtw_aheads: WhisperAheads,
    dtw_mem_size: usize,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct WhisperTokenData {
    id: WhisperToken,
    tid: WhisperToken,
    p: f32,
    plog: f32,
    pt: f32,
    ptsum: f32,
    t0: i64,
    t1: i64,
    t_dtw: i64,
    vlen: f32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct WhisperVadParams {
    threshold: f32,
    min_speech_duration_ms: c_int,
    min_silence_duration_ms: c_int,
    max_speech_duration_s: f32,
    speech_pad_ms: c_int,
    samples_overlap: f32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WhisperSamplingStrategy {
    Greedy = 0,
    BeamSearch = 1,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct WhisperGreedyParams {
    best_of: c_int,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct WhisperBeamSearchParams {
    beam_size: c_int,
    patience: f32,
}

type WhisperNewSegmentCallback = Option<
    unsafe extern "C" fn(
        ctx: *mut WhisperContext,
        state: *mut WhisperState,
        n_new: c_int,
        user_data: *mut c_void,
    ),
>;

type WhisperProgressCallback = Option<
    unsafe extern "C" fn(
        ctx: *mut WhisperContext,
        state: *mut WhisperState,
        progress: c_int,
        user_data: *mut c_void,
    ),
>;

type WhisperEncoderBeginCallback = Option<
    unsafe extern "C" fn(
        ctx: *mut WhisperContext,
        state: *mut WhisperState,
        user_data: *mut c_void,
    ) -> bool,
>;

type GgmlAbortCallback = Option<unsafe extern "C" fn(user_data: *mut c_void) -> bool>;

type WhisperLogitsFilterCallback = Option<
    unsafe extern "C" fn(
        ctx: *mut WhisperContext,
        state: *mut WhisperState,
        tokens: *const WhisperTokenData,
        n_tokens: c_int,
        logits: *mut f32,
        user_data: *mut c_void,
    ),
>;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct WhisperFullParams {
    strategy: WhisperSamplingStrategy,
    n_threads: c_int,
    n_max_text_ctx: c_int,
    offset_ms: c_int,
    duration_ms: c_int,
    translate: bool,
    no_context: bool,
    no_timestamps: bool,
    single_segment: bool,
    print_special: bool,
    print_progress: bool,
    print_realtime: bool,
    print_timestamps: bool,
    token_timestamps: bool,
    thold_pt: f32,
    thold_ptsum: f32,
    max_len: c_int,
    split_on_word: bool,
    max_tokens: c_int,
    debug_mode: bool,
    audio_ctx: c_int,
    tdrz_enable: bool,
    suppress_regex: *const c_char,
    initial_prompt: *const c_char,
    carry_initial_prompt: bool,
    prompt_tokens: *const WhisperToken,
    prompt_n_tokens: c_int,
    language: *const c_char,
    detect_language: bool,
    suppress_blank: bool,
    suppress_nst: bool,
    temperature: f32,
    max_initial_ts: f32,
    length_penalty: f32,
    temperature_inc: f32,
    entropy_thold: f32,
    logprob_thold: f32,
    no_speech_thold: f32,
    greedy: WhisperGreedyParams,
    beam_search: WhisperBeamSearchParams,
    new_segment_callback: WhisperNewSegmentCallback,
    new_segment_callback_user_data: *mut c_void,
    progress_callback: WhisperProgressCallback,
    progress_callback_user_data: *mut c_void,
    encoder_begin_callback: WhisperEncoderBeginCallback,
    encoder_begin_callback_user_data: *mut c_void,
    abort_callback: GgmlAbortCallback,
    abort_callback_user_data: *mut c_void,
    logits_filter_callback: WhisperLogitsFilterCallback,
    logits_filter_callback_user_data: *mut c_void,
    grammar_rules: *const *const WhisperGrammarElement,
    n_grammar_rules: usize,
    i_start_rule: usize,
    grammar_penalty: f32,
    vad: bool,
    vad_model_path: *const c_char,
    vad_params: WhisperVadParams,
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

unsafe fn load_sym<T: Copy>(lib: &Library, name: &'static str) -> Result<T, HostError> {
    let mut bytes = Vec::with_capacity(name.len() + 1);
    bytes.extend_from_slice(name.as_bytes());
    bytes.push(0);
    // SAFETY: Caller guarantees `T` matches the exact C ABI signature of `name` in whisper.h.
    let sym: Symbol<'_, T> = unsafe { lib.get(&bytes) }.map_err(|source| HostError::MissingSymbol {
        symbol: name,
        source,
    })?;
    Ok(*sym)
}

impl WhisperApi {
    fn load(dll_dir: &Path) -> Result<Self, HostError> {
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
fn load_whisper_library(dll_dir: &Path) -> Result<Library, HostError> {
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
            .map_err(|e| HostError::DllDirSetup(e.to_string()))?;
    }

    let dll_path = abs_dir.join("whisper.dll");
    let path_str = dll_path.display().to_string();
    // SAFETY: Loading `whisper.dll` from the validated `dll_dir` directory.
    unsafe { Library::new(&dll_path) }.map_err(|source| HostError::LoadLibrary {
        path: path_str,
        source,
    })
}

#[cfg(not(windows))]
fn load_whisper_library(dll_dir: &Path) -> Result<Library, HostError> {
    let so_path = dll_dir.join("libwhisper.so");
    let path_str = so_path.display().to_string();
    // SAFETY: Loading `libwhisper.so` from the configured `dll_dir` directory.
    unsafe { Library::new(&so_path) }.map_err(|source| HostError::LoadLibrary {
        path: path_str,
        source,
    })
}

fn cstr_to_string(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    // SAFETY: `ptr` is a valid null-terminated C string returned by whisper.cpp.
    unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned()
}

/// Loaded `whisper.cpp 1.8.4` model context and dynamic library handle.
pub struct WhisperEngine {
    // Field order matters: `ctx` is freed in `Drop` before `api` (`Library`) is dropped.
    ctx: *mut WhisperContext,
    api: WhisperApi,
    version: String,
    system_info: String,
    use_gpu: bool,
}

impl fmt::Debug for WhisperEngine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WhisperEngine")
            .field("version", &self.version)
            .field("use_gpu", &self.use_gpu)
            .finish_non_exhaustive()
    }
}

impl Drop for WhisperEngine {
    fn drop(&mut self) {
        if !self.ctx.is_null() {
            // SAFETY: `self.ctx` was created by `whisper_init_from_file_with_params` and `self.api._lib` is still loaded.
            unsafe { (self.api.whisper_free)(self.ctx) };
            self.ctx = std::ptr::null_mut();
        }
    }
}

impl WhisperEngine {
    /// Load `whisper.dll` / `libwhisper.so`, initialize the model from `model_path`,
    /// and run a 0.5 s silence warm-up inference.
    pub fn load(dll_dir: &Path, model_path: &Path, use_gpu: bool) -> Result<Self, HostError> {
        let api = WhisperApi::load(dll_dir)?;

        // SAFETY: `whisper_version` and `whisper_print_system_info` take no arguments and return static C strings.
        let version = unsafe { cstr_to_string((api.whisper_version)()) };
        // SAFETY: See above.
        let system_info = unsafe { cstr_to_string((api.whisper_print_system_info)()) };

        // SAFETY: `whisper_context_default_params_by_ref` allocates a `WhisperContextParams` freed immediately below.
        let mut ctx_params = unsafe {
            let ptr = (api.whisper_context_default_params_by_ref)();
            if ptr.is_null() {
                return Err(HostError::NullDefaultParams);
            }
            let val = *ptr;
            (api.whisper_free_context_params)(ptr);
            val
        };
        ctx_params.use_gpu = use_gpu;
        ctx_params.flash_attn = use_gpu;

        let model_cstr = CString::new(model_path.to_string_lossy().as_bytes())?;
        // SAFETY: `model_cstr` is a valid null-terminated path and `ctx_params` is initialized from defaults.
        let ctx_ptr =
            unsafe { (api.whisper_init_from_file_with_params)(model_cstr.as_ptr(), ctx_params) };
        if ctx_ptr.is_null() {
            return Err(HostError::InitModelFailed(model_path.display().to_string()));
        }

        let mut engine = Self {
            ctx: ctx_ptr,
            api,
            version,
            system_info,
            use_gpu,
        };

        // 0.5 s silence warm-up (verifies GPU/CPU backend and warms shaders/buffers).
        let silence = vec![0.0f32; TARGET_SAMPLE_RATE / 2];
        let warmup_threads = engine.effective_threads(4);
        let _ = engine.transcribe(&silence, "en", 1, "", warmup_threads)?;

        Ok(engine)
    }

    /// Safeguard for prebuilt Windows `ggml-cpu.dll` compiled with MSVC OpenMP (`vcomp140.dll`),
    /// which faults (`0xC0000005`) when `n_threads > 1` on CPU-only inference. GPU Vulkan runs
    /// multi-threaded cleanly; once R4 CI builds `ggml-cpu` with `-DGGML_OPENMP=OFF`, `OPENMP = 1`
    /// disappears from `system_info` and multi-threaded CPU unlocks automatically.
    fn effective_threads(&self, requested: u32) -> u32 {
        let clamped = requested.max(1);
        #[cfg(windows)]
        if !self.use_gpu
            && self.system_info.contains("OPENMP = 1")
            && std::env::var_os("EQHO_ALLOW_CPU_OPENMP_MT").is_none()
        {
            return 1;
        }
        clamped
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn system_info(&self) -> &str {
        &self.system_info
    }

    pub fn use_gpu(&self) -> bool {
        self.use_gpu
    }

    /// Transcribe mono 16 kHz `f32` PCM samples and return segments with real
    /// `no_speech_prob` and mean non-special-token `avg_logprob`.
    pub fn transcribe(
        &mut self,
        audio: &[f32],
        language: &str,
        beam_size: u32,
        initial_prompt: &str,
        threads: u32,
    ) -> Result<Vec<HostSegment>, HostError> {
        if audio.is_empty() {
            return Ok(Vec::new());
        }
        let n_samples = c_int::try_from(audio.len())
            .map_err(|_| HostError::AudioTooLarge(audio.len()))?;

        let strategy = if beam_size > 1 {
            WhisperSamplingStrategy::BeamSearch
        } else {
            WhisperSamplingStrategy::Greedy
        };

        // SAFETY: `whisper_full_default_params_by_ref` allocates a `WhisperFullParams` freed immediately below.
        let mut full_params = unsafe {
            let ptr = (self.api.whisper_full_default_params_by_ref)(strategy);
            if ptr.is_null() {
                return Err(HostError::NullDefaultParams);
            }
            let val = *ptr;
            (self.api.whisper_free_params)(ptr);
            val
        };

        let lang_arg = if language.is_empty() { "auto" } else { language };
        let lang_cstr = CString::new(lang_arg.as_bytes())?;
        let prompt_cstr = if initial_prompt.is_empty() {
            None
        } else {
            Some(CString::new(initial_prompt.as_bytes())?)
        };

        full_params.n_threads = self.effective_threads(threads).min(i32::MAX as u32) as c_int;
        full_params.language = lang_cstr.as_ptr();
        full_params.initial_prompt = prompt_cstr
            .as_ref()
            .map_or(std::ptr::null(), |s| s.as_ptr());
        full_params.print_progress = false;
        full_params.print_realtime = false;
        full_params.print_special = false;
        full_params.print_timestamps = false;
        full_params.no_timestamps = true;
        full_params.no_context = true;
        full_params.single_segment = false;
        full_params.suppress_blank = true;
        full_params.suppress_nst = true;
        if beam_size > 1 {
            full_params.beam_search.beam_size = beam_size.min(i32::MAX as u32) as c_int;
        } else {
            // `whisper_full_default_params_by_ref(Greedy)` defaults `best_of` to 5, which multiplies
            // decode latency on distilled/turbo models without improving live dictation accuracy.
            full_params.greedy.best_of = 1;
        }

        // SAFETY: `self.ctx` is valid, `lang_cstr` and `prompt_cstr` outlive this call,
        // and `audio` has `n_samples` contiguous `f32` elements.
        let rc = unsafe { (self.api.whisper_full)(self.ctx, full_params, audio.as_ptr(), n_samples) };
        if rc != 0 {
            return Err(HostError::InferenceFailed(rc));
        }

        // SAFETY: `self.ctx` is valid after a successful `whisper_full` call.
        let n_segments = unsafe { (self.api.whisper_full_n_segments)(self.ctx) };
        // SAFETY: `self.ctx` is valid.
        let eot_id = unsafe { (self.api.whisper_token_eot)(self.ctx) };

        let mut segments = Vec::with_capacity(n_segments.max(0) as usize);
        for i_seg in 0..n_segments {
            // SAFETY: `i_seg` is in `0..n_segments`.
            let text =
                unsafe { cstr_to_string((self.api.whisper_full_get_segment_text)(self.ctx, i_seg)) };
            // SAFETY: `i_seg` is in `0..n_segments`.
            let no_speech_prob =
                unsafe { (self.api.whisper_full_get_segment_no_speech_prob)(self.ctx, i_seg) };
            // SAFETY: `i_seg` is in `0..n_segments`.
            let n_tokens = unsafe { (self.api.whisper_full_n_tokens)(self.ctx, i_seg) };

            let mut logprob_sum = 0.0f64;
            let mut logprob_count = 0usize;
            for i_tok in 0..n_tokens {
                // SAFETY: `i_seg` and `i_tok` are within bounds.
                let tok = unsafe { (self.api.whisper_full_get_token_data)(self.ctx, i_seg, i_tok) };
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

            segments.push(HostSegment {
                text,
                no_speech_prob,
                avg_logprob,
            });
        }

        Ok(segments)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::mem::{align_of, offset_of, size_of};

    #[test]
    fn abi_struct_layouts_match_whisper_h() {
        assert_eq!(size_of::<WhisperAlignmentHeadsPreset>(), size_of::<c_int>());
        assert_eq!(size_of::<WhisperSamplingStrategy>(), size_of::<c_int>());
        assert_eq!(size_of::<WhisperAhead>(), 8);
        assert_eq!(size_of::<WhisperAheads>(), 2 * size_of::<usize>());
        assert!(size_of::<WhisperContextParams>() >= 24);
        assert!(size_of::<WhisperTokenData>() >= 48);
        assert_eq!(size_of::<WhisperVadParams>(), 24);
        assert!(size_of::<WhisperFullParams>() > 128);
        assert_eq!(align_of::<WhisperFullParams>(), align_of::<*const c_void>());

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
