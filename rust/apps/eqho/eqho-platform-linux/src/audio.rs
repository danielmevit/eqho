//! Linux 16 kHz mono float32 microphone capture and chime playback via dynamic `libpulse-simple.so.0`.
//!
//! Works transparently on both PipeWire (`pipewire-pulse`) and PulseAudio with zero build-time
//! C library dependencies and automatic hardware resampling straight to `16000 Hz mono f32`.

use std::ffi::{CStr, CString, c_char, c_int, c_uint, c_void};
use std::process::{Command, Stdio};
use std::ptr::null;
use std::sync::{Arc, OnceLock};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};

use eqho_platform::{
    AudioBackend, AudioBlockCallback, AudioDeviceInfo, AudioStreamHandle, PlatformError,
};
use libloading::Library;

use crate::pure::{is_monitor_source, parse_pactl_sources};

const TARGET_SAMPLE_RATE: u32 = 16_000;
const BLOCK_SAMPLES: usize = 800; // 50 ms at 16 kHz

const PA_STREAM_PLAYBACK: c_int = 1;
const PA_STREAM_RECORD: c_int = 2;
const PA_SAMPLE_FLOAT32LE: c_int = 5;

#[repr(C)]
struct PaSampleSpec {
    format: c_int,
    rate: u32,
    channels: u8,
}

#[repr(C)]
struct PaBufferAttr {
    maxlength: u32,
    tlength: u32,
    prebuf: u32,
    minreq: u32,
    fragsize: u32,
}

struct PulseSimpleApi {
    _lib: Library,
    pa_simple_new: unsafe extern "C" fn(
        server: *const c_char,
        name: *const c_char,
        dir: c_int,
        dev: *const c_char,
        stream_name: *const c_char,
        ss: *const PaSampleSpec,
        map: *const c_void,
        attr: *const PaBufferAttr,
        error: *mut c_int,
    ) -> *mut c_void,
    pa_simple_read:
        unsafe extern "C" fn(*mut c_void, *mut c_void, usize, *mut c_int) -> c_int,
    pa_simple_write:
        unsafe extern "C" fn(*mut c_void, *const c_void, usize, *mut c_int) -> c_int,
    pa_simple_drain: unsafe extern "C" fn(*mut c_void, *mut c_int) -> c_int,
    pa_simple_free: unsafe extern "C" fn(*mut c_void),
}

impl PulseSimpleApi {
    fn load() -> Result<Self, PlatformError> {
        // SAFETY: Standard PulseAudio Simple API (`libpulse-simple.so.0`, backed by PipeWire or PulseAudio).
        unsafe {
            let lib = Library::new("libpulse-simple.so.0").map_err(|e| {
                PlatformError(format!(
                    "libpulse-simple.so.0 unavailable (install pipewire-pulse or libpulse0): {e}"
                ))
            })?;
            Ok(Self {
                pa_simple_new: *lib
                    .get(b"pa_simple_new\0")
                    .map_err(|e| PlatformError(e.to_string()))?,
                pa_simple_read: *lib
                    .get(b"pa_simple_read\0")
                    .map_err(|e| PlatformError(e.to_string()))?,
                pa_simple_write: *lib
                    .get(b"pa_simple_write\0")
                    .map_err(|e| PlatformError(e.to_string()))?,
                pa_simple_drain: *lib
                    .get(b"pa_simple_drain\0")
                    .map_err(|e| PlatformError(e.to_string()))?,
                pa_simple_free: *lib
                    .get(b"pa_simple_free\0")
                    .map_err(|e| PlatformError(e.to_string()))?,
                _lib: lib,
            })
        }
    }
}

