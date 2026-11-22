//! WASAPI shared-mode microphone capture (16 kHz mono `f32`) and chime PCM playback.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use eqho_platform::{
    AudioBackend, AudioBlockCallback, AudioDeviceInfo, AudioStreamHandle, PlatformError,
};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Media::Audio::{
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, DEVICE_STATE_ACTIVE, IAudioCaptureClient,
    IAudioClient, IAudioRenderClient, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator,
    WAVEFORMATEX, WAVEFORMATEXTENSIBLE, eCapture, eConsole, eMultimedia, eRender,
};
use windows::Win32::Media::KernelStreaming::WAVE_FORMAT_EXTENSIBLE;
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance, CoTaskMemFree, STGM_READ};
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::core::{GUID, HRESULT, Interface, PWSTR};

use crate::com::{ComGuard, activate_mm_device};
use crate::pure::{downmix_f32_to_mono, downmix_i16_to_mono, linear_resample_mono};

const KSDATAFORMAT_SUBTYPE_IEEE_FLOAT: GUID =
    GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);
const VT_LPWSTR: u16 = 31;

const TARGET_SAMPLE_RATE: u32 = 16_000;
const BLOCK_SAMPLES: usize = 800; // 50 ms @ 16 kHz
const WASAPI_BUFFER_100NS: i64 = 1_000_000; // 100 ms
const WAVE_FORMAT_PCM_TAG: u16 = 1;
const WAVE_FORMAT_IEEE_FLOAT_TAG: u16 = 3;

/// WASAPI audio backend for microphone capture and chime playback.
#[derive(Debug, Default, Clone, Copy)]
pub struct WinAudioBackend;

/// Active WASAPI capture stream handle; dropping signals stop and joins the capture thread.
#[derive(Debug)]
pub struct WinAudioStreamHandle {
    stop: Arc<AtomicBool>,
    join_handle: Option<JoinHandle<()>>,
}

impl AudioStreamHandle for WinAudioStreamHandle {}

impl Drop for WinAudioStreamHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.join_handle.take() {
            let _ = handle.join();
        }
    }
}

struct CoTaskMemGuard<T>(*mut T);

impl<T> Drop for CoTaskMemGuard<T> {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: `self.0` was allocated by COM (`CoTaskMemAlloc`) and is freed once.
            unsafe { CoTaskMemFree(Some(self.0.cast::<c_void>())) };
        }
    }
}

#[repr(C)]
struct RawPropVariant {
    vt: u16,
    reserved: [u16; 3],
    val: *mut u16,
    extra: usize,
}

impl Default for RawPropVariant {
    fn default() -> Self {
        Self {
            vt: 0,
            reserved: [0; 3],
            val: std::ptr::null_mut(),
            extra: 0,
        }
    }
}

#[link(name = "ole32")]
unsafe extern "system" {
    fn PropVariantClear(pvar: *mut RawPropVariant) -> HRESULT;
}

struct PropVariantGuard(RawPropVariant);

impl Drop for PropVariantGuard {
    fn drop(&mut self) {
        // SAFETY: `self.0` was populated by `IPropertyStore::GetValue`.
        let _ = unsafe { PropVariantClear(&raw mut self.0) };
    }
}

fn property_store_get_value(
    store: &IPropertyStore,
    key: &PROPERTYKEY,
) -> windows::core::Result<RawPropVariant> {
    type GetValueFn = unsafe extern "system" fn(
        this: *mut c_void,
        key: *const PROPERTYKEY,
        pv: *mut RawPropVariant,
    ) -> HRESULT;

    let raw = store.as_raw();
    // SAFETY: `raw` is a valid `IPropertyStore` COM pointer.
    let vtbl = unsafe { *(raw.cast::<*const usize>()) };
    // SAFETY: Slot 5 of `IPropertyStore_Vtbl` (`IUnknown` 0..2, `GetCount` 3, `GetAt` 4) is `GetValue`.
    let get_value: GetValueFn = unsafe { std::mem::transmute(*vtbl.add(5)) };

    let mut pv = RawPropVariant::default();
    // SAFETY: `raw` is valid, `key` is valid, and `&raw mut pv` is a valid out-pointer.
    unsafe { get_value(raw, key, &raw mut pv) }.ok()?;
    Ok(pv)
}

