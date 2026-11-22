//! Pure-Rust dictation coordinator with session-ID fencing, volume ducking,
//! verified focus restoration, `EQ-MODS` / `EQ-CLIP` injection, and recovery.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use eqho_core::chime::{CHIME_SAMPLE_RATE, start_chime, stop_chime};
use eqho_core::history::TranscriptHistory;
use eqho_core::modelpick::{HardwareClass, recommend_model};
use eqho_core::modelstore::{
    LOW_MEMORY_LOAD_TEXT, low_memory_warning, model_info, required_memory_mb,
};
use eqho_core::recovery::LastDictationBuffer;
use eqho_core::settings::{Settings, is_english_only_model, volume_duck_level};
use eqho_core::textproc::{
    apply_replacements, clean_text, is_delete_command, match_command, smart_join,
};
use eqho_core::vad::{
    BlockAssembler, CHUNK_SAMPLES, NEAR_SILENCE_FACTOR, PARTIAL_MAX_INFER_S, PhraseSegmenter,
    SAMPLE_RATE, Segment, VadEvent, filter_segments, level_meter, voice_features,
};
use eqho_host::{ModelHostError, ModelInfo};
use eqho_platform::{
    AudioBlockCallback, AudioStreamHandle, HotkeyEvent, HotkeyMode, HotkeyRegistration,
    HotkeySpec, WindowHandle,
};

use crate::traits::{EqhoPlatform, SpeechBackend, UiEventSink};
use crate::wav_source::TestAudioStream;

const AUDIO_QUEUE_CAPACITY: usize = 64;
const CLIPBOARD_RESTORE_DELAY: Duration = Duration::from_millis(200);
const INJECT_SETTLE_DELAY: Duration = Duration::from_millis(20);
const DISCRETE_GPU_MIN_VRAM_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_BACKSPACE_CHARS: usize = 400;
const SLOW_MODEL_HINT_SECS: f64 = 6.0;

const STILL_WRITING_TEXT: &str = "Still writing your last dictation — one moment.";
const WRITING_TEXT: &str = "Writing it down…";
const FOCUS_RESTORE_FAILED_TEXT: &str = "Couldn't return focus to your target window — your dictation is saved in Eqho (Copy Last Dictation).";
const MIC_PRIVACY_BLOCKED_TEXT: &str = "Windows is blocking the microphone. Turn on microphone access for desktop apps — the settings page is opening now.";
const MIC_UNAVAILABLE_TEXT: &str =
    "No microphone found. Plug one in, or choose another one in Eqho → General → Microphone.";

#[derive(Debug)]
struct CoordinatorState {
    next_session_id: u64,
    active_session: u64,
    running: bool,
    finishing: bool,
    target_hwnd: WindowHandle,
    pending_text: Vec<String>,
    saved_volume: Option<f32>,
    last_injected: String,
    dictation_start: Option<Instant>,
    mic_settings_opened: bool,
    last_infer_s: Option<f64>,
    recovery: LastDictationBuffer,
}

impl Default for CoordinatorState {
    fn default() -> Self {
        Self {
            next_session_id: 0,
            active_session: 0,
            running: false,
            finishing: false,
            target_hwnd: WindowHandle::NONE,
            pending_text: Vec::new(),
            saved_volume: None,
            last_injected: String::new(),
            dictation_start: None,
            mic_settings_opened: false,
            last_infer_s: None,
            recovery: LastDictationBuffer::default(),
        }
    }
}

struct CoordinatorInner {
    session_lock: Mutex<()>,
    state: Mutex<CoordinatorState>,
    settings: Mutex<Settings>,
    settings_path: PathBuf,
    history: TranscriptHistory,
    platform: Arc<dyn EqhoPlatform>,
    speech: Mutex<Box<dyn SpeechBackend>>,
    ui: Arc<dyn UiEventSink>,
    stream: Mutex<Option<Box<dyn AudioStreamHandle>>>,
    assembler: Mutex<Option<Arc<Mutex<BlockAssembler>>>>,
    audio_tx: Mutex<Option<mpsc::SyncSender<Vec<f32>>>>,
    stop_flag: Mutex<Option<Arc<AtomicBool>>>,
    worker_handle: Mutex<Option<JoinHandle<()>>>,
    finish_handle: Mutex<Option<JoinHandle<()>>>,
    hotkey_reg: Mutex<Option<Box<dyn HotkeyRegistration>>>,
    dictation_suspended: AtomicBool,
}

/// Pure-Rust dictation coordinator orchestrating audio capture, VAD,
/// out-of-process speech inference, volume ducking, and verified text injection.
#[derive(Clone)]
pub struct DictationCoordinator {
    inner: Arc<CoordinatorInner>,
}

impl std::fmt::Debug for DictationCoordinator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (active_session, running, finishing) = {
            let st = lock_unpoisoned(&self.inner.state);
            (st.active_session, st.running, st.finishing)
        };
        f.debug_struct("DictationCoordinator")
            .field("active_session", &active_session)
            .field("running", &running)
            .field("finishing", &finishing)
            .field("settings_path", &self.inner.settings_path)
            .finish()
    }
}

impl DictationCoordinator {
    pub fn new(
        settings: Settings,
        settings_path: PathBuf,
        history: TranscriptHistory,
        platform: Arc<dyn EqhoPlatform>,
        speech: Box<dyn SpeechBackend>,
        ui: Arc<dyn UiEventSink>,
    ) -> Self {
        Self {
            inner: Arc::new(CoordinatorInner {
                session_lock: Mutex::new(()),
                state: Mutex::new(CoordinatorState::default()),
                settings: Mutex::new(settings),
                settings_path,
                history,
                platform,
                speech: Mutex::new(speech),
                ui,
                stream: Mutex::new(None),
                assembler: Mutex::new(None),
                audio_tx: Mutex::new(None),
                stop_flag: Mutex::new(None),
                worker_handle: Mutex::new(None),
                finish_handle: Mutex::new(None),
                hotkey_reg: Mutex::new(None),
                dictation_suspended: AtomicBool::new(false),
            }),
        }
    }

    pub fn is_running(&self) -> bool {
        lock_unpoisoned(&self.inner.state).running
    }

    pub fn is_finishing(&self) -> bool {
        lock_unpoisoned(&self.inner.state).finishing
    }

    pub fn is_dictation_suspended(&self) -> bool {
        self.inner.dictation_suspended.load(Ordering::Acquire)
    }

    /// Quietly suspend or resume dictation (used by Store trial expiry / purchase).
    ///
    /// When `suspended` is `true`, unregisters the global hotkey and quietly ends any active
    /// capture without nagging. When `false`, re-registers the global hotkey immediately.
    pub fn set_dictation_suspended(&self, suspended: bool) {
        let was_suspended = self
            .inner
            .dictation_suspended
            .swap(suspended, Ordering::AcqRel);
        if suspended {
            self.inner.unregister_hotkey();
            let _guard = lock_unpoisoned(&self.inner.session_lock);
            self.inner.teardown_session_locked(None, None, true);
        } else if was_suspended {
            let _ = self.inner.register_hotkey();
        }
    }

    pub fn activate(&self) {
        self.inner.activate();
    }

    pub fn deactivate(&self) {
        self.inner.deactivate();
    }

    pub fn toggle(&self) {
        self.inner.toggle();
    }

    pub fn register_hotkey(&self) -> Result<(), String> {
        self.inner.register_hotkey()
    }

    pub fn unregister_hotkey(&self) {
        self.inner.unregister_hotkey();
    }

    pub fn preload_model(&self) -> Result<ModelInfo, ModelHostError> {
        self.inner.preload_model()
    }

    pub fn apply_settings(&self, new_settings: Settings, reload_model: bool) {
        self.inner.apply_settings(new_settings, reload_model);
    }

    pub fn last_dictation(&self) -> Option<String> {
        lock_unpoisoned(&self.inner.state)
            .recovery
            .get()
            .map(str::to_owned)
    }