struct LinuxAudioStream {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl AudioStreamHandle for LinuxAudioStream {}

impl Drop for LinuxAudioStream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

const PA_CONTEXT_READY: c_int = 4;
const PA_CONTEXT_FAILED: c_int = 5;
const PA_CONTEXT_TERMINATED: c_int = 6;
const PA_OPERATION_RUNNING: c_int = 0;

#[repr(C)]
struct PaServerInfo {
    user_name: *const c_char,
    host_name: *const c_char,
    server_version: *const c_char,
    server_name: *const c_char,
    sample_spec: PaSampleSpec,
    default_sink_name: *const c_char,
    default_source_name: *const c_char,
}

#[repr(C)]
struct PaSourceInfoHeader {
    name: *const c_char,
    index: u32,
    description: *const c_char,
}

type PaServerInfoCb =
    unsafe extern "C" fn(*mut c_void, *const PaServerInfo, *mut c_void);
type PaSourceInfoCb =
    unsafe extern "C" fn(*mut c_void, *const PaSourceInfoHeader, c_int, *mut c_void);

struct PulseIntrospectApi {
    _lib: Library,
    pa_mainloop_new: unsafe extern "C" fn() -> *mut c_void,
    pa_mainloop_get_api: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    pa_mainloop_iterate: unsafe extern "C" fn(*mut c_void, c_int, *mut c_int) -> c_int,
    pa_mainloop_free: unsafe extern "C" fn(*mut c_void),
    pa_context_new: unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void,
    pa_context_connect:
        unsafe extern "C" fn(*mut c_void, *const c_char, c_int, *const c_void) -> c_int,
    pa_context_get_state: unsafe extern "C" fn(*mut c_void) -> c_int,
    pa_context_disconnect: unsafe extern "C" fn(*mut c_void),
    pa_context_unref: unsafe extern "C" fn(*mut c_void),
    pa_context_get_server_info:
        unsafe extern "C" fn(*mut c_void, PaServerInfoCb, *mut c_void) -> *mut c_void,
    pa_context_get_source_info_list:
        unsafe extern "C" fn(*mut c_void, PaSourceInfoCb, *mut c_void) -> *mut c_void,
    pa_operation_get_state: unsafe extern "C" fn(*mut c_void) -> c_int,
    pa_operation_unref: unsafe extern "C" fn(*mut c_void),
}

// SAFETY: `PulseIntrospectApi` holds immutable function pointers and a process-lifetime library handle.
unsafe impl Send for PulseIntrospectApi {}
unsafe impl Sync for PulseIntrospectApi {}

static PULSE_INTROSPECT_API: OnceLock<Option<PulseIntrospectApi>> = OnceLock::new();

impl PulseIntrospectApi {
    fn load() -> Option<&'static Self> {
        PULSE_INTROSPECT_API
            .get_or_init(|| {
                // SAFETY: Standard PulseAudio client API (`libpulse.so.0`, backed by PipeWire or PulseAudio).
                unsafe {
                    let lib = Library::new("libpulse.so.0").ok()?;
                    Some(Self {
                        pa_mainloop_new: *lib.get(b"pa_mainloop_new\0").ok()?,
                        pa_mainloop_get_api: *lib.get(b"pa_mainloop_get_api\0").ok()?,
                        pa_mainloop_iterate: *lib.get(b"pa_mainloop_iterate\0").ok()?,
                        pa_mainloop_free: *lib.get(b"pa_mainloop_free\0").ok()?,
                        pa_context_new: *lib.get(b"pa_context_new\0").ok()?,
                        pa_context_connect: *lib.get(b"pa_context_connect\0").ok()?,
                        pa_context_get_state: *lib.get(b"pa_context_get_state\0").ok()?,
                        pa_context_disconnect: *lib.get(b"pa_context_disconnect\0").ok()?,
                        pa_context_unref: *lib.get(b"pa_context_unref\0").ok()?,
                        pa_context_get_server_info: *lib
                            .get(b"pa_context_get_server_info\0")
                            .ok()?,
                        pa_context_get_source_info_list: *lib
                            .get(b"pa_context_get_source_info_list\0")
                            .ok()?,
                        pa_operation_get_state: *lib.get(b"pa_operation_get_state\0").ok()?,
                        pa_operation_unref: *lib.get(b"pa_operation_unref\0").ok()?,
                        _lib: lib,
                    })
                }
            })
            .as_ref()
    }
}

unsafe extern "C" fn on_pa_server_info(
    _ctx: *mut c_void,
    info: *const PaServerInfo,
    userdata: *mut c_void,
) {
    if info.is_null() || userdata.is_null() {
        return;
    }
    // SAFETY: `info` is valid for the duration of the callback and `userdata` points to our `Option<String>`.
    unsafe {
        let name_ptr = (*info).default_source_name;
        if !name_ptr.is_null() {
            let name = CStr::from_ptr(name_ptr).to_string_lossy().trim().to_owned();
            if !name.is_empty() {
                let slot = &mut *userdata.cast::<Option<String>>();
                *slot = Some(name);
            }
        }
    }
}

unsafe extern "C" fn on_pa_source_info(
    _ctx: *mut c_void,
    info: *const PaSourceInfoHeader,
    eol: c_int,
    userdata: *mut c_void,
) {
    if eol != 0 || info.is_null() || userdata.is_null() {
        return;
    }
    // SAFETY: `info` is valid while `eol == 0` and `userdata` points to our `Vec<(String, String)>`.
    unsafe {
        let raw_name = if (*info).name.is_null() {
            String::new()
        } else {
            CStr::from_ptr((*info).name).to_string_lossy().trim().to_owned()
        };
        if raw_name.is_empty() {
            return;
        }
        let desc = if (*info).description.is_null() {
            raw_name.clone()
        } else {
            let d = CStr::from_ptr((*info).description)
                .to_string_lossy()
                .trim()
                .to_owned();
            if d.is_empty() { raw_name.clone() } else { d }
        };
        if !is_monitor_source(&raw_name, &desc) {
            let list = &mut *userdata.cast::<Vec<(String, String)>>();
            list.push((raw_name, desc));
        }
    }
}

