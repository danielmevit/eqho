//! macOS CoreAudio (`AudioToolbox` AudioQueue) 16 kHz mono float32 capture and chime playback.
//!
//! Uses `AudioQueueNewInput` directly at `16000 Hz mono float32 LPCM` so CoreAudio handles
//! studio-grade hardware resampling cleanly with zero external audio dependencies.

use std::ffi::{CStr, c_char, c_void};
use std::ptr::{null, null_mut};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use eqho_platform::{
    AudioBackend, AudioBlockCallback, AudioDeviceInfo, AudioStreamHandle, PlatformError,
};

const TARGET_SAMPLE_RATE: f64 = 16_000.0;
const BLOCK_SAMPLES: u32 = 800; // 50 ms at 16 kHz
const NUM_BUFFERS: usize = 3;

const K_AUDIO_FORMAT_LINEAR_PCM: u32 = u32::from_be_bytes(*b"lpcm");
const K_AUDIO_FORMAT_FLAG_IS_FLOAT: u32 = 1 << 0;
const K_AUDIO_FORMAT_FLAG_IS_PACKED: u32 = 1 << 3;

const K_AUDIO_OBJECT_SYSTEM_OBJECT: u32 = 1;
const K_AUDIO_HARDWARE_PROPERTY_DEVICES: u32 = u32::from_be_bytes(*b"dev#");
const K_AUDIO_HARDWARE_PROPERTY_DEFAULT_INPUT_DEVICE: u32 = u32::from_be_bytes(*b"dIn ");
const K_AUDIO_DEVICE_PROPERTY_DEVICE_NAME_CF_STRING: u32 = u32::from_be_bytes(*b"lnam");
const K_AUDIO_DEVICE_PROPERTY_DEVICE_UID: u32 = u32::from_be_bytes(*b"uid ");
const K_AUDIO_DEVICE_PROPERTY_STREAMS: u32 = u32::from_be_bytes(*b"stm#");
const K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL: u32 = u32::from_be_bytes(*b"glob");
const K_AUDIO_OBJECT_PROPERTY_SCOPE_INPUT: u32 = u32::from_be_bytes(*b"inpt");
const K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN: u32 = 0;
const K_AUDIO_QUEUE_PROPERTY_CURRENT_DEVICE: u32 = u32::from_be_bytes(*b"aqcd");
const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;