#[derive(Debug, Clone, Copy)]
struct MixFormatInfo {
    sample_rate: u32,
    channels: usize,
    is_float: bool,
    bits_per_sample: u16,
}

fn inspect_mix_format(wf_ptr: *const WAVEFORMATEX) -> Result<MixFormatInfo, PlatformError> {
    if wf_ptr.is_null() {
        return Err(PlatformError("GetMixFormat returned null".to_owned()));
    }
    // SAFETY: `wf_ptr` is non-null and points to a valid `WAVEFORMATEX` from `GetMixFormat`.
    let wf = unsafe { *wf_ptr };
    let channels = usize::from(wf.nChannels);
    if channels == 0 || wf.nSamplesPerSec == 0 {
        return Err(PlatformError("invalid WASAPI mix format".to_owned()));
    }

    let format_tag = wf.wFormatTag;
    let bits_per_sample = wf.wBitsPerSample;
    let is_float = if format_tag == WAVE_FORMAT_IEEE_FLOAT_TAG {
        true
    } else if format_tag == WAVE_FORMAT_PCM_TAG {
        false
    } else if u32::from(format_tag) == WAVE_FORMAT_EXTENSIBLE {
        // SAFETY: `wFormatTag == WAVE_FORMAT_EXTENSIBLE` guarantees the struct is `WAVEFORMATEXTENSIBLE`.
        let ext = unsafe { &*(wf_ptr.cast::<WAVEFORMATEXTENSIBLE>()) };
        let sub: GUID = ext.SubFormat;
        sub == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT
    } else {
        return Err(PlatformError(format!(
            "unsupported WASAPI format tag: {format_tag}"
        )));
    };

    if (is_float && bits_per_sample != 32) || (!is_float && bits_per_sample != 16) {
        return Err(PlatformError(format!(
            "unsupported WASAPI bit depth: {bits_per_sample} (float={is_float})"
        )));
    }

    Ok(MixFormatInfo {
        sample_rate: wf.nSamplesPerSec,
        channels,
        is_float,
        bits_per_sample,
    })
}

fn read_pwstr_and_free(pwstr: PWSTR) -> Option<String> {
    let _guard = CoTaskMemGuard(pwstr.0);
    // SAFETY: `pwstr` is a NUL-terminated UTF-16 string allocated by COM.
    unsafe { pwstr.to_string() }.ok()
}

fn device_friendly_name(device: &IMMDevice) -> Option<String> {
    // SAFETY: `device` is a valid `IMMDevice` COM pointer.
    let store = unsafe { device.OpenPropertyStore(STGM_READ) }.ok()?;
    let pv = property_store_get_value(&store, &PKEY_Device_FriendlyName).ok()?;
    let guard = PropVariantGuard(pv);
    if guard.0.vt != VT_LPWSTR || guard.0.val.is_null() {
        return None;
    }
    let pwstr = PWSTR(guard.0.val);
    // SAFETY: `pwstr` is a valid NUL-terminated UTF-16 string owned by `guard`.
    unsafe { pwstr.to_string() }.ok()
}

impl AudioBackend for WinAudioBackend {
    fn list_input_devices(&self) -> Vec<AudioDeviceInfo> {
        let Ok(_com) = ComGuard::init() else {
            return Vec::new();
        };
        // SAFETY: COM is initialized on this thread.
        let Ok(enumerator) =
            (unsafe { CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL) })
        else {
            return Vec::new();
        };

        let default_id = unsafe { enumerator.GetDefaultAudioEndpoint(eCapture, eConsole) }
            .ok()
            .and_then(|dev| unsafe { dev.GetId() }.ok())
            .and_then(read_pwstr_and_free);