type PulseSourceList = (Vec<(String, String)>, Option<String>);

/// PipeWire / PulseAudio audio capture & playback backend.
#[derive(Debug, Default, Clone, Copy)]
pub struct LinuxAudioBackend;

impl LinuxAudioBackend {
    fn enumerate_sources_via_libpulse() -> Option<PulseSourceList> {
        let api = PulseIntrospectApi::load()?;
        // SAFETY: Dedicated single-threaded PulseAudio mainloop created, iterated, and destroyed in this call.
        unsafe {
            let ml = (api.pa_mainloop_new)();
            if ml.is_null() {
                return None;
            }
            let ml_api = (api.pa_mainloop_get_api)(ml);
            let ctx = (api.pa_context_new)(ml_api, c"Eqho".as_ptr());
            if ctx.is_null() {
                (api.pa_mainloop_free)(ml);
                return None;
            }
            if (api.pa_context_connect)(ctx, null(), 0, null()) < 0 {
                (api.pa_context_unref)(ctx);
                (api.pa_mainloop_free)(ml);
                return None;
            }

            let mut retval: c_int = 0;
            let mut ready = false;
            for _ in 0..500 {
                let st = (api.pa_context_get_state)(ctx);
                if st == PA_CONTEXT_READY {
                    ready = true;
                    break;
                }
                if st == PA_CONTEXT_FAILED || st == PA_CONTEXT_TERMINATED {
                    break;
                }
                if (api.pa_mainloop_iterate)(ml, 1, &mut retval) < 0 {
                    break;
                }
            }
            if !ready {
                (api.pa_context_disconnect)(ctx);
                (api.pa_context_unref)(ctx);
                (api.pa_mainloop_free)(ml);
                return None;
            }

            let mut default_name: Option<String> = None;
            let op_srv = (api.pa_context_get_server_info)(
                ctx,
                on_pa_server_info,
                (&raw mut default_name).cast(),
            );
            if !op_srv.is_null() {
                for _ in 0..500 {
                    if (api.pa_operation_get_state)(op_srv) != PA_OPERATION_RUNNING {
                        break;
                    }
                    if (api.pa_mainloop_iterate)(ml, 1, &mut retval) < 0 {
                        break;
                    }
                }
                (api.pa_operation_unref)(op_srv);
            }

            let mut sources: Vec<(String, String)> = Vec::new();
            let op_src = (api.pa_context_get_source_info_list)(
                ctx,
                on_pa_source_info,
                (&raw mut sources).cast(),
            );
            if !op_src.is_null() {
                for _ in 0..500 {
                    if (api.pa_operation_get_state)(op_src) != PA_OPERATION_RUNNING {
                        break;
                    }
                    if (api.pa_mainloop_iterate)(ml, 1, &mut retval) < 0 {
                        break;
                    }
                }
                (api.pa_operation_unref)(op_src);
            }

            (api.pa_context_disconnect)(ctx);
            (api.pa_context_unref)(ctx);
            (api.pa_mainloop_free)(ml);
            Some((sources, default_name))
        }
    }

    fn enumerate_sources() -> (Vec<(String, String)>, Option<String>) {
        // Primary: native `libpulse.so.0` introspection (works on PipeWire & PulseAudio without `pactl`).
        if let Some((sources, default_name)) = Self::enumerate_sources_via_libpulse()
            && !sources.is_empty()
        {
            return (sources, default_name);
        }

        // Fallback: `pactl` CLI when available.
        let default_name = Command::new("pactl")
            .arg("get-default-source")
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
            .filter(|s| !s.is_empty());

        let sources = Command::new("pactl")
            .args(["list", "sources"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| parse_pactl_sources(&String::from_utf8_lossy(&o.stdout)))
            .unwrap_or_default();

        (sources, default_name)
    }
}

impl AudioBackend for LinuxAudioBackend {
    fn list_input_devices(&self) -> Vec<AudioDeviceInfo> {
        let (sources, default_name) = Self::enumerate_sources();
        sources
            .into_iter()
            .enumerate()
            .map(|(index, (raw_name, desc))| AudioDeviceInfo {
                index,
                name: desc,
                is_default: default_name.as_deref() == Some(raw_name.as_str()),
            })
            .collect()
    }