#[repr(C)]
struct AudioObjectPropertyAddress {
    selector: u32,
    scope: u32,
    element: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct AudioStreamBasicDescription {
    sample_rate: f64,
    format_id: u32,
    format_flags: u32,
    bytes_per_packet: u32,
    frames_per_packet: u32,
    bytes_per_frame: u32,
    channels_per_frame: u32,
    bits_per_channel: u32,
    reserved: u32,
}

#[repr(C)]
struct AudioQueueBuffer {
    audio_data_bytes_capacity: u32,
    audio_data: *mut c_void,
    audio_data_byte_size: u32,
    user_data: *mut c_void,
    packet_description_capacity: u32,
    packet_descriptions: *mut c_void,
    packet_description_count: u32,
}

type AudioQueueInputCallback = unsafe extern "C" fn(
    user_data: *mut c_void,
    aq: *mut c_void,
    buffer: *mut AudioQueueBuffer,
    start_time: *const c_void,
    num_packet_descs: u32,
    packet_descs: *const c_void,
);

type AudioQueueOutputCallback =
    unsafe extern "C" fn(user_data: *mut c_void, aq: *mut c_void, buffer: *mut AudioQueueBuffer);

#[link(name = "CoreAudio", kind = "framework")]
unsafe extern "C" {
    fn AudioObjectGetPropertyDataSize(
        object_id: u32,
        address: *const AudioObjectPropertyAddress,
        qualifier_data_size: u32,
        qualifier_data: *const c_void,
        out_data_size: *mut u32,
    ) -> i32;
    fn AudioObjectGetPropertyData(
        object_id: u32,
        address: *const AudioObjectPropertyAddress,
        qualifier_data_size: u32,
        qualifier_data: *const c_void,
        data_size: *mut u32,
        data: *mut c_void,
    ) -> i32;
}

#[link(name = "AudioToolbox", kind = "framework")]
unsafe extern "C" {
    fn AudioQueueNewInput(
        format: *const AudioStreamBasicDescription,
        callback: AudioQueueInputCallback,
        user_data: *mut c_void,
        callback_run_loop: *mut c_void,
        callback_run_loop_mode: *const c_void,
        flags: u32,
        out_aq: *mut *mut c_void,
    ) -> i32;
    fn AudioQueueNewOutput(
        format: *const AudioStreamBasicDescription,
        callback: AudioQueueOutputCallback,
        user_data: *mut c_void,
        callback_run_loop: *mut c_void,
        callback_run_loop_mode: *const c_void,
        flags: u32,
        out_aq: *mut *mut c_void,
    ) -> i32;
    fn AudioQueueSetProperty(
        aq: *mut c_void,
        id: u32,
        data: *const c_void,
        data_size: u32,
    ) -> i32;
    fn AudioQueueAllocateBuffer(
        aq: *mut c_void,
        buffer_byte_size: u32,
        out_buffer: *mut *mut AudioQueueBuffer,
    ) -> i32;
    fn AudioQueueEnqueueBuffer(
        aq: *mut c_void,
        buffer: *mut AudioQueueBuffer,
        num_packet_descs: u32,
        packet_descs: *const c_void,
    ) -> i32;
    fn AudioQueueStart(aq: *mut c_void, start_time: *const c_void) -> i32;
    fn AudioQueueStop(aq: *mut c_void, immediate: u8) -> i32;
    fn AudioQueueDispose(aq: *mut c_void, immediate: u8) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFStringGetCString(
        the_string: *const c_void,
        buffer: *mut c_char,
        buffer_size: isize,
        encoding: u32,
    ) -> u8;
    fn CFStringCreateWithCString(
        alloc: *const c_void,
        c_str: *const c_char,
        encoding: u32,
    ) -> *const c_void;
    fn CFRelease(cf: *const c_void);
}

struct InputCallbackState {
    stop: Arc<AtomicBool>,
    on_block: AudioBlockCallback,
}

unsafe extern "C" fn input_callback(
    user_data: *mut c_void,
    aq: *mut c_void,
    buffer: *mut AudioQueueBuffer,
    _start_time: *const c_void,
    _num_packet_descs: u32,
    _packet_descs: *const c_void,
) {
    if user_data.is_null() || buffer.is_null() {
        return;
    }
    // SAFETY: `user_data` points to live `InputCallbackState` until `AudioQueueDispose` completes.
    let state = unsafe { &*(user_data as *const InputCallbackState) };
    if state.stop.load(Ordering::Acquire) {
        return;
    }
    // SAFETY: `audio_data` contains `audio_data_byte_size / 4` `f32` PCM samples.
    let samples = unsafe {
        let buf = &*buffer;
        let count = (buf.audio_data_byte_size as usize) / std::mem::size_of::<f32>();
        if count > 0 && !buf.audio_data.is_null() {
            std::slice::from_raw_parts(buf.audio_data.cast::<f32>(), count)
        } else {
            &[]
        }
    };
    if !samples.is_empty() {
        (state.on_block)(samples);
    }
    if !state.stop.load(Ordering::Acquire) {
        // SAFETY: Re-enqueuing the filled input buffer onto the active AudioQueue.
        unsafe {
            let _ = AudioQueueEnqueueBuffer(aq, buffer, 0, null());
        }
    }
}

unsafe extern "C" fn output_done_callback(
    user_data: *mut c_void,
    _aq: *mut c_void,
    _buffer: *mut AudioQueueBuffer,
) {
    if !user_data.is_null() {
        // SAFETY: `user_data` points to an `AtomicBool` alive until playback finishes.
        let done = unsafe { &*(user_data as *const AtomicBool) };
        done.store(true, Ordering::Release);
    }
}

struct MacAudioStream {
    stop: Arc<AtomicBool>,
    queue_raw: usize,
    _state_ptr: usize,
}

impl AudioStreamHandle for MacAudioStream {}

impl Drop for MacAudioStream {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if self.queue_raw != 0 {
            let aq = self.queue_raw as *mut c_void;
            // SAFETY: Synchronous `AudioQueueDispose(aq, 1)` guarantees all callbacks have ended.
            unsafe {
                let _ = AudioQueueStop(aq, 1);
                let _ = AudioQueueDispose(aq, 1);
            }
            self.queue_raw = 0;
        }
        if self._state_ptr != 0 {
            // SAFETY: Reclaiming the `Box<InputCallbackState>` after queue disposal.
            unsafe {
                drop(Box::from_raw(self._state_ptr as *mut InputCallbackState));
            }
            self._state_ptr = 0;
        }
    }
}

