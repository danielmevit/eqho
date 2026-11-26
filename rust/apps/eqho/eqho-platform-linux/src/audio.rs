//! Linux 16 kHz mono float32 microphone capture and chime playback via dynamic `libpulse-simple.so.0`.
//!
//! Works transparently on both PipeWire (`pipewire-pulse`) and PulseAudio with zero build-time
//! C library dependencies and automatic hardware resampling straight to `16000 Hz mono f32`.

use std::ffi::{CString, c_char, c_int, c_uint, c_void};
use std::process::{Command, Stdio};
use std::ptr::null;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};

use eqho_platform::{
    AudioBackend, AudioBlockCallback, AudioDeviceInfo, AudioStreamHandle, PlatformError,
};
use libloading::Library;

use crate::pure::parse_pactl_sources;

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

/// PipeWire / PulseAudio audio capture & playback backend.
#[derive(Debug, Default, Clone, Copy)]
pub struct LinuxAudioBackend;

impl LinuxAudioBackend {
    fn enumerate_sources() -> (Vec<(String, String)>, Option<String>) {
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