        // SAFETY: `enumerator` is valid; `DEVICE_STATE_ACTIVE` lists active capture endpoints.
        let Ok(collection) = (unsafe { enumerator.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE) })
        else {
            return Vec::new();
        };
        // SAFETY: `collection` is a valid `IMMDeviceCollection`.
        let count = unsafe { collection.GetCount() }.unwrap_or(0);

        let mut out = Vec::with_capacity(count as usize);
        for i in 0..count {
            // SAFETY: `i < count` on valid `IMMDeviceCollection`.
            let Ok(device) = (unsafe { collection.Item(i) }) else {
                continue;
            };
            let id = unsafe { device.GetId() }.ok().and_then(read_pwstr_and_free);
            let is_default = default_id.is_some() && id == default_id;
            let name = device_friendly_name(&device)
                .unwrap_or_else(|| format!("Microphone {}", i + 1));
            out.push(AudioDeviceInfo {
                index: i as usize,
                name,
                is_default,
            });
        }
        out
    }

    fn start_capture(
        &self,
        device_index: Option<usize>,
        on_block: AudioBlockCallback,
    ) -> Result<Box<dyn AudioStreamHandle>, PlatformError> {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = Arc::clone(&stop);
        let (tx, rx) = mpsc::sync_channel::<Result<(), PlatformError>>(1);

        let join_handle = thread::Builder::new()
            .name("eqho-wasapi-capture".to_owned())
            .spawn(move || {
                if let Err(err) = run_capture_loop(device_index, &stop_thread, &on_block, &tx) {
                    let _ = tx.send(Err(err));
                }
            })
            .map_err(|e| PlatformError(format!("failed to spawn capture thread: {e}")))?;

        rx.recv()
            .map_err(|_| PlatformError("capture thread exited before startup".to_owned()))??;

        Ok(Box::new(WinAudioStreamHandle {
            stop,
            join_handle: Some(join_handle),
        }))
    }

    fn play_pcm(&self, samples: &[f32], sample_rate: u32, blocking: bool) -> Result<(), PlatformError> {
        if samples.is_empty() || sample_rate == 0 {
            return Ok(());
        }
        if blocking {
            return play_pcm_blocking(samples, sample_rate);
        }
        let owned = samples.to_vec();
        thread::Builder::new()
            .name("eqho-wasapi-play".to_owned())
            .spawn(move || {
                let _ = play_pcm_blocking(&owned, sample_rate);
            })
            .map_err(|e| PlatformError(format!("failed to spawn playback thread: {e}")))?;
        Ok(())
    }
}