/// CoreAudio AudioQueue microphone capture and chime playback backend.
#[derive(Debug, Default, Clone, Copy)]
pub struct MacAudioBackend;

impl MacAudioBackend {
    fn cf_string_prop(device_id: u32, selector: u32) -> Option<String> {
        let addr = AudioObjectPropertyAddress {
            selector,
            scope: K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL,
            element: K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN,
        };
        let mut cf_ref: *const c_void = null();
        let mut size = std::mem::size_of::<*const c_void>() as u32;
        // SAFETY: Reading a CoreAudio CFStringRef property and releasing it after copying UTF-8.
        unsafe {
            if AudioObjectGetPropertyData(
                device_id,
                &addr,
                0,
                null(),
                &mut size,
                (&mut cf_ref as *mut *const c_void).cast(),
            ) != 0
                || cf_ref.is_null()
            {
                return None;
            }
            let mut buf = [0 as c_char; 256];
            let ok = CFStringGetCString(
                cf_ref,
                buf.as_mut_ptr(),
                buf.len() as isize,
                K_CF_STRING_ENCODING_UTF8,
            );
            CFRelease(cf_ref);
            if ok == 0 {
                return None;
            }
            Some(CStr::from_ptr(buf.as_ptr()).to_string_lossy().trim().to_owned())
        }
    }

    fn device_has_input_streams(device_id: u32) -> bool {
        let addr = AudioObjectPropertyAddress {
            selector: K_AUDIO_DEVICE_PROPERTY_STREAMS,
            scope: K_AUDIO_OBJECT_PROPERTY_SCOPE_INPUT,
            element: K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN,
        };
        let mut size: u32 = 0;
        // SAFETY: Checking byte size of input stream list on `device_id`.
        unsafe {
            AudioObjectGetPropertyDataSize(device_id, &addr, 0, null(), &mut size) == 0 && size > 0
        }
    }