    pub fn copy_last_dictation(&self) -> Result<bool, String> {
        let text = self.last_dictation();
        match text {
            Some(t) if !t.is_empty() => {
                self.inner
                    .platform
                    .set_text(&t, false)
                    .map_err(|e| e.to_string())?;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    pub fn shutdown(&self) {
        self.inner.shutdown();
    }
}

impl CoordinatorInner {
    fn activate(self: &Arc<Self>) {
        if self.dictation_suspended.load(Ordering::Acquire) {
            return;
        }
        let _guard = lock_unpoisoned(&self.session_lock);

        let settings = lock_unpoisoned(&self.settings).clone();
        let use_gpu = has_discrete_gpu(self.platform.as_ref(), &settings.engine_backend);

        let model_ready = match self.speech.try_lock() {
            Ok(mut backend) => backend.is_ready(&settings.model_size, use_gpu),
            Err(_) => false,
        };

        let sid = {
            let mut st = lock_unpoisoned(&self.state);
            if st.finishing {
                drop(st);
                self.ui.update_overlay_text(STILL_WRITING_TEXT);
                self.ui.notify(STILL_WRITING_TEXT);
                return;
            }
            if st.running {
                return;
            }

            st.target_hwnd = self.platform.get_foreground_window();
            st.pending_text.clear();
            st.next_session_id += 1;
            let sid = st.next_session_id;
            st.active_session = sid;
            st.running = true;
            st.dictation_start = Some(Instant::now());
            sid
        };

        self.ui.show_overlay(if model_ready {
            "Listening..."
        } else {
            "Loading model…"
        });
        self.ui.set_tray_active(true);

        let (tx, rx) = mpsc::sync_channel::<Vec<f32>>(AUDIO_QUEUE_CAPACITY);
        let assembler = Arc::new(Mutex::new(BlockAssembler::new(CHUNK_SAMPLES)));

        let make_callback = |tx: mpsc::SyncSender<Vec<f32>>,
                             assembler: Arc<Mutex<BlockAssembler>>,
                             ui: Arc<dyn UiEventSink>|
         -> AudioBlockCallback {
            Box::new(move |block: &[f32]| {
                let (loud, tone) = voice_features(block);
                ui.set_overlay_waveform(loud, tone);
                let chunks = {
                    let mut asm = lock_unpoisoned(&assembler);
                    asm.push(block)
                };
                for chunk in chunks {
                    match tx.try_send(chunk) {
                        Ok(()) => {}
                        Err(TrySendError::Full(_)) => {
                            // Bounded channel: drop newest chunk rather than growing RAM unboundedly.
                        }
                        Err(TrySendError::Disconnected(_)) => break,
                    }
                }
            })
        };

        let test_audio_path = std::env::var_os("EQHO_TEST_AUDIO")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from);

        let stream_res: Result<Box<dyn AudioStreamHandle>, String> = if let Some(path) = test_audio_path {
            let cb = make_callback(tx.clone(), Arc::clone(&assembler), Arc::clone(&self.ui));
            TestAudioStream::start(&path, cb)
                .map(|s| Box::new(s) as Box<dyn AudioStreamHandle>)
                .map_err(|e| e.to_string())
        } else {
            let dev_idx = settings.audio_device.and_then(|d| usize::try_from(d).ok());
            let cb = make_callback(tx.clone(), Arc::clone(&assembler), Arc::clone(&self.ui));
            match self.platform.start_capture(dev_idx, cb) {
                Ok(s) => Ok(s),
                Err(first_err) => {
                    if dev_idx.is_some() {
                        let cb_fallback =
                            make_callback(tx.clone(), Arc::clone(&assembler), Arc::clone(&self.ui));
                        match self.platform.start_capture(None, cb_fallback) {
                            Ok(s) => {
                                self.ui.notify("Selected mic unavailable, using default microphone.");
                                Ok(s)
                            }
                            Err(second_err) => Err(second_err.to_string()),
                        }
                    } else {
                        Err(first_err.to_string())
                    }
                }
            }
        };

        let stream = match stream_res {
            Ok(s) => s,
            Err(_err) => {
                let msg = self.mic_unavailable_message();
                self.teardown_session_locked(Some(sid), Some(&msg), true);
                return;
            }
        };

        *lock_unpoisoned(&self.stream) = Some(stream);
        *lock_unpoisoned(&self.assembler) = Some(assembler);
        *lock_unpoisoned(&self.audio_tx) = Some(tx);

        // Chime BEFORE ducking (fixes race where mute swallows the start chime).
        if settings.sound_feedback {
            let _ = self
                .platform
                .play_pcm(&start_chime(), CHIME_SAMPLE_RATE, true);
        }
        self.duck_volume(&settings.volume_duck);

        let stop_flag = Arc::new(AtomicBool::new(false));
        *lock_unpoisoned(&self.stop_flag) = Some(Arc::clone(&stop_flag));

        let inner = Arc::clone(self);
        let stop_worker = Arc::clone(&stop_flag);
        match thread::Builder::new()
            .name(format!("eqho-worker-{sid}"))
            .spawn(move || inner.worker_loop(sid, rx, stop_worker))
        {
            Ok(handle) => {
                *lock_unpoisoned(&self.worker_handle) = Some(handle);
            }
            Err(e) => {
                let msg = format!("Failed to start transcription worker: {e}");
                self.teardown_session_locked(Some(sid), Some(&msg), true);
            }
        }
    }

    fn deactivate(self: &Arc<Self>) {
        let sound_feedback = {
            let _guard = lock_unpoisoned(&self.session_lock);
            let sound_feedback = lock_unpoisoned(&self.settings).sound_feedback;

            {
                let mut st = lock_unpoisoned(&self.state);
                if !st.running || st.finishing {
                    return;
                }
                st.running = false;
                st.finishing = true;
            }

            // Immediately close mic stream so the OS mic indicator turns off right away.
            *lock_unpoisoned(&self.stream) = None;

            // Flush any residual < 500 ms audio buffered in BlockAssembler so the final word is
            // never clipped off when the user presses the hotkey right after finishing a sentence.
            if let (Some(asm), Some(tx)) = (
                lock_unpoisoned(&self.assembler).take(),
                lock_unpoisoned(&self.audio_tx).take(),
            ) {
                let tail = lock_unpoisoned(&asm).flush();
                if !tail.is_empty() {
                    let _ = tx.try_send(tail);
                }
            }

            if let Some(flag) = lock_unpoisoned(&self.stop_flag).take() {
                flag.store(true, Ordering::Release);
            }

            self.restore_volume();
            self.ui.set_tray_active(false);
            self.ui.update_overlay_text(WRITING_TEXT);

            sound_feedback
        };

        if sound_feedback {
            let _ = self
                .platform
                .play_pcm(&stop_chime(), CHIME_SAMPLE_RATE, false);
        }

        let inner = Arc::clone(self);
        match thread::Builder::new()
            .name("eqho-finish".into())
            .spawn(move || inner.finish_dictation())
        {
            Ok(handle) => {
                *lock_unpoisoned(&self.finish_handle) = Some(handle);
            }
            Err(_) => {
                self.finish_dictation();
            }
        }
    }

    fn toggle(self: &Arc<Self>) {
        if self.dictation_suspended.load(Ordering::Acquire) {
            return;
        }
        let (running, finishing) = {
            let st = lock_unpoisoned(&self.state);
            (st.running, st.finishing)
        };
        if finishing {
            self.ui.update_overlay_text(STILL_WRITING_TEXT);
            self.ui.notify(STILL_WRITING_TEXT);
            return;
        }
        if running {
            self.deactivate();
        } else {
            self.activate();
        }
    }

    fn finish_dictation(self: &Arc<Self>) {
        let worker_handle = lock_unpoisoned(&self.worker_handle).take();
        if let Some(handle) = worker_handle {
            let _ = handle.join();
        }

        // Bump/clear active_session to fence any late results before injection.
        {
            let mut st = lock_unpoisoned(&self.state);
            st.active_session = 0;
        }

        self.inject_pending();

        let _guard = lock_unpoisoned(&self.session_lock);
        {
            let mut st = lock_unpoisoned(&self.state);
            st.pending_text.clear();
            st.finishing = false;
        }
        self.ui.hide_overlay();
    }

    fn worker_loop(
        self: &Arc<Self>,
        sid: u64,
        rx: mpsc::Receiver<Vec<f32>>,
        stop_flag: Arc<AtomicBool>,
    ) {
        let settings = lock_unpoisoned(&self.settings).clone();
        if let Err(err_msg) = self.ensure_model_loaded(&settings) {
            self.abort_session(sid, &err_msg);
            return;
        }

        if self.is_session_current(sid) {
            self.ui.update_overlay_text("Listening...");
        }

        let mut segmenter = PhraseSegmenter::new(settings.vad_sensitivity as f32, 2, 30.0);

        loop {
            if !self.is_session_current(sid) {
                break;
            }
            if stop_flag.load(Ordering::Acquire) {
                break;
            }

            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(chunk) => {
                    // Drain any already-queued chunks before firing slow partial inference so
                    // live partials never fall behind real-time speech.
                    let mut batch = vec![chunk];
                    while let Ok(next) = rx.try_recv() {
                        batch.push(next);
                    }
                    let last_idx = batch.len().saturating_sub(1);
                    for (idx, item) in batch.into_iter().enumerate() {
                        let skip_partials =
                            idx < last_idx || stop_flag.load(Ordering::Acquire);
                        if !self.process_chunk(sid, &item, &mut segmenter, &settings, skip_partials)
                        {
                            return;
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }

        // Drain any remaining chunks queued before stream close. Never run partial inference
        // while draining (and never let a skipped partial abort the flush!).
        while let Ok(chunk) = rx.try_recv() {
            if !self.is_session_current_for_flush(sid) {
                return;
            }
            if !self.process_chunk(sid, &chunk, &mut segmenter, &settings, true) {
                return;
            }
        }

        if !self.is_session_current_for_flush(sid) {
            return;
        }
        if let Some(ev) = segmenter.flush_on_stop() {
            let _ = self.handle_vad_event(sid, ev, &settings, true);
        }
    }

    fn process_chunk(
        self: &Arc<Self>,
        sid: u64,
        chunk: &[f32],
        segmenter: &mut PhraseSegmenter,
        settings: &Settings,
        skip_partials: bool,
    ) -> bool {
        let rms = compute_rms(chunk);
        if self.is_session_current(sid) {
            self.ui.set_overlay_level(level_meter(rms));
        }

        for ev in segmenter.push_chunk(chunk) {
            if !self.handle_vad_event(sid, ev, settings, skip_partials) {
                return false;
            }
        }
        true
    }

    fn handle_vad_event(
        self: &Arc<Self>,
        sid: u64,
        ev: VadEvent,
        settings: &Settings,
        skip_partials: bool,
    ) -> bool {
        let lang = if is_english_only_model(&settings.model_size) {
            "en"
        } else {
            settings.language.as_str()
        };

        match ev {
            VadEvent::PartialReady(audio) => {
                // Skip partials cleanly during stop/drain or batch catch-up without aborting
                // the phrase drain & final `flush_on_stop()`.
                if skip_partials || !self.is_session_current(sid) {
                    return true;
                }
                let (allowed, is_gpu) = {
                    let backend = lock_unpoisoned(&self.speech);
                    let gpu = backend
                        .info()
                        .is_some_and(|i| i.device == "cuda" || i.device == "gpu" || i.device == "vulkan");
                    let last = lock_unpoisoned(&self.state).last_infer_s;
                    let ok = last.map_or(gpu, |s| s <= PARTIAL_MAX_INFER_S);
                    (ok, gpu)
                };
                let _ = is_gpu;
                if !allowed {
                    return true;
                }

                let t0 = Instant::now();
                let res = {
                    let mut backend = lock_unpoisoned(&self.speech);
                    backend.transcribe(&audio, lang, 1, &settings.initial_prompt)
                };
                let elapsed = t0.elapsed().as_secs_f64();
                lock_unpoisoned(&self.state).last_infer_s = Some(elapsed);
                self.maybe_notify_slow_model(elapsed);

                match res {
                    Ok(segs) => {
                        let text: String = segs
                            .iter()
                            .map(|s| s.text.trim())
                            .filter(|s| !s.is_empty())
                            .collect::<Vec<_>>()
                            .join(" ");
                        if !text.is_empty() {
                            let st = lock_unpoisoned(&self.state);
                            if st.active_session == sid && !st.finishing {
                                drop(st);
                                self.ui.update_overlay_text(&text);
                            }
                        }
                        true
                    }
                    Err(err) => {
                        self.preserve_pending_on_error(settings);
                        let msg = format_transcribe_error(&err);
                        self.abort_session(sid, &msg);
                        false
                    }
                }
            }
            VadEvent::PhraseReady {
                audio,
                peak_rms,
                threshold,
            } => {
                if peak_rms < threshold * NEAR_SILENCE_FACTOR {
                    return true;
                }

                // Distilled and turbo Whisper models (`distil-*`, `*turbo*`) are tuned for greedy
                // decoding (`beam_size = 1`, ~250 ms on RTX 3060 vs 5–8 s with 5-beam search).
                // Keep live dictation snappy across all models by capping beam search.
                let beam_size = {
                    let is_fast_arch = settings.model_size.starts_with("distil")
                        || settings.model_size.contains("turbo");
                    if is_fast_arch {
                        1
                    } else {
                        let backend = lock_unpoisoned(&self.speech);
                        let gpu = backend.info().is_some_and(|i| {
                            i.device == "cuda" || i.device == "gpu" || i.device == "vulkan"
                        });
                        if gpu { 2 } else { 1 }
                    }
                };

                let t0 = Instant::now();
                let res = {
                    let mut backend = lock_unpoisoned(&self.speech);
                    backend.transcribe(&audio, lang, beam_size, &settings.initial_prompt)
                };
                let elapsed = t0.elapsed().as_secs_f64();
                lock_unpoisoned(&self.state).last_infer_s = Some(elapsed);
                self.maybe_notify_slow_model(elapsed);

                match res {
                    Ok(segs) => {
                        let vad_segs: Vec<Segment> = segs.iter().map(Segment::from).collect();
                        let duration_s = audio.len() as f64 / SAMPLE_RATE as f64;
                        if let Some(text) =
                            filter_segments(&vad_segs, duration_s, Some(peak_rms), threshold)
                        {
                            self.deliver_complete(sid, text, settings);
                        }
                        true
                    }
                    Err(err) => {
                        self.preserve_pending_on_error(settings);
                        let msg = format_transcribe_error(&err);
                        self.abort_session(sid, &msg);
                        false
                    }
                }
            }
        }
    }

    fn deliver_complete(&self, sid: u64, mut text: String, settings: &Settings) {
        if settings.voice_commands {
            if is_delete_command(&text) {
                self.handle_delete_command(sid);
                return;
            }
            if let Some(cmd) = match_command(&text) {
                text = cmd.to_owned();
            }
        }

        let (full_so_far, finishing) = {
            let mut st = lock_unpoisoned(&self.state);
            if st.active_session != sid {
                return;
            }
            st.pending_text.push(text);
            let refs: Vec<&str> = st.pending_text.iter().map(String::as_str).collect();
            (smart_join(&refs), st.finishing)
        };

        if !finishing {
            self.ui.update_overlay_text(&full_so_far);
        }
    }

    fn handle_delete_command(&self, sid: u64) {
        let (remaining, to_backspace, finishing) = {
            let mut st = lock_unpoisoned(&self.state);
            if st.active_session != sid {
                return;
            }
            if !st.pending_text.is_empty() {
                st.pending_text.pop();
                let refs: Vec<&str> = st.pending_text.iter().map(String::as_str).collect();
                (Some(smart_join(&refs)), None, st.finishing)
            } else if !st.last_injected.is_empty() {
                let prev = std::mem::take(&mut st.last_injected);
                (None, Some(prev), st.finishing)
            } else {
                (Some(String::new()), None, st.finishing)
            }
        };

        if let Some(prev) = to_backspace {
            let count = prev.chars().count();
            if count <= MAX_BACKSPACE_CHARS {
                let _ = self.platform.send_backspaces(count);
            }
            return;
        }

        if let Some(rem) = remaining
            && !finishing
        {
            let display = if rem.is_empty() { "Listening..." } else { &rem };
            self.ui.update_overlay_text(display);
        }
    }

    fn preserve_pending_on_error(&self, settings: &Settings) {
        let text = {
            let st = lock_unpoisoned(&self.state);
            let refs: Vec<&str> = st.pending_text.iter().map(String::as_str).collect();
            let joined = smart_join(&refs);
            let replaced = apply_replacements(&joined, &settings.replacements);
            if settings.format_cleanup {
                clean_text(&replaced, &settings.language, settings.remove_fillers)
            } else {
                replaced
            }
        };

        if !text.is_empty() {
            lock_unpoisoned(&self.state).recovery.store(&text);
            self.ui.on_recovery_available(Some(&text));
        }
    }

    fn abort_session(&self, sid: u64, error: &str) {
        let _guard = lock_unpoisoned(&self.session_lock);
        self.teardown_session_locked(Some(sid), Some(error), true);
    }

    fn teardown_session_locked(&self, sid: Option<u64>, error: Option<&str>, hide_overlay: bool) {
        {
            let mut st = lock_unpoisoned(&self.state);
            if let Some(expected) = sid
                && st.active_session != expected
            {
                return;
            }
            st.active_session = 0;
            st.running = false;
        }

        *lock_unpoisoned(&self.stream) = None;
        *lock_unpoisoned(&self.assembler) = None;
        *lock_unpoisoned(&self.audio_tx) = None;
        if let Some(flag) = lock_unpoisoned(&self.stop_flag).take() {
            flag.store(true, Ordering::Release);
        }

        self.restore_volume();
        if hide_overlay {
            self.ui.hide_overlay();
        }
        self.ui.set_tray_active(false);
        if let Some(err) = error {
            self.ui.notify(err);
        }
    }

    fn inject_pending(&self) {
        let settings = lock_unpoisoned(&self.settings).clone();
        let (raw_text, target_hwnd, duration_s) = {
            let mut st = lock_unpoisoned(&self.state);
            let refs: Vec<&str> = st.pending_text.iter().map(String::as_str).collect();
            let joined = smart_join(&refs);
            st.pending_text.clear();
            let dur = st
                .dictation_start
                .take()
                .map(|t| t.elapsed().as_secs_f64())
                .unwrap_or(0.0);
            (joined, st.target_hwnd, dur)
        };

        let mut full_text = apply_replacements(&raw_text, &settings.replacements);
        if settings.format_cleanup {
            full_text = clean_text(&full_text, &settings.language, settings.remove_fillers);
        }

        if full_text.is_empty() {
            return;
        }

        // Always store in LastDictationBuffer BEFORE attempting injection.
        lock_unpoisoned(&self.state).recovery.store(&full_text);

        let mut use_clipboard = settings.auto_paste;
        if target_hwnd.is_valid()
            && !settings.paste_rules.is_empty()
            && let Some(exe) = self.platform.get_window_process_name(target_hwnd)
        {
            let exe_low = exe.to_lowercase();
            let rule = settings
                .paste_rules
                .iter()
                .find(|(k, _)| k.to_lowercase() == exe_low)
                .map(|(_, v)| v.as_str());
            match rule {
                Some("typing") => use_clipboard = false,
                Some("clipboard") => use_clipboard = true,
                _ => {}
            }
        }

        // Modifier hygiene (EQ-MODS).
        let mut need_settle = false;
        if let Some(held) = self.platform.held_modifiers()
            && !held.is_empty()
        {
            let _ = self.platform.release_modifiers(&held);
            need_settle = true;
        }

        // Verified focus restore.
        if target_hwnd.is_valid() {
            let already_focused = self.platform.get_foreground_window() == target_hwnd;
            let restored = self.platform.restore_foreground_window(target_hwnd);
            if !restored && self.platform.get_foreground_window() != target_hwnd {
                self.ui.on_recovery_available(Some(&full_text));
                self.ui.notify(FOCUS_RESTORE_FAILED_TEXT);
                return;
            }
            if !already_focused {
                need_settle = true;
            }
        }
        if need_settle {
            thread::sleep(INJECT_SETTLE_DELAY);
        }

        let inject_res = if use_clipboard {
            let prev = self.platform.get_text();
            self.platform
                .set_text(&full_text, true)
                .and_then(|()| self.platform.send_paste_shortcut())
                .map(|()| {
                    thread::sleep(CLIPBOARD_RESTORE_DELAY);
                    if let Some(prev_text) = prev {
                        let _ = self.platform.set_text(&prev_text, false);
                    }
                })
        } else {
            self.platform.type_unicode_text(&full_text)
        };

        if let Err(err) = inject_res {
            self.ui.on_recovery_available(Some(&full_text));
            self.ui
                .notify(&format!("Couldn't insert your dictation ({err}). It is saved in Eqho."));
            return;
        }

        lock_unpoisoned(&self.state).last_injected = full_text.clone();
        self.ui.on_dictation_committed(&full_text);
        if settings.history_enabled {
            let _ = self.history.append(
                &full_text,
                duration_s,
                &settings.model_size,
                &settings.language,
            );
            self.ui.on_history_changed();
        }
    }

    fn ensure_model_loaded(&self, settings: &Settings) -> Result<ModelInfo, String> {
        let use_gpu = has_discrete_gpu(self.platform.as_ref(), &settings.engine_backend);
        let model_key = &settings.model_size;
        let model_dir = settings.resolve_model_dir();
        let threads = default_threads();

        let mut backend = lock_unpoisoned(&self.speech);
        if backend.is_ready(model_key, use_gpu)
            && let Some(info) = backend.info().cloned()
        {
            return Ok(info);
        }

        // Low-memory warning when running on PC RAM (CPU mode — warning only, never blocks).
        if !use_gpu
            && let Some(free_mb) = self.platform.free_memory_mb()
        {
            let need_mb = required_memory_mb(model_key);
            if free_mb < need_mb {
                self.ui.notify(&low_memory_warning(free_mb, need_mb));
            }
        }

        let ui = Arc::clone(&self.ui);
        let key_owned = model_key.clone();
        let mut on_progress = |msg: &str, frac: Option<f32>| {
            ui.on_model_progress(&key_owned, msg, frac);
        };

        match backend.load_model(model_key, &model_dir, use_gpu, threads, &mut on_progress) {
            Ok(info) => Ok(info),
            Err(ModelHostError::LowMemory(_)) => Err(LOW_MEMORY_LOAD_TEXT.to_owned()),
            Err(err) => Err(format!("Model failed to load — {err}")),
        }
    }

    fn preload_model(&self) -> Result<ModelInfo, ModelHostError> {
        let settings = lock_unpoisoned(&self.settings).clone();
        let use_gpu = has_discrete_gpu(self.platform.as_ref(), &settings.engine_backend);
        let model_key = settings.model_size.clone();
        let model_dir = settings.resolve_model_dir();
        let threads = default_threads();

        let mut backend = lock_unpoisoned(&self.speech);
        if backend.is_ready(&model_key, use_gpu)
            && let Some(info) = backend.info().cloned()
        {
            return Ok(info);
        }

        let ui = Arc::clone(&self.ui);
        let key_owned = model_key.clone();
        let mut on_progress = |msg: &str, frac: Option<f32>| {
            ui.on_model_progress(&key_owned, msg, frac);
        };

        backend.load_model(&model_key, &model_dir, use_gpu, threads, &mut on_progress)
    }

    fn register_hotkey(self: &Arc<Self>) -> Result<(), String> {
        if self.dictation_suspended.load(Ordering::Acquire) {
            return Ok(());
        }
        let settings = lock_unpoisoned(&self.settings).clone();
        let mode = if settings.hotkey_mode.eq_ignore_ascii_case("hold") {
            HotkeyMode::Hold
        } else {
            HotkeyMode::Toggle
        };
        let spec = HotkeySpec {
            raw: settings.hotkey,
            mode,
        };

        let inner = Arc::clone(self);
        let reg = self
            .platform
            .register(
                &spec,
                Box::new(move |ev| match ev {
                    HotkeyEvent::Toggle => inner.toggle(),
                    HotkeyEvent::HoldPressed => inner.activate(),
                    HotkeyEvent::HoldReleased => inner.deactivate(),
                }),
            )
            .map_err(|e| e.to_string())?;

        *lock_unpoisoned(&self.hotkey_reg) = Some(reg);
        Ok(())
    }

    fn unregister_hotkey(&self) {
        *lock_unpoisoned(&self.hotkey_reg) = None;
    }

    fn apply_settings(self: &Arc<Self>, new_settings: Settings, reload_model: bool) {
        let hotkey_changed = {
            let mut s = lock_unpoisoned(&self.settings);
            let changed =
                s.hotkey != new_settings.hotkey || s.hotkey_mode != new_settings.hotkey_mode;
            *s = new_settings.clone();
            changed
        };
        let _ = new_settings.save_to_path(&self.settings_path);
        if hotkey_changed {
            self.unregister_hotkey();
            let _ = self.register_hotkey();
        }
        if reload_model {
            let _ = self.preload_model();
        }
    }

    fn shutdown(&self) {
        self.unregister_hotkey();

        {
            let _guard = lock_unpoisoned(&self.session_lock);
            self.teardown_session_locked(None, None, true);
        }

        if let Some(handle) = lock_unpoisoned(&self.worker_handle).take() {
            let _ = handle.join();
        }
        if let Some(handle) = lock_unpoisoned(&self.finish_handle).take() {
            let _ = handle.join();
        }

        // Always unmute on exit.
        let _ = self.platform.set_mute(false);
        self.restore_volume();

        lock_unpoisoned(&self.speech).shutdown();
        let settings = lock_unpoisoned(&self.settings).clone();
        let _ = settings.save_to_path(&self.settings_path);
    }

    fn duck_volume(&self, duck_setting: &str) {
        let Some(mult) = volume_duck_level(duck_setting) else {
            return;
        };
        let Some(saved) = self.platform.get_volume() else {
            return;
        };
        lock_unpoisoned(&self.state).saved_volume = Some(saved);
        if mult == 0.0 {
            let _ = self.platform.set_mute(true);
        } else {
            let _ = self.platform.set_volume((saved * mult).clamp(0.0, 1.0));
        }
    }

    fn restore_volume(&self) {
        let saved = lock_unpoisoned(&self.state).saved_volume;
        let Some(level) = saved else {
            return;
        };
        let _ = self.platform.set_mute(false);
        if self.platform.set_volume(level) {
            lock_unpoisoned(&self.state).saved_volume = None;
        }
    }

    fn mic_unavailable_message(&self) -> String {
        if self.platform.mic_access_blocked() == Some(true) {
            let should_open = {
                let mut st = lock_unpoisoned(&self.state);
                if st.mic_settings_opened {
                    false
                } else {
                    st.mic_settings_opened = true;
                    true
                }
            };
            if should_open {
                let _ = self.platform.open_mic_privacy_settings();
            }
            MIC_PRIVACY_BLOCKED_TEXT.to_owned()
        } else {
            MIC_UNAVAILABLE_TEXT.to_owned()
        }
    }

    fn maybe_notify_slow_model(&self, elapsed_s: f64) {
        if elapsed_s <= SLOW_MODEL_HINT_SECS {
            return;
        }
        let hint_msg = {
            let mut s = lock_unpoisoned(&self.settings);
            if s.slow_hint_shown_for == s.model_size {
                return;
            }
            let faster_key = recommend_model(HardwareClass::Cpu, &s.language);
            let cur_size = model_info(&s.model_size).map(|m| m.size_mb).unwrap_or(0);
            let faster_size = model_info(faster_key).map(|m| m.size_mb).unwrap_or(0);
            if faster_key == s.model_size || faster_size >= cur_size {
                return;
            }
            let cur_name = model_info(&s.model_size)
                .map(|m| m.label.split(" (").next().unwrap_or(m.label))
                .unwrap_or(&s.model_size)
                .to_owned();
            let faster_name = model_info(faster_key)
                .map(|m| m.label.split(" (").next().unwrap_or(m.label))
                .unwrap_or(faster_key);
            let msg = format!(
                "{cur_name} took {elapsed_s:.1} s on this PC. For faster replies, try {faster_name} in Eqho → Models."
            );
            s.slow_hint_shown_for = s.model_size.clone();
            let _ = s.save_to_path(&self.settings_path);
            msg
        };
        self.ui.notify(&hint_msg);
    }

    fn is_session_current(&self, sid: u64) -> bool {
        let st = lock_unpoisoned(&self.state);
        st.active_session == sid && st.running
    }

    fn is_session_current_for_flush(&self, sid: u64) -> bool {
        let st = lock_unpoisoned(&self.state);
        st.active_session == sid
    }
}

fn has_discrete_gpu(platform: &dyn EqhoPlatform, engine_backend: &str) -> bool {
    let backend = engine_backend.trim().to_ascii_lowercase();
    if backend == "cpu" {
        return false;
    }
    if backend == "vulkan" || backend == "gpu" {
        return true;
    }
    platform
        .list_adapters()
        .iter()
        .any(|a| a.dedicated_video_memory_bytes >= DISCRETE_GPU_MIN_VRAM_BYTES)
}

fn default_threads() -> u32 {
    thread::available_parallelism()
        .map(|n| n.get().min(8) as u32)
        .unwrap_or(4)
}

fn compute_rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f64 = samples.iter().map(|&x| f64::from(x) * f64::from(x)).sum();
    (sum_sq / samples.len() as f64).sqrt() as f32
}

fn format_transcribe_error(err: &ModelHostError) -> String {
    match err {
        ModelHostError::LowMemory(_) => LOW_MEMORY_LOAD_TEXT.to_owned(),
        ModelHostError::Timeout(_) => {
            "Transcription timed out — your dictation was saved in Eqho if any phrases finished."
                .to_owned()
        }
        other => format!("Transcription failed — {other}"),
    }
}

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::fs;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    use eqho_core::vad::WAVE_BLOCK_SAMPLES;
    use eqho_host::HostSegment;
    use eqho_platform::{
        AudioBackend, AudioDeviceInfo, ClipboardService, FocusTracker, GpuAdapter, GpuProbe,
        HotkeyCallback, HotkeyService, InputInjector, MemoryProbe, MicPrivacyProbe, ModifierKey,
        PlatformError, VolumeControl,
    };

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Action {
        PlayPcm { blocking: bool, len: usize },
        SetVolume(u32),
        SetMute(bool),
        RestoreFocus(usize, bool),
        SetClipboard { text: String, private: bool },
        SendPaste,
        TypeText(String),
        SendBackspaces(usize),
    }

    struct FakeStream;
    impl AudioStreamHandle for FakeStream {}

    struct FakeHotkey;
    impl HotkeyRegistration for FakeHotkey {}

    struct FakePlatform {
        foreground: Mutex<WindowHandle>,
        restore_succeeds: Mutex<bool>,
        clipboard: Mutex<Option<String>>,
        volume: Mutex<f32>,
        muted: Mutex<bool>,
        held_mods: Mutex<Vec<ModifierKey>>,
        audio_cb: Mutex<Option<AudioBlockCallback>>,
        wav_path_override: Mutex<Option<PathBuf>>,
        actions: Mutex<Vec<Action>>,
    }

    impl FakePlatform {
        fn new(target: WindowHandle) -> Self {
            Self {
                foreground: Mutex::new(target),
                restore_succeeds: Mutex::new(true),
                clipboard: Mutex::new(Some("previous-clip".to_owned())),
                volume: Mutex::new(0.8),
                muted: Mutex::new(false),
                held_mods: Mutex::new(vec![ModifierKey::AltLeft]),
                audio_cb: Mutex::new(None),
                wav_path_override: Mutex::new(None),
                actions: Mutex::new(Vec::new()),
            }
        }

        fn push_block(&self, block: &[f32]) {
            if let Some(cb) = lock_unpoisoned(&self.audio_cb).as_ref() {
                cb(block);
            }
        }

        fn actions(&self) -> Vec<Action> {
            lock_unpoisoned(&self.actions).clone()
        }
    }

    impl GpuProbe for FakePlatform {
        fn list_adapters(&self) -> Vec<GpuAdapter> {
            vec![GpuAdapter {
                name: "Test GPU".into(),
                vendor_id: 0x10DE,
                dedicated_video_memory_bytes: 8 * 1024 * 1024 * 1024,
            }]
        }
        fn cpu_has_avx2(&self) -> bool {
            true
        }
    }

    impl MemoryProbe for FakePlatform {
        fn free_memory_mb(&self) -> Option<u64> {
            Some(8192)
        }
    }

    impl MicPrivacyProbe for FakePlatform {
        fn mic_access_blocked(&self) -> Option<bool> {
            Some(false)
        }
        fn open_mic_privacy_settings(&self) -> bool {
            false
        }
    }

    impl VolumeControl for FakePlatform {
        fn is_supported(&self) -> bool {
            true
        }
        fn get_volume(&self) -> Option<f32> {
            Some(*lock_unpoisoned(&self.volume))
        }
        fn set_volume(&self, level: f32) -> bool {
            *lock_unpoisoned(&self.volume) = level;
            lock_unpoisoned(&self.actions).push(Action::SetVolume((level * 100.0).round() as u32));
            true
        }
        fn set_mute(&self, muted: bool) -> bool {
            *lock_unpoisoned(&self.muted) = muted;
            lock_unpoisoned(&self.actions).push(Action::SetMute(muted));
            true
        }
    }

    impl FocusTracker for FakePlatform {
        fn get_foreground_window(&self) -> WindowHandle {
            *lock_unpoisoned(&self.foreground)
        }
        fn restore_foreground_window(&self, window: WindowHandle) -> bool {
            let ok = *lock_unpoisoned(&self.restore_succeeds);
            if ok {
                *lock_unpoisoned(&self.foreground) = window;
            }
            lock_unpoisoned(&self.actions).push(Action::RestoreFocus(window.0, ok));
            ok
        }
        fn get_window_process_name(&self, _window: WindowHandle) -> Option<String> {
            Some("notepad.exe".to_owned())
        }
    }

    impl ClipboardService for FakePlatform {
        fn get_text(&self) -> Option<String> {
            lock_unpoisoned(&self.clipboard).clone()
        }
        fn set_text(&self, text: &str, private: bool) -> Result<(), PlatformError> {
            *lock_unpoisoned(&self.clipboard) = Some(text.to_owned());
            lock_unpoisoned(&self.actions).push(Action::SetClipboard {
                text: text.to_owned(),
                private,
            });
            Ok(())
        }
    }

    impl InputInjector for FakePlatform {
        fn held_modifiers(&self) -> Option<Vec<ModifierKey>> {
            Some(lock_unpoisoned(&self.held_mods).clone())
        }
        fn release_modifiers(&self, _mods: &[ModifierKey]) -> Result<(), PlatformError> {
            lock_unpoisoned(&self.held_mods).clear();
            Ok(())
        }
        fn send_paste_shortcut(&self) -> Result<(), PlatformError> {
            lock_unpoisoned(&self.actions).push(Action::SendPaste);
            Ok(())
        }
        fn type_unicode_text(&self, text: &str) -> Result<(), PlatformError> {
            lock_unpoisoned(&self.actions).push(Action::TypeText(text.to_owned()));
            Ok(())
        }
        fn send_backspaces(&self, count: usize) -> Result<(), PlatformError> {
            lock_unpoisoned(&self.actions).push(Action::SendBackspaces(count));
            Ok(())
        }
    }

    impl HotkeyService for FakePlatform {
        fn register(
            &self,
            _spec: &HotkeySpec,
            _callback: HotkeyCallback,
        ) -> Result<Box<dyn HotkeyRegistration>, PlatformError> {
            Ok(Box::new(FakeHotkey))
        }
    }

    impl AudioBackend for FakePlatform {
        fn list_input_devices(&self) -> Vec<AudioDeviceInfo> {
            vec![AudioDeviceInfo {
                index: 0,
                name: "Fake Mic".into(),
                is_default: true,
            }]
        }
        fn start_capture(
            &self,
            _device_index: Option<usize>,
            on_block: AudioBlockCallback,
        ) -> Result<Box<dyn AudioStreamHandle>, PlatformError> {
            if let Some(path) = lock_unpoisoned(&self.wav_path_override).clone() {
                let stream = TestAudioStream::start(&path, on_block)?;
                return Ok(Box::new(stream));
            }
            *lock_unpoisoned(&self.audio_cb) = Some(on_block);
            Ok(Box::new(FakeStream))
        }
        fn play_pcm(
            &self,
            samples: &[f32],
            _sample_rate: u32,
            blocking: bool,
        ) -> Result<(), PlatformError> {
            lock_unpoisoned(&self.actions).push(Action::PlayPcm {
                blocking,
                len: samples.len(),
            });
            Ok(())
        }
    }

    struct RecordingUi {
        overlay_texts: Mutex<Vec<String>>,
        notifications: Mutex<Vec<String>>,
        recovery_events: Mutex<Vec<Option<String>>>,
        history_changes: Mutex<usize>,
    }

    impl RecordingUi {
        fn new() -> Self {
            Self {
                overlay_texts: Mutex::new(Vec::new()),
                notifications: Mutex::new(Vec::new()),
                recovery_events: Mutex::new(Vec::new()),
                history_changes: Mutex::new(0),
            }
        }
    }

    impl UiEventSink for RecordingUi {
        fn show_overlay(&self, text: &str) {
            lock_unpoisoned(&self.overlay_texts).push(text.to_owned());
        }
        fn update_overlay_text(&self, text: &str) {
            lock_unpoisoned(&self.overlay_texts).push(text.to_owned());
        }
        fn set_overlay_level(&self, _level: f32) {}
        fn set_overlay_waveform(&self, _loud: f32, _tone: f32) {}
        fn hide_overlay(&self) {}
        fn set_tray_active(&self, _active: bool) {}
        fn notify(&self, message: &str) {
            lock_unpoisoned(&self.notifications).push(message.to_owned());
        }
        fn on_model_progress(&self, _model_key: &str, _message: &str, _fraction: Option<f32>) {}
        fn on_recovery_available(&self, text: Option<&str>) {
            lock_unpoisoned(&self.recovery_events).push(text.map(str::to_owned));
        }
        fn on_history_changed(&self) {
            *lock_unpoisoned(&self.history_changes) += 1;
        }
    }

    struct FakeSpeech {
        ready: bool,
        info: Option<ModelInfo>,
        responses: VecDeque<Result<Vec<HostSegment>, ModelHostError>>,
        delay: Duration,
    }

    impl FakeSpeech {
        fn with_responses(responses: Vec<Result<Vec<HostSegment>, ModelHostError>>) -> Self {
            Self {
                ready: false,
                info: None,
                responses: responses.into_iter().collect(),
                delay: Duration::ZERO,
            }
        }
    }

    impl SpeechBackend for FakeSpeech {
        fn is_ready(&mut self, _model_key: &str, _use_gpu: bool) -> bool {
            self.ready
        }
        fn load_model(
            &mut self,
            model_key: &str,
            _model_dir: &Path,
            _use_gpu: bool,
            _threads: u32,
            on_progress: &mut dyn FnMut(&str, Option<f32>),
        ) -> Result<ModelInfo, ModelHostError> {
            on_progress("loading", Some(1.0));
            let info = ModelInfo {
                backend: "whisper.cpp".into(),
                model: model_key.to_owned(),
                device: "gpu".into(),
                whisper_version: "1.8.4".into(),
            };
            self.ready = true;
            self.info = Some(info.clone());
            Ok(info)
        }
        fn transcribe(
            &mut self,
            _audio: &[f32],
            _language: &str,
            _beam_size: u32,
            _initial_prompt: &str,
        ) -> Result<Vec<HostSegment>, ModelHostError> {
            if !self.delay.is_zero() {
                thread::sleep(self.delay);
            }
            self.responses.pop_front().unwrap_or_else(|| Ok(Vec::new()))
        }
        fn info(&self) -> Option<&ModelInfo> {
            self.info.as_ref()
        }
        fn shutdown(&mut self) {}
    }

    fn temp_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("eqho_engine_test_{name}_{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn feed_speech_then_silence(platform: &FakePlatform, speech_chunks: usize, silence_chunks: usize) {
        let speech_block = vec![0.05_f32; WAVE_BLOCK_SAMPLES];
        let silence_block = vec![0.0_f32; WAVE_BLOCK_SAMPLES];
        // 10 blocks = 1 chunk (0.5 s)
        for _ in 0..(speech_chunks * 10) {
            platform.push_block(&speech_block);
        }
        for _ in 0..(silence_chunks * 10) {
            platform.push_block(&silence_block);
        }
    }

    fn wait_until_idle(coord: &DictationCoordinator) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while (coord.is_running() || coord.is_finishing()) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(15));
        }
    }