fn run_capture_loop(
    device_index: Option<usize>,
    stop: &AtomicBool,
    on_block: &AudioBlockCallback,
    ready_tx: &mpsc::SyncSender<Result<(), PlatformError>>,
) -> Result<(), PlatformError> {
    let _com = ComGuard::init().map_err(|e| PlatformError(format!("COM init failed: {e}")))?;
    // SAFETY: COM is initialized on this thread.
    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
            .map_err(|e| PlatformError(format!("MMDeviceEnumerator failed: {e}")))?;

    let device: IMMDevice = match device_index {
        Some(idx) => {
            // SAFETY: `enumerator` is valid.
            let collection = unsafe { enumerator.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE) }
                .map_err(|e| PlatformError(format!("EnumAudioEndpoints failed: {e}")))?;
            let idx_u32 = u32::try_from(idx)
                .map_err(|_| PlatformError(format!("invalid device index: {idx}")))?;
            // SAFETY: `collection` is valid.
            unsafe { collection.Item(idx_u32) }
                .map_err(|e| PlatformError(format!("capture device {idx} not found: {e}")))?
        }
        None => {
            // SAFETY: `enumerator` is valid.
            unsafe { enumerator.GetDefaultAudioEndpoint(eCapture, eConsole) }
                .map_err(|e| PlatformError(format!("GetDefaultAudioEndpoint failed: {e}")))?
        }
    };

    let audio_client: IAudioClient = activate_mm_device(&device, CLSCTX_ALL)
        .map_err(|e| PlatformError(format!("IAudioClient activate failed: {e}")))?;
    // SAFETY: `audio_client` is a valid `IAudioClient`.
    let mix_fmt_ptr = unsafe { audio_client.GetMixFormat() }
        .map_err(|e| PlatformError(format!("GetMixFormat failed: {e}")))?;
    let _mix_guard = CoTaskMemGuard(mix_fmt_ptr);
    let fmt = inspect_mix_format(mix_fmt_ptr)?;

    // SAFETY: `mix_fmt_ptr` is the exact format returned by `GetMixFormat`.
    unsafe {
        audio_client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            0,
            WASAPI_BUFFER_100NS,
            0,
            mix_fmt_ptr,
            None,
        )
    }
    .map_err(|e| PlatformError(format!("IAudioClient::Initialize failed: {e}")))?;

    // SAFETY: `audio_client` is initialized.
    let capture_client: IAudioCaptureClient = unsafe { audio_client.GetService() }
        .map_err(|e| PlatformError(format!("GetService<IAudioCaptureClient> failed: {e}")))?;
    // SAFETY: `audio_client` is initialized.
    unsafe { audio_client.Start() }
        .map_err(|e| PlatformError(format!("IAudioClient::Start failed: {e}")))?;

    let _ = ready_tx.send(Ok(()));

    let mut pending_16k: Vec<f32> = Vec::with_capacity(BLOCK_SAMPLES * 2);
    while !stop.load(Ordering::Relaxed) {
        // SAFETY: `capture_client` is active.
        let packet_frames = unsafe { capture_client.GetNextPacketSize() }.unwrap_or(0);
        if packet_frames == 0 {
            thread::sleep(Duration::from_millis(10));
            continue;
        }

        let mut data_ptr: *mut u8 = std::ptr::null_mut();
        let mut num_frames = 0u32;
        let mut flags = 0u32;
        // SAFETY: `capture_client` is active and out-pointers are valid stack variables.
        if unsafe {
            capture_client.GetBuffer(&mut data_ptr, &mut num_frames, &mut flags, None, None)
        }
        .is_err()
        {
            thread::sleep(Duration::from_millis(10));
            continue;
        }

        let mono = if num_frames == 0 {
            Vec::new()
        } else if (flags & u32::try_from(AUDCLNT_BUFFERFLAGS_SILENT.0).unwrap_or(2)) != 0
            || data_ptr.is_null()
        {
            vec![0.0f32; num_frames as usize]
        } else {
            let sample_count = (num_frames as usize) * fmt.channels;
            if fmt.is_float && fmt.bits_per_sample == 32 {
                // SAFETY: `data_ptr` holds `num_frames * channels` `f32` samples until `ReleaseBuffer`.
                let slice = unsafe { std::slice::from_raw_parts(data_ptr.cast::<f32>(), sample_count) };
                downmix_f32_to_mono(slice, fmt.channels)
            } else {
                // SAFETY: `data_ptr` holds `num_frames * channels` `i16` samples until `ReleaseBuffer`.
                let slice = unsafe { std::slice::from_raw_parts(data_ptr.cast::<i16>(), sample_count) };
                downmix_i16_to_mono(slice, fmt.channels)
            }
        };

        // SAFETY: Matches the preceding `GetBuffer` call with `num_frames`.
        let _ = unsafe { capture_client.ReleaseBuffer(num_frames) };

        if !mono.is_empty() {
            let resampled = linear_resample_mono(&mono, fmt.sample_rate, TARGET_SAMPLE_RATE);
            pending_16k.extend_from_slice(&resampled);
            while pending_16k.len() >= BLOCK_SAMPLES {
                let block: Vec<f32> = pending_16k.drain(..BLOCK_SAMPLES).collect();
                on_block(&block);
            }
        }
    }

    // SAFETY: `audio_client` was started above.
    let _ = unsafe { audio_client.Stop() };
    drop(capture_client);
    drop(audio_client);
    drop(device);
    drop(enumerator);
    Ok(())
}