    fn enumerate_input_devices() -> Vec<(u32, String, String, bool)> {
        let addr = AudioObjectPropertyAddress {
            selector: K_AUDIO_HARDWARE_PROPERTY_DEVICES,
            scope: K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL,
            element: K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN,
        };
        let def_addr = AudioObjectPropertyAddress {
            selector: K_AUDIO_HARDWARE_PROPERTY_DEFAULT_INPUT_DEVICE,
            scope: K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL,
            element: K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN,
        };
        // SAFETY: Enumerating CoreAudio devices and filtering those with input streams.
        unsafe {
            let mut default_id: u32 = 0;
            let mut def_size = std::mem::size_of::<u32>() as u32;
            let _ = AudioObjectGetPropertyData(
                K_AUDIO_OBJECT_SYSTEM_OBJECT,
                &def_addr,
                0,
                null(),
                &mut def_size,
                (&mut default_id as *mut u32).cast(),
            );

            let mut bytes: u32 = 0;
            if AudioObjectGetPropertyDataSize(
                K_AUDIO_OBJECT_SYSTEM_OBJECT,
                &addr,
                0,
                null(),
                &mut bytes,
            ) != 0
                || bytes == 0
            {
                return Vec::new();
            }
            let count = (bytes as usize) / std::mem::size_of::<u32>();
            let mut ids = vec![0u32; count];
            if AudioObjectGetPropertyData(
                K_AUDIO_OBJECT_SYSTEM_OBJECT,
                &addr,
                0,
                null(),
                &mut bytes,
                ids.as_mut_ptr().cast(),
            ) != 0
            {
                return Vec::new();
            }
            let mut out = Vec::new();
            for id in ids {
                if !Self::device_has_input_streams(id) {
                    continue;
                }
                let name = Self::cf_string_prop(id, K_AUDIO_DEVICE_PROPERTY_DEVICE_NAME_CF_STRING)
                    .unwrap_or_else(|| format!("Microphone {id}"));
                let uid = Self::cf_string_prop(id, K_AUDIO_DEVICE_PROPERTY_DEVICE_UID)
                    .unwrap_or_default();
                out.push((id, name, uid, id == default_id));
            }
            out
        }
    }
}

impl AudioBackend for MacAudioBackend {
    fn list_input_devices(&self) -> Vec<AudioDeviceInfo> {
        Self::enumerate_input_devices()
            .into_iter()
            .enumerate()
            .map(|(index, (_, name, _, is_default))| AudioDeviceInfo {
                index,
                name,
                is_default,
            })
            .collect()
    }