    fn wait_until_not_finishing(coord: &DictationCoordinator) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while coord.is_finishing() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(15));
        }
    }

    fn seg(text: &str) -> HostSegment {
        HostSegment {
            text: text.to_owned(),
            no_speech_prob: 0.02,
            avg_logprob: -0.1,
        }
    }

    #[test]
    fn happy_path_dictation_chime_duck_focus_clipboard_and_history() {
        let dir = temp_dir("happy");
        let settings = Settings {
            volume_duck: "50%".to_owned(),
            sound_feedback: true,
            auto_paste: true,
            ..Settings::default()
        };

        let platform = Arc::new(FakePlatform::new(WindowHandle(42)));
        let ui = Arc::new(RecordingUi::new());
        let speech = FakeSpeech::with_responses(vec![
            Ok(vec![seg("hello")]),       // partial
            Ok(vec![seg("hello world")]), // complete phrase
        ]);
        let hist = TranscriptHistory::with_default_cap(dir.join("history.jsonl"));
        let coord = DictationCoordinator::new(
            settings,
            dir.join("settings.json"),
            hist,
            platform.clone(),
            Box::new(speech),
            ui.clone(),
        );

        coord.activate();
        thread::sleep(Duration::from_millis(40));
        // 3 speech chunks (1.5s -> triggers partial), then 3 silence chunks (1.5s -> triggers PhraseReady)
        feed_speech_then_silence(&platform, 3, 3);
        thread::sleep(Duration::from_millis(80));
        coord.deactivate();
        wait_until_not_finishing(&coord);

        let actions = platform.actions();
        // Chime played blocking BEFORE ducking (SetVolume(40))
        let chime_idx = actions
            .iter()
            .position(|a| matches!(a, Action::PlayPcm { blocking: true, .. }))
            .expect("start chime should play");
        let duck_idx = actions
            .iter()
            .position(|a| matches!(a, Action::SetVolume(40)))
            .expect("volume should duck to 40%");
        assert!(chime_idx < duck_idx, "start chime must happen before ducking");

        // Volume restored to 80%
        assert!(actions.contains(&Action::SetVolume(80)));
        // Focus restored, private clipboard set, paste sent, previous clipboard restored
        assert!(actions.contains(&Action::RestoreFocus(42, true)));
        assert!(actions.contains(&Action::SetClipboard {
            text: "Hello world".to_owned(),
            private: true,
        }));
        assert!(actions.contains(&Action::SendPaste));
        assert!(actions.contains(&Action::SetClipboard {
            text: "previous-clip".to_owned(),
            private: false,
        }));

        let entries = coord.inner.history.read_all();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].text, "Hello world");
        assert_eq!(*lock_unpoisoned(&ui.history_changes), 1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn focus_restore_failure_does_not_paste_and_triggers_recovery() {
        let dir = temp_dir("focus_fail");
        let platform = Arc::new(FakePlatform::new(WindowHandle(42)));
        *lock_unpoisoned(&platform.restore_succeeds) = false;
        let ui = Arc::new(RecordingUi::new());
        let speech = FakeSpeech::with_responses(vec![Ok(vec![seg("important note")])]);
        let hist = TranscriptHistory::with_default_cap(dir.join("history.jsonl"));
        let coord = DictationCoordinator::new(
            Settings::default(),
            dir.join("settings.json"),
            hist,
            platform.clone(),
            Box::new(speech),
            ui.clone(),
        );

        coord.activate();
        thread::sleep(Duration::from_millis(40));
        // Simulate focus changing to a different window while dictating
        *lock_unpoisoned(&platform.foreground) = WindowHandle(999);
        feed_speech_then_silence(&platform, 2, 3);
        thread::sleep(Duration::from_millis(80));
        coord.deactivate();
        wait_until_not_finishing(&coord);

        let actions = platform.actions();
        assert!(!actions.contains(&Action::SendPaste), "must NOT paste into wrong window");
        assert_eq!(coord.last_dictation().as_deref(), Some("Important note"));
        assert!(
            lock_unpoisoned(&ui.recovery_events)
                .contains(&Some("Important note".to_owned()))
        );
        assert!(
            lock_unpoisoned(&ui.notifications)
                .iter()
                .any(|m| m.contains("Couldn't return focus"))
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn mid_session_transcribe_error_surfaces_notice_restores_volume_and_preserves_text() {
        let dir = temp_dir("transcribe_err");
        let settings = Settings {
            volume_duck: "25%".to_owned(),
            ..Settings::default()
        };

        let platform = Arc::new(FakePlatform::new(WindowHandle(7)));
        let ui = Arc::new(RecordingUi::new());
        let speech = FakeSpeech::with_responses(vec![
            Ok(vec![seg("first phrase")]),
            Err(ModelHostError::TranscribeFailed("boom".into())),
        ]);
        let coord = DictationCoordinator::new(
            settings,
            dir.join("settings.json"),
            TranscriptHistory::with_default_cap(dir.join("history.jsonl")),
            platform.clone(),
            Box::new(speech),
            ui.clone(),
        );

        coord.activate();
        thread::sleep(Duration::from_millis(40));
        // Phrase 1 succeeds
        feed_speech_then_silence(&platform, 2, 3);
        thread::sleep(Duration::from_millis(80));
        // Phrase 2 fails
        feed_speech_then_silence(&platform, 2, 3);
        wait_until_idle(&coord);

        assert!(!coord.is_running());
        assert_eq!(*lock_unpoisoned(&platform.volume), 0.8);
        assert_eq!(coord.last_dictation().as_deref(), Some("First phrase"));
        assert!(
            lock_unpoisoned(&ui.notifications)
                .iter()
                .any(|m| m.contains("Transcription failed"))
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn session_id_fencing_drops_late_result_from_previous_session() {
        let dir = temp_dir("fencing");
        let platform = Arc::new(FakePlatform::new(WindowHandle(10)));
        let ui = Arc::new(RecordingUi::new());
        let mut speech = FakeSpeech::with_responses(vec![
            Ok(vec![seg("stale from session 1")]),
            Ok(vec![seg("fresh from session 2")]),
        ]);
        speech.delay = Duration::from_millis(120);

        let coord = DictationCoordinator::new(
            Settings::default(),
            dir.join("settings.json"),
            TranscriptHistory::with_default_cap(dir.join("history.jsonl")),
            platform.clone(),
            Box::new(speech),
            ui.clone(),
        );

        // Manually verify deliver_complete fences stale session IDs:
        coord.activate();
        let sid1 = lock_unpoisoned(&coord.inner.state).active_session;
        coord.deactivate();
        wait_until_not_finishing(&coord);

        coord.activate();
        let sid2 = lock_unpoisoned(&coord.inner.state).active_session;
        assert_ne!(sid1, sid2);

        // Late delivery from sid1 must be dropped
        let settings = Settings::default();
        coord.inner.deliver_complete(sid1, "stale phrase".into(), &settings);
        assert!(lock_unpoisoned(&coord.inner.state).pending_text.is_empty());

        // Delivery from sid2 succeeds
        coord.inner.deliver_complete(sid2, "fresh phrase".into(), &settings);
        assert_eq!(
            lock_unpoisoned(&coord.inner.state).pending_text,
            vec!["fresh phrase".to_owned()]
        );

        coord.shutdown();
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn delete_that_voice_command_pops_pending_or_sends_backspaces() {
        let dir = temp_dir("delete_cmd");
        let settings = Settings {
            voice_commands: true,
            ..Settings::default()
        };

        let platform = Arc::new(FakePlatform::new(WindowHandle(5)));
        let ui = Arc::new(RecordingUi::new());
        let speech = FakeSpeech::with_responses(Vec::new());
        let coord = DictationCoordinator::new(
            settings.clone(),
            dir.join("settings.json"),
            TranscriptHistory::with_default_cap(dir.join("history.jsonl")),
            platform.clone(),
            Box::new(speech),
            ui,
        );

        coord.activate();
        let sid = lock_unpoisoned(&coord.inner.state).active_session;
        coord.inner.deliver_complete(sid, "hello".into(), &settings);
        coord.inner.deliver_complete(sid, "world".into(), &settings);
        coord.inner.deliver_complete(sid, "delete that".into(), &settings);
        assert_eq!(
            lock_unpoisoned(&coord.inner.state).pending_text,
            vec!["hello".to_owned()]
        );

        // Empty pending + existing last_injected triggers send_backspaces
        coord.inner.deliver_complete(sid, "delete that".into(), &settings);
        lock_unpoisoned(&coord.inner.state).last_injected = "abcde".to_owned();
        coord.inner.deliver_complete(sid, "scratch that".into(), &settings);
        assert!(platform.actions().contains(&Action::SendBackspaces(5)));

        coord.shutdown();
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn wav_feeder_runs_through_coordinator_end_to_end() {
        let dir = temp_dir("wav_e2e");
        let wav_path = dir.join("test.wav");

        // Build 1.5 s of 16 kHz mono PCM16 audio (amplitude ~0.25)
        let num_samples = 24_000_usize;
        let mut pcm = Vec::with_capacity(num_samples * 2);
        for i in 0..num_samples {
            let sample = ((i as f32 * 0.05).sin() * 8000.0) as i16;
            pcm.extend_from_slice(&sample.to_le_bytes());
        }
        let data_len = pcm.len() as u32;
        let mut wav = Vec::with_capacity(44 + pcm.len());
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_len).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&16_000u32.to_le_bytes());
        wav.extend_from_slice(&32_000u32.to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_len.to_le_bytes());
        wav.extend_from_slice(&pcm);
        fs::write(&wav_path, wav).unwrap();

        let platform = Arc::new(FakePlatform::new(WindowHandle(88)));
        *lock_unpoisoned(&platform.wav_path_override) = Some(wav_path);
        let ui = Arc::new(RecordingUi::new());
        let speech = FakeSpeech::with_responses(vec![
            Ok(vec![seg("wav")]),
            Ok(vec![seg("wav test passed")]),
        ]);
        let hist = TranscriptHistory::with_default_cap(dir.join("history.jsonl"));
        let coord = DictationCoordinator::new(
            Settings::default(),
            dir.join("settings.json"),
            hist,
            platform.clone(),
            Box::new(speech),
            ui.clone(),
        );

        coord.activate();
        // Wait until the WAV feeder + VAD emit the completed phrase (bounded deadline so
        // heavy scheduler load never flakes on a fixed sleep).
        let deadline = Instant::now() + Duration::from_secs(10);
        while !lock_unpoisoned(&ui.overlay_texts)
            .iter()
            .any(|t| t.contains("Wav test passed"))
            && Instant::now() < deadline
        {
            thread::sleep(Duration::from_millis(25));
        }
        coord.deactivate();
        wait_until_not_finishing(&coord);

        assert_eq!(coord.last_dictation().as_deref(), Some("Wav test passed"));
        assert_eq!(coord.inner.history.read_all().len(), 1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn run_smoke_passes_all_pure_checks() {
        let platform = FakePlatform::new(WindowHandle::NONE);
        let report = crate::smoke::run_smoke(&platform, None);
        assert!(report.ok, "smoke report failed: {:?}", report.checks);
        assert!(report.checks.len() >= 5);
    }

    #[test]
    fn concurrent_settings_updates_and_model_preload_never_deadlock() {
        let dir = temp_dir("concurrent_settings");
        let platform = Arc::new(FakePlatform::new(WindowHandle(99)));
        let ui = Arc::new(RecordingUi::new());
        let speech = FakeSpeech::with_responses(vec![Ok(vec![seg("ok")])]);
        let hist = TranscriptHistory::with_default_cap(dir.join("history.jsonl"));
        let coord = DictationCoordinator::new(
            Settings::default(),
            dir.join("settings.json"),
            hist,
            platform,
            Box::new(speech),
            ui,
        );

        let c_bg = coord.clone();
        let bg = thread::spawn(move || {
            for i in 0..40 {
                let s = Settings {
                    model_size: if i % 2 == 0 {
                        "distil-large-v3".to_owned()
                    } else {
                        "small.en-q5".to_owned()
                    },
                    ..Settings::default()
                };
                c_bg.apply_settings(s, false);
                let _ = c_bg.preload_model();
            }
        });

        let c_ui = coord.clone();
        let ui_t = thread::spawn(move || {
            for i in 0..80 {
                let s = Settings {
                    volume_duck: if i % 2 == 0 {
                        "mute".to_owned()
                    } else {
                        "off".to_owned()
                    },
                    ..Settings::default()
                };
                c_ui.apply_settings(s, false);
            }
        });

        bg.join().expect("background download/preload thread should not deadlock");
        ui_t.join().expect("UI settings thread should not deadlock");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn slow_model_hint_fires_once_per_model_and_engine_backend_override_works() {
        let dir = temp_dir("slow_hint");
        let settings_path = dir.join("settings.json");
        let platform = Arc::new(FakePlatform::new(WindowHandle(42)));
        let ui = Arc::new(RecordingUi::new());
        let speech = FakeSpeech::with_responses(vec![]);
        let hist = TranscriptHistory::with_default_cap(dir.join("history.jsonl"));

        assert!(!has_discrete_gpu(platform.as_ref(), "cpu"));
        assert!(has_discrete_gpu(platform.as_ref(), "vulkan"));
        assert!(has_discrete_gpu(platform.as_ref(), "gpu"));

        let s = Settings {
            model_size: "distil-large-v3".to_owned(),
            language: "en".to_owned(),
            ..Settings::default()
        };
        let coord = DictationCoordinator::new(
            s,
            settings_path.clone(),
            hist,
            platform,
            Box::new(speech),
            Arc::clone(&ui) as Arc<dyn UiEventSink>,
        );

        // Below threshold: no notification.
        coord.inner.maybe_notify_slow_model(2.5);
        assert!(ui.notifications.lock().unwrap().is_empty());

        // Above 6s threshold: fires once and records slow_hint_shown_for.
        coord.inner.maybe_notify_slow_model(8.2);
        {
            let notes = ui.notifications.lock().unwrap();
            assert_eq!(notes.len(), 1);
            assert!(notes[0].contains("Distil Large v3"));
            assert!(notes[0].contains("Small EN Q5"));
        }

        // Second slow utterance for same model: does NOT spam.
        coord.inner.maybe_notify_slow_model(9.5);
        assert_eq!(ui.notifications.lock().unwrap().len(), 1);

        let persisted = Settings::load_from_path(&settings_path);
        assert_eq!(persisted.slow_hint_shown_for, "distil-large-v3");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn stop_while_queued_chunks_trigger_partial_never_drops_dictation() {
        // Regression test: previously, when chunks crossing `PARTIAL_INTERVAL_S` (1.0 s) were
        // drained after `deactivate()` set `running = false`, `PartialReady` returned `false`
        // and exited `worker_loop` BEFORE `segmenter.flush_on_stop()`, silently discarding the
        // entire spoken utterance!
        let dir = temp_dir("drain_partial");
        let platform = Arc::new(FakePlatform::new(WindowHandle(42)));
        let ui = Arc::new(RecordingUi::new());
        // Supply 2 responses so whichever scheduling order occurs (0 partials + 1 final on drain,
        // or 1 partial + 1 final), the final flush always receives its transcript.
        let mut speech = FakeSpeech::with_responses(vec![
            Ok(vec![seg("never drop this text")]),
            Ok(vec![seg("never drop this text")]),
        ]);
        speech.delay = Duration::from_millis(10);
        let hist = TranscriptHistory::with_default_cap(dir.join("history.jsonl"));
        let coord = DictationCoordinator::new(
            Settings::default(),
            dir.join("settings.json"),
            hist,
            platform.clone(),
            Box::new(speech),
            ui.clone(),
        );

        // Direct invariant check: `PartialReady` while `running == false` / `skip_partials == true`
        // MUST return `true` so the drain loop never aborts before `flush_on_stop()`.
        assert!(coord.inner.handle_vad_event(
            999,
            VadEvent::PartialReady(vec![0.05; 16_000]),
            &Settings::default(),
            true,
        ));
        assert!(coord.inner.handle_vad_event(
            999,
            VadEvent::PartialReady(vec![0.05; 16_000]),
            &Settings::default(),
            false,
        ));

        coord.activate();
        // Wait for capture stream to be installed, then push 3.5 chunks (1.75 s, including
        // 250 ms residual in BlockAssembler!) and stop immediately with zero trailing silence.
        let deadline = Instant::now() + Duration::from_secs(2);
        while lock_unpoisoned(&platform.audio_cb).is_none() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        let speech_block = vec![0.06_f32; WAVE_BLOCK_SAMPLES];
        for _ in 0..35 {
            platform.push_block(&speech_block);
        }
        coord.deactivate();
        wait_until_not_finishing(&coord);

        let actions = platform.actions();
        assert!(
            actions.contains(&Action::SetClipboard {
                text: "Never drop this text".to_owned(),
                private: true,
            }),
            "utterance must NOT be dropped during drain: actions={actions:?}"
        );
        assert!(actions.contains(&Action::SendPaste));
        let _ = fs::remove_dir_all(&dir);
    }
}