    fn start_capture(
        &self,
        device_index: Option<usize>,
        on_block: AudioBlockCallback,
    ) -> Result<Box<dyn AudioStreamHandle>, PlatformError> {
        let api = PulseSimpleApi::load()?;
        let dev_name = device_index.and_then(|idx| {
            let (sources, _) = Self::enumerate_sources();
            sources.get(idx).map(|(raw, _)| raw.clone())
        });
        let dev_cstr = dev_name.and_then(|s| CString::new(s).ok());

        // Validate stream open synchronously so callers immediately get an error on failure.
        let ss = PaSampleSpec {
            format: PA_SAMPLE_FLOAT32LE,
            rate: TARGET_SAMPLE_RATE,
            channels: 1,
        };
        let frag_bytes = (BLOCK_SAMPLES * std::mem::size_of::<f32>()) as u32;
        let attr = PaBufferAttr {
            maxlength: u32::MAX,
            tlength: u32::MAX,
            prebuf: u32::MAX,
            minreq: u32::MAX,
            fragsize: frag_bytes,
        };
        let mut pa_err: c_int = 0;
        // SAFETY: Opening a PulseAudio / PipeWire-pulse recording stream.
        let raw_stream = unsafe {
            (api.pa_simple_new)(
                null(),
                c"Eqho".as_ptr(),
                PA_STREAM_RECORD,
                dev_cstr.as_ref().map_or(null(), |c| c.as_ptr()),
                c"Dictation Microphone".as_ptr(),
                &ss,
                null(),
                &attr,
                &mut pa_err,
            )
        };
        if raw_stream.is_null() {
            return Err(PlatformError(format!(
                "failed to open Linux microphone stream (pa error {pa_err})"
            )));
        }

        let stop = Arc::new(AtomicBool::new(false));
        let stop_bg = Arc::clone(&stop);
        let stream_addr = raw_stream as usize;

        let handle = thread::Builder::new()
            .name("eqho-audio-linux".to_owned())
            .spawn(move || {
                let pa = stream_addr as *mut c_void;
                let mut buf = vec![0.0_f32; BLOCK_SAMPLES];
                let byte_len = buf.len() * std::mem::size_of::<f32>();
                let mut err: c_int = 0;
                while !stop_bg.load(Ordering::Acquire) {
                    // SAFETY: `pa` is a live `pa_simple` handle and `buf` has `byte_len` bytes.
                    let rc =
                        unsafe { (api.pa_simple_read)(pa, buf.as_mut_ptr().cast(), byte_len, &mut err) };
                    if rc < 0 {
                        break;
                    }
                    on_block(&buf);
                }
                // SAFETY: Releasing the `pa_simple` stream owned by this thread.
                unsafe {
                    (api.pa_simple_free)(pa);
                }
            })
            .map_err(|e| {
                // SAFETY: Cleaning up opened stream because thread spawn failed.
                unsafe {
                    (api.pa_simple_free)(raw_stream);
                }
                PlatformError(format!("failed to spawn Linux audio capture thread: {e}"))
            })?;

        Ok(Box::new(LinuxAudioStream {
            stop,
            thread: Some(handle),
        }))
    }

    fn play_pcm(
        &self,
        samples: &[f32],
        sample_rate: u32,
        blocking: bool,
    ) -> Result<(), PlatformError> {
        if samples.is_empty() || sample_rate == 0 {
            return Ok(());
        }
        let owned = samples.to_vec();
        let play_fn = move || -> Result<(), PlatformError> {
            let api = PulseSimpleApi::load()?;
            let ss = PaSampleSpec {
                format: PA_SAMPLE_FLOAT32LE,
                rate: sample_rate as c_uint,
                channels: 1,
            };
            let mut err: c_int = 0;
            // SAFETY: Opening a short-lived playback stream for start/stop chime.
            unsafe {
                let pa = (api.pa_simple_new)(
                    null(),
                    c"Eqho".as_ptr(),
                    PA_STREAM_PLAYBACK,
                    null(),
                    c"Dictation Chime".as_ptr(),
                    &ss,
                    null(),
                    null(),
                    &mut err,
                );
                if pa.is_null() {
                    return Err(PlatformError(format!(
                        "failed to open Linux audio playback stream (pa error {err})"
                    )));
                }
                let bytes = owned.len() * std::mem::size_of::<f32>();
                let _ = (api.pa_simple_write)(pa, owned.as_ptr().cast(), bytes, &mut err);
                let _ = (api.pa_simple_drain)(pa, &mut err);
                (api.pa_simple_free)(pa);
            }
            Ok(())
        };

        if blocking {
            play_fn()
        } else {
            let _ = thread::Builder::new()
                .name("eqho-chime-linux".to_owned())
                .spawn(move || {
                    let _ = play_fn();
                });
            Ok(())
        }
    }
}