    fn start_capture(
        &self,
        device_index: Option<usize>,
        on_block: AudioBlockCallback,
    ) -> Result<Box<dyn AudioStreamHandle>, PlatformError> {
        let selected_uid = device_index.and_then(|idx| {
            Self::enumerate_input_devices()
                .get(idx)
                .map(|(_, _, uid, _)| uid.clone())
                .filter(|u| !u.is_empty())
        });

        let desc = AudioStreamBasicDescription {
            sample_rate: TARGET_SAMPLE_RATE,
            format_id: K_AUDIO_FORMAT_LINEAR_PCM,
            format_flags: K_AUDIO_FORMAT_FLAG_IS_FLOAT | K_AUDIO_FORMAT_FLAG_IS_PACKED,
            bytes_per_packet: 4,
            frames_per_packet: 1,
            bytes_per_frame: 4,
            channels_per_frame: 1,
            bits_per_channel: 32,
            reserved: 0,
        };

        let stop = Arc::new(AtomicBool::new(false));
        let state = Box::new(InputCallbackState {
            stop: Arc::clone(&stop),
            on_block,
        });
        let state_ptr = Box::into_raw(state);
        let mut aq: *mut c_void = null_mut();

        // SAFETY: Creating, configuring, allocating buffers, and starting an AudioQueue input stream.
        unsafe {
            let status = AudioQueueNewInput(
                &desc,
                input_callback,
                state_ptr.cast(),
                null_mut(),
                null(),
                0,
                &mut aq,
            );
            if status != 0 || aq.is_null() {
                drop(Box::from_raw(state_ptr));
                return Err(PlatformError(format!(
                    "AudioQueueNewInput failed (OSStatus {status})"
                )));
            }

            if let Some(uid) = selected_uid
                && let Ok(c_uid) = std::ffi::CString::new(uid)
            {
                let cf_uid = CFStringCreateWithCString(
                    null(),
                    c_uid.as_ptr(),
                    K_CF_STRING_ENCODING_UTF8,
                );
                if !cf_uid.is_null() {
                    let _ = AudioQueueSetProperty(
                        aq,
                        K_AUDIO_QUEUE_PROPERTY_CURRENT_DEVICE,
                        (&cf_uid as *const *const c_void).cast(),
                        std::mem::size_of::<*const c_void>() as u32,
                    );
                    CFRelease(cf_uid);
                }
            }

            let buf_bytes = BLOCK_SAMPLES * 4;
            for _ in 0..NUM_BUFFERS {
                let mut buf: *mut AudioQueueBuffer = null_mut();
                let rc = AudioQueueAllocateBuffer(aq, buf_bytes, &mut buf);
                if rc != 0 || buf.is_null() {
                    let _ = AudioQueueDispose(aq, 1);
                    drop(Box::from_raw(state_ptr));
                    return Err(PlatformError(format!(
                        "AudioQueueAllocateBuffer failed (OSStatus {rc})"
                    )));
                }
                let _ = AudioQueueEnqueueBuffer(aq, buf, 0, null());
            }

            let start_rc = AudioQueueStart(aq, null());
            if start_rc != 0 {
                let _ = AudioQueueDispose(aq, 1);
                drop(Box::from_raw(state_ptr));
                return Err(PlatformError(format!(
                    "AudioQueueStart failed (OSStatus {start_rc})"
                )));
            }
        }

        Ok(Box::new(MacAudioStream {
            stop,
            queue_raw: aq as usize,
            _state_ptr: state_ptr as usize,
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
            let desc = AudioStreamBasicDescription {
                sample_rate: f64::from(sample_rate),
                format_id: K_AUDIO_FORMAT_LINEAR_PCM,
                format_flags: K_AUDIO_FORMAT_FLAG_IS_FLOAT | K_AUDIO_FORMAT_FLAG_IS_PACKED,
                bytes_per_packet: 4,
                frames_per_packet: 1,
                bytes_per_frame: 4,
                channels_per_frame: 1,
                bits_per_channel: 32,
                reserved: 0,
            };
            let done = Box::new(AtomicBool::new(false));
            let done_ptr = Box::into_raw(done);
            let mut aq: *mut c_void = null_mut();
            let byte_len = (owned.len() * std::mem::size_of::<f32>()) as u32;

            // SAFETY: Short-lived AudioQueue output stream for chime playback.
            unsafe {
                let status = AudioQueueNewOutput(
                    &desc,
                    output_done_callback,
                    done_ptr.cast(),
                    null_mut(),
                    null(),
                    0,
                    &mut aq,
                );
                if status != 0 || aq.is_null() {
                    drop(Box::from_raw(done_ptr));
                    return Err(PlatformError(format!(
                        "AudioQueueNewOutput failed (OSStatus {status})"
                    )));
                }
                let mut buf: *mut AudioQueueBuffer = null_mut();
                if AudioQueueAllocateBuffer(aq, byte_len, &mut buf) != 0 || buf.is_null() {
                    let _ = AudioQueueDispose(aq, 1);
                    drop(Box::from_raw(done_ptr));
                    return Err(PlatformError("AudioQueueAllocateBuffer failed".to_owned()));
                }
                std::ptr::copy_nonoverlapping(
                    owned.as_ptr().cast::<u8>(),
                    (*buf).audio_data.cast::<u8>(),
                    byte_len as usize,
                );
                (*buf).audio_data_byte_size = byte_len;
                let _ = AudioQueueEnqueueBuffer(aq, buf, 0, null());
                let _ = AudioQueueStart(aq, null());

                let timeout_ms = ((owned.len() as u64 * 1000) / u64::from(sample_rate)) + 400;
                let mut waited = 0u64;
                while !(*done_ptr).load(Ordering::Acquire) && waited < timeout_ms {
                    thread::sleep(Duration::from_millis(10));
                    waited += 10;
                }
                let _ = AudioQueueStop(aq, 1);
                let _ = AudioQueueDispose(aq, 1);
                drop(Box::from_raw(done_ptr));
            }
            Ok(())
        };

        if blocking {
            play_fn()
        } else {
            let _: Option<JoinHandle<()>> = thread::Builder::new()
                .name("eqho-chime-macos".to_owned())
                .spawn(move || {
                    let _ = play_fn();
                })
                .ok();
            Ok(())
        }
    }
}