fn play_pcm_blocking(samples: &[f32], sample_rate: u32) -> Result<(), PlatformError> {
    let _com = ComGuard::init().map_err(|e| PlatformError(format!("COM init failed: {e}")))?;
    // SAFETY: COM is initialized on this thread.
    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
            .map_err(|e| PlatformError(format!("MMDeviceEnumerator failed: {e}")))?;
    // SAFETY: `enumerator` is valid.
    let device = unsafe { enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia) }
        .map_err(|e| PlatformError(format!("GetDefaultAudioEndpoint(eRender) failed: {e}")))?;
    let audio_client: IAudioClient = activate_mm_device(&device, CLSCTX_ALL)
        .map_err(|e| PlatformError(format!("IAudioClient activate failed: {e}")))?;
    // SAFETY: `audio_client` is valid.
    let mix_fmt_ptr = unsafe { audio_client.GetMixFormat() }
        .map_err(|e| PlatformError(format!("GetMixFormat failed: {e}")))?;
    let _mix_guard = CoTaskMemGuard(mix_fmt_ptr);
    let fmt = inspect_mix_format(mix_fmt_ptr)?;

    let resampled = linear_resample_mono(samples, sample_rate, fmt.sample_rate);
    if resampled.is_empty() {
        return Ok(());
    }

    // SAFETY: `mix_fmt_ptr` is the format returned by `GetMixFormat`.
    unsafe {
        audio_client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            0,
            WASAPI_BUFFER_100NS,
            0,
            mix_fmt_ptr,
            None,
        )
    }
    .map_err(|e| PlatformError(format!("IAudioClient::Initialize failed: {e}")))?;

    // SAFETY: `audio_client` is initialized.
    let buffer_frames = unsafe { audio_client.GetBufferSize() }
        .map_err(|e| PlatformError(format!("GetBufferSize failed: {e}")))?;
    // SAFETY: `audio_client` is initialized.
    let render_client: IAudioRenderClient = unsafe { audio_client.GetService() }
        .map_err(|e| PlatformError(format!("GetService<IAudioRenderClient> failed: {e}")))?;
    // SAFETY: `audio_client` is initialized.
    unsafe { audio_client.Start() }
        .map_err(|e| PlatformError(format!("IAudioClient::Start failed: {e}")))?;

    let mut offset = 0usize;
    while offset < resampled.len() {
        // SAFETY: `audio_client` is active.
        let padding = unsafe { audio_client.GetCurrentPadding() }.unwrap_or(buffer_frames);
        let available = buffer_frames.saturating_sub(padding) as usize;
        if available == 0 {
            thread::sleep(Duration::from_millis(10));
            continue;
        }
        let to_write = available.min(resampled.len() - offset);
        let to_write_u32 = u32::try_from(to_write).unwrap_or(u32::MAX);
        // SAFETY: `to_write_u32 <= available` frames in the render buffer.
        let Ok(ptr) = (unsafe { render_client.GetBuffer(to_write_u32) }) else {
            thread::sleep(Duration::from_millis(10));
            continue;
        };

        if fmt.is_float && fmt.bits_per_sample == 32 {
            // SAFETY: `ptr` has space for `to_write * fmt.channels` `f32` samples.
            let dst = unsafe {
                std::slice::from_raw_parts_mut(ptr.cast::<f32>(), to_write * fmt.channels)
            };
            for (frame_idx, &sample) in resampled[offset..offset + to_write].iter().enumerate() {
                let base = frame_idx * fmt.channels;
                for ch in 0..fmt.channels {
                    dst[base + ch] = sample;
                }
            }
        } else {
            // SAFETY: `ptr` has space for `to_write * fmt.channels` `i16` samples.
            let dst = unsafe {
                std::slice::from_raw_parts_mut(ptr.cast::<i16>(), to_write * fmt.channels)
            };
            for (frame_idx, &sample) in resampled[offset..offset + to_write].iter().enumerate() {
                let quantized = (sample.clamp(-1.0, 1.0) * 32767.0) as i16;
                let base = frame_idx * fmt.channels;
                for ch in 0..fmt.channels {
                    dst[base + ch] = quantized;
                }
            }
        }

        // SAFETY: Matches the preceding `GetBuffer(to_write_u32)`.
        let _ = unsafe { render_client.ReleaseBuffer(to_write_u32, 0) };
        offset += to_write;
    }

    let drain_deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < drain_deadline {
        // SAFETY: `audio_client` is active.
        let padding = unsafe { audio_client.GetCurrentPadding() }.unwrap_or(0);
        if padding == 0 {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }

    // SAFETY: `audio_client` was started above.
    let _ = unsafe { audio_client.Stop() };
    drop(render_client);
    drop(audio_client);
    drop(device);
    drop(enumerator);
    Ok(())
}
