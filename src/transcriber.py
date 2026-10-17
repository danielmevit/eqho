"""Live mic recording + VAD-based chunking. Inference runs OUT OF PROCESS via
model_host (the model never loads in this process — see model_host.py)."""

import logging
import os
import queue
import threading
import time
from typing import Callable, Optional

import numpy as np
import sounddevice as sd

from . import oskit
from .settings import Settings

log = logging.getLogger(__name__)

os.environ.setdefault("HF_HUB_DISABLE_TELEMETRY", "1")
os.environ.setdefault("HF_HUB_DISABLE_IMPLICIT_TOKEN", "1")

SAMPLE_RATE = 16000
CHUNK_DURATION = 0.5
# The mic delivers small blocks (50 ms) so the overlay can draw the live
# waveform; _audio_callback re-assembles them into the same CHUNK_DURATION
# chunks the worker always got (VAD/partials/flush timing unchanged).
WAVE_BLOCK = int(SAMPLE_RATE * 0.05)
WAVE_POINTS = 48  # downsampled, peak-preserving points per block


def downsample_wave(block: np.ndarray, points: int = WAVE_POINTS) -> np.ndarray:
    """Signed peak-preserving downsample: per bin, the sample with the
    largest magnitude (keeps the voice's shape; a plain mean cancels it)."""
    n = (len(block) // points) * points
    if n == 0:
        return np.zeros(points, dtype=np.float32)
    bins = block[:n].reshape(points, -1)
    idx = np.argmax(np.abs(bins), axis=1)
    return bins[np.arange(points), idx].astype(np.float32)
# VAD threshold is ADAPTIVE since v0.6.8: it tracks the ambient noise floor
# and sits at noise_floor×3.5, clamped to [VAD_THRESHOLD_MIN, SILENCE_THRESHOLD].
# Quiet mics (whose speech never crossed the old fixed 0.003) now trigger
# reliably; noisy rooms still cap at the old value.
SILENCE_THRESHOLD = 0.003       # legacy fixed value = upper clamp
VAD_THRESHOLD_MIN = 0.0009      # never trigger below this (electrical noise)
VAD_NOISE_MULTIPLIER = 3.5      # speech must be this × the noise floor
SILENCE_TIMEOUT = 1.2
MIN_PHRASE_DURATION = 0.4
PARTIAL_INTERVAL = 1.0  # send a partial transcription every N seconds of active speech
PARTIAL_TAIL_SECONDS = 10.0  # partials re-transcribe at most this much recent audio
SLOW_HINT_S = 6.0  # a final model call slower than this may show the slow-model tip
STOP_JOIN_TIMEOUT = 5.0  # stop() waits this long for the final flush; anything
                         # slower is dropped (the session is closed) rather than
                         # leaking into the next dictation. The app's normal stop
                         # uses request_stop()/finish_stop() with a much longer cap.
PARTIAL_MAX_INFER_S = 1.5  # live preview only while one model call is faster than this

# Low free memory (owner rule: plain words, no error codes). A model that runs
# on the PC's memory (CPU faster-whisper, or whisper.cpp — on built-in graphics
# it shares the PC's memory too) needs about size × 1.3 + 300 MB free.
LOW_MEMORY_LOAD_TEXT = ("Not enough free memory to load the speech model. Close "
                        "some programs, or pick a smaller model in Eqho → Models.")
_MEMORY_ERROR_MARKERS = ("memoryerror", "mkl_malloc", "out of memory",
                         "bad_alloc", "failed to allocate")


def looks_like_memory_error(text: str) -> bool:
    low = (text or "").lower()
    return any(m in low for m in _MEMORY_ERROR_MARKERS)


def low_memory_warning(free_mb: int, need_mb: int) -> str:
    return (f"Your computer is short on free memory (about {free_mb / 1024:.1f} GB "
            f"free; this speech model needs about {need_mb / 1024:.1f} GB). Close "
            "some programs, or pick a smaller model in Eqho → Models.")


class LowMemoryLoadError(RuntimeError):
    """A model load failed for lack of memory; str() is the plain user text."""

    def __init__(self):
        super().__init__(LOW_MEMORY_LOAD_TEXT)


# Hallucination gating: Whisper invents text on (near-)silence. Buffers whose
# peak RMS never rose meaningfully above the VAD threshold are skipped, low
# confidence segments are dropped, and short utterances matching the known
# artifact list are discarded.
NEAR_SILENCE_FACTOR = 1.5
NO_SPEECH_PROB_MAX = 0.6
AVG_LOGPROB_MIN = -1.0
HALLUCINATION_BLOCKLIST = {
    "thank you", "thank you very much", "thanks for watching",
    "thank you for watching", "you", "bye", "bye-bye", "the end", "so",
    "subtitles by the amara.org community", "1", "2",
}


# Device/compute selection lives in the model_host backend (it runs in the
# child process, where the engine is actually loaded).


class TestAudioSource:
    """Test-only microphone stand-in, used when EQHO_TEST_AUDIO is set
    (EQ-TESTE2E). Decodes a WAV/FLAC file to float32 mono 16 kHz and feeds
    0.5 s chunks into the audio queue at real-time pace, then silence until
    stopped — the VAD and the worker see exactly what a mic in a quiet room
    would produce. The file is (re-)read on every start(), so a test can swap
    its content between dictations. Zero effect when the env var is unset.
    """

    def __init__(self, path: str, audio_q: "queue.Queue",
                 on_block: Optional[Callable[[np.ndarray], None]] = None) -> None:
        self._path = path
        self._queue = audio_q
        self._on_block = on_block  # waveform hook, fed per 50 ms slice
        self._thread: Optional[threading.Thread] = None
        self._stop = threading.Event()

    def _decode(self) -> np.ndarray:
        import av  # already in the venv (tools use it); test-only path
        container = av.open(str(self._path))
        try:
            stream = container.streams.audio[0]
            resampler = av.AudioResampler(
                format=av.AudioFormat("s16"), layout="mono", rate=SAMPLE_RATE)
            parts = []
            for frame in container.decode(stream):
                outs = resampler.resample(frame)
                if not isinstance(outs, list):  # older PyAV returns one frame
                    outs = [outs]
                for out in outs:
                    arr = out.to_ndarray()
                    if arr.ndim == 2:  # packed mono -> (1, n)
                        arr = arr.reshape(-1)
                    parts.append(arr.astype(np.float32) / 32768.0)
            return np.concatenate(parts) if parts else np.zeros(0, dtype=np.float32)
        finally:
            container.close()

    def _put(self, chunk: np.ndarray) -> None:
        self._queue.put(chunk)
        if self._on_block:
            for i in range(0, len(chunk), WAVE_BLOCK):
                self._on_block(chunk[i:i + WAVE_BLOCK])

    def _feed(self) -> None:
        try:
            audio = self._decode()
        except Exception as e:
            log.error("EQHO_TEST_AUDIO: could not decode %s: %s", self._path, e)
            return
        chunk_len = int(SAMPLE_RATE * CHUNK_DURATION)
        log.info("TEST AUDIO: feeding %s (%.1f s) in real time",
                 self._path, len(audio) / SAMPLE_RATE)
        for start in range(0, len(audio), chunk_len):
            if self._stop.is_set():
                return
            t0 = time.monotonic()
            self._put(audio[start:start + chunk_len].copy())
            delay = CHUNK_DURATION - (time.monotonic() - t0)  # real-time pace
            if delay > 0:
                self._stop.wait(delay)
        silence = np.zeros(chunk_len, dtype=np.float32)
        while not self._stop.is_set():  # then silence, like a quiet room
            t0 = time.monotonic()
            self._put(silence.copy())
            delay = CHUNK_DURATION - (time.monotonic() - t0)
            if delay > 0:
                self._stop.wait(delay)

    def start(self) -> None:
        self._stop.clear()
        self._thread = threading.Thread(target=self._feed, name="test-audio",
                                        daemon=True)
        self._thread.start()

    def stop(self) -> None:
        self._stop.set()
        if self._thread:
            self._thread.join(timeout=2.0)
            self._thread = None

    def close(self) -> None:
        self.stop()


class VoiceTranscriber:
    """Records from mic, detects speech via energy-based VAD, transcribes with faster-whisper."""

    def __init__(self, settings: Settings):
        self._settings = settings
        from .model_host import ModelHost, resolve_backend
        # The model runs in a child process — switching = kill+respawn a fresh
        # child (the only reliable path; a second in-process CUDA model crashes
        # natively). The main app never loads a model. The engine backend
        # auto-resolves (NVIDIA->faster-whisper, else->whisper.cpp if installed
        # and the CPU can run it).
        engine_setting = getattr(settings, "engine_backend", "auto")
        backend = resolve_backend(engine_setting)
        # "auto" may fall back to faster-whisper once if whisper.cpp fails to
        # load; an engine picked explicitly in Settings reports the error.
        self._auto_engine = engine_setting not in ("faster-whisper", "whisper.cpp")
        self._auto_fallback_used = False
        log.info("Inference backend: %s", backend)
        self._host = ModelHost(backend=backend)
        oskit.get().disable_os_mic_ducking()
        self._on_partial: Optional[Callable[[str], None]] = None
        self._on_complete: Optional[Callable[[str], None]] = None
        self._on_status: Optional[Callable[[str], None]] = None
        self._on_level: Optional[Callable[[float], None]] = None
        self._on_waveform: Optional[Callable[[np.ndarray], None]] = None
        self._chunk_parts: list[np.ndarray] = []  # small mic blocks → one chunk
        self._chunk_fill = 0
        self._on_abort: Optional[Callable[[str, int], None]] = None
        self._on_notice: Optional[Callable[[str], None]] = None
        self._running = False
        self._session_id = 0  # bumped by start()/stop(); results from a
                               # closed session are never delivered
        self._lock = threading.Lock()
        self._model_lock = threading.Lock()  # serializes load/unload across threads
        self._stream_lock = threading.Lock()
        self._model_loaded = False
        self._stream: Optional[sd.InputStream] = None
        self._audio_q: queue.Queue = queue.Queue()
        self._worker: Optional[threading.Thread] = None
        self._current_model_size: Optional[str] = None
        self._mic_error: Optional[str] = None
        self._vad_threshold: float = SILENCE_THRESHOLD  # live adaptive value (diagnostics)
        self._last_infer_s: Optional[float] = None  # last measured model-call time
        self._partials_off_logged = 0  # session that already logged "preview off"
        self._model_dir = settings.resolve_model_dir()
        os.environ["HF_HUB_CACHE"] = str(self._model_dir / "huggingface")

    def set_callbacks(
        self,
        on_partial: Callable[[str], None],
        on_complete: Callable[[str], None],
        on_status: Optional[Callable[[str], None]] = None,
        on_level: Optional[Callable[[float], None]] = None,
        on_abort: Optional[Callable[[str, int], None]] = None,
        on_notice: Optional[Callable[[str], None]] = None,
        on_waveform: Optional[Callable[[np.ndarray], None]] = None,
    ) -> None:
        self._on_partial = on_partial
        self._on_complete = on_complete
        self._on_status = on_status
        self._on_level = on_level
        self._on_abort = on_abort
        self._on_notice = on_notice
        self._on_waveform = on_waveform

    def current_session(self) -> int:
        """Id of the latest session (bumped by start() and stop())."""
        with self._lock:
            return self._session_id

    def is_model_ready(self) -> bool:
        return self._host.is_ready(self._settings.model_size)

    def consume_mic_error(self) -> Optional[str]:
        """Return and clear the last microphone error (None if there was none)."""
        err, self._mic_error = self._mic_error, None
        return err

    def _load_with_auto_fallback(self, target: str) -> bool:
        """Load `target` on the model host. If the engine was chosen by "auto"
        and whisper.cpp fails to load (error reply, ModelHostError, or a dead
        child), log a warning and fall back to faster-whisper ONCE for this
        run so dictation still works. An engine picked explicitly in Settings
        keeps today's behaviour: the failure is reported to the caller. The
        fallback goes through the child host (set_backend kills the child; the
        next load spawns a fresh one) — never a second model in one process."""
        from .model_host import ModelHostError
        try:
            if self._host.load_model(target, self._model_dir):
                return True
        except ModelHostError as e:
            if not (self._auto_engine and not self._auto_fallback_used
                    and self._host.backend == "whisper.cpp"):
                raise  # keep the real error (e.g. a failed download) for the caller
            log.warning("Model host load error on whisper.cpp: %s", e)
        if not (self._auto_engine and not self._auto_fallback_used
                and self._host.backend == "whisper.cpp"):
            return False
        self._auto_fallback_used = True
        log.warning("whisper.cpp failed to load (engine was auto) — "
                    "falling back to faster-whisper for this run")
        self._host.set_backend("faster-whisper")
        return self._host.load_model(target, self._model_dir)

    def _ensure_model(self) -> None:
        with self._model_lock:
            target = self._settings.model_size
            if self._host.is_ready(target):
                self._model_loaded = True
                self._current_model_size = target
                return
            self._model_dir = self._settings.resolve_model_dir()
            self._check_free_memory(target)
            phase_start = time.monotonic()
            try:
                ok = self._load_with_auto_fallback(target)
            except Exception as e:
                if looks_like_memory_error(str(e)) or isinstance(e, MemoryError):
                    log.error("Model load ran out of memory: %s", e)
                    self._model_loaded = False
                    raise LowMemoryLoadError() from e
                raise
            if ok:
                self._current_model_size = target
                self._model_loaded = True
                log.info("Model ready: %s %s (%.1fs)", target, self._host.info,
                         time.monotonic() - phase_start)
            else:
                self._model_loaded = False
                if looks_like_memory_error(getattr(self._host, "last_load_error", "")):
                    raise LowMemoryLoadError()
                raise RuntimeError(f"Model host failed to load {target}")

    def _runs_on_pc_memory(self) -> bool:
        """True when the model will live in the PC's memory: whisper.cpp
        (built-in graphics share it), or faster-whisper without CUDA."""
        if self._host.backend == "whisper.cpp":
            return True
        from .model_host import _cuda_available
        if self._settings.model_size == "large-v3":  # always on the CPU
            return True
        return not _cuda_available()

    def _check_free_memory(self, target: str) -> None:
        """Warn once per load (never block it) when free memory looks too
        small for the model."""
        try:
            if not self._runs_on_pc_memory():
                return
            free_mb = oskit.get().free_memory_mb()
            if free_mb is None:
                return
            from .modelstore import MODEL_SIZES_MB
            size_mb = MODEL_SIZES_MB.get(target)
            if size_mb is None:
                return
            need_mb = int(size_mb * 1.3 + 300)
            if free_mb >= need_mb:
                return
            log.warning("Low free memory before loading %s: %d MB free, about %d MB needed",
                        target, free_mb, need_mb)
            if self._on_notice:
                self._on_notice(low_memory_warning(free_mb, need_mb))
        except Exception:
            log.debug("Free-memory check failed", exc_info=True)

    def transcribe_once(self, audio: np.ndarray) -> str:
        """Synchronously transcribe a float32 mono 16 kHz array (used by --smoke)."""
        self._ensure_model()
        segs = self._host.transcribe(audio, self._settings.language, 1,
                                     self._settings.initial_prompt or "")
        return " ".join(s["text"].strip() for s in segs if s["text"].strip())

    def _open_stream(self, device: Optional[int]):
        test_audio = os.environ.get("EQHO_TEST_AUDIO")
        if test_audio:
            # Test hook (EQ-TESTE2E): feed a file instead of the microphone.
            source = TestAudioSource(test_audio, self._audio_q,
                                     on_block=self._emit_waveform)
            source.start()
            return source
        stream = sd.InputStream(
            samplerate=SAMPLE_RATE,
            channels=1,
            dtype="float32",
            blocksize=WAVE_BLOCK,
            device=device,
            callback=self._audio_callback,
        )
        stream.start()
        return stream

    def _close_stream(self, only=None) -> None:
        """Close the mic stream. With `only`, close it only if it is still
        that stream — a finished session's worker must never close the mic
        of a newer session."""
        with self._stream_lock:
            if only is not None and self._stream is not only:
                return
            if self._stream:
                try:
                    self._stream.stop()
                    self._stream.close()
                except Exception:
                    pass
                self._stream = None

    def start(self) -> None:
        with self._lock:
            if self._running:
                return
            assert self._on_partial and self._on_complete, "Set callbacks before starting"
            self._mic_error = None
            self._running = True
            self._session_id += 1
            session = self._session_id

            self._chunk_parts = []
            self._chunk_fill = 0
            while not self._audio_q.empty():
                try:
                    self._audio_q.get_nowait()
                except queue.Empty:
                    break

            device = self._settings.audio_device
            try:
                self._stream = self._open_stream(device)
            except Exception as e:
                log.warning("Mic device %s failed (%s), falling back to default.", device, e)
                self._mic_error = str(e)
                try:
                    self._stream = self._open_stream(None)
                    log.info("Fallback to default mic succeeded.")
                except Exception as e2:
                    log.error("No microphone available: %s", e2)
                    self._mic_error = str(e2)
                    self._running = False
                    return

            # Model loading happens on the worker thread (not here) so the
            # hotkey-callback thread never blocks; audio recorded while the
            # model loads queues up and is processed once it is ready.
            self._worker = threading.Thread(
                target=self._transcription_loop, args=(session, self._stream),
                daemon=True)
            self._worker.start()
            log.info("Transcription started (session %d, mic recording).", session)

    def _audio_callback(self, indata, frames, time_info, status):
        if status:
            log.warning("Audio callback status: %s", status)
        block = indata[:, 0].copy()
        self._emit_waveform(block)
        # Re-assemble the small blocks into CHUNK_DURATION chunks for the worker.
        self._chunk_parts.append(block)
        self._chunk_fill += len(block)
        chunk_len = int(SAMPLE_RATE * CHUNK_DURATION)
        while self._chunk_fill >= chunk_len:
            buf = np.concatenate(self._chunk_parts)
            self._audio_q.put(buf[:chunk_len])
            rest = buf[chunk_len:]
            self._chunk_parts = [rest] if len(rest) else []
            self._chunk_fill = len(rest)

    def _emit_waveform(self, block: np.ndarray) -> None:
        """Hand a downsampled waveform to the overlay. Runs on the audio
        thread: cheap, never blocks, never raises."""
        cb = self._on_waveform
        if cb is None:
            return
        try:
            cb(downsample_wave(block))
        except Exception:
            pass

    def _transcription_loop(self, session: int, stream=None) -> None:
        """Worker thread body: load the model if needed, then accumulate
        audio, detect speech/silence and transcribe phrases. Results are
        delivered through _deliver, which drops anything whose session is
        no longer current. Any abnormal exit (model-load failure or crash)
        fires on_abort so the app can run its single teardown path."""
        abort_reason: Optional[str] = None
        try:
            if not self.is_model_ready():
                try:
                    self._ensure_model()
                except Exception as e:
                    # A failed load must not leave the session half-alive
                    # (audio ducked, overlay up, tray active) — abort and
                    # let the app's teardown restore everything.
                    log.error("Model load failed: %s", e)
                    if isinstance(e, LowMemoryLoadError):
                        abort_reason = str(e)
                    else:
                        abort_reason = f"Model failed to load — {e}"
                    return
                if self._running and self._on_status:
                    self._on_status("Listening...")

            chunks: list[np.ndarray] = []
            total_samples = 0
            silence_start: Optional[float] = None
            is_speaking = False
            last_partial_time: float = 0
            chunk_count = 0
            log_peak = 0.0        # peak since the last level log line
            utterance_peak = 0.0  # peak since the buffer last reset (gates hallucinations)
            noise_floor: Optional[float] = None
            threshold = SILENCE_THRESHOLD

            while self._running:
                try:
                    chunk = self._audio_q.get(timeout=0.1)
                except queue.Empty:
                    continue

                chunks.append(chunk)
                total_samples += len(chunk)
                rms = float(np.sqrt(np.mean(chunk ** 2)))
                chunk_count += 1
                log_peak = max(log_peak, rms)
                utterance_peak = max(utterance_peak, rms)

                # Adaptive VAD threshold: drop to a new quietest level instantly,
                # creep upward very slowly (so speech doesn't inflate the floor).
                # User sensitivity scales the whole thing: higher sensitivity =
                # lower threshold = picks up quieter speech (for quiet mics).
                if noise_floor is None or rms < noise_floor:
                    noise_floor = rms
                else:
                    noise_floor += (rms - noise_floor) * 0.02
                sens = max(0.5, min(2.0, getattr(self._settings, "vad_sensitivity", 1.0)))
                threshold = max(VAD_THRESHOLD_MIN / sens,
                                min(SILENCE_THRESHOLD / sens,
                                    noise_floor * VAD_NOISE_MULTIPLIER / sens))
                self._vad_threshold = threshold

                if self._on_level:
                    # Normalized mic level for the overlay's audio indicator —
                    # calibrated so NORMAL speaking volume lands around mid-bar
                    self._on_level(min(1.0, (rms / 0.012) ** 0.6))

                # Log mic levels periodically so we can diagnose issues
                if chunk_count % 20 == 0:
                    log.info(
                        "Mic level: RMS=%.5f peak=%.5f threshold=%.4f (floor=%.5f) speaking=%s buf=%.1fs",
                        rms, log_peak, threshold, noise_floor, is_speaking,
                        total_samples / SAMPLE_RATE,
                    )
                    log_peak = 0.0

                if rms > threshold:
                    silence_start = None
                    if not is_speaking:
                        is_speaking = True
                        last_partial_time = time.monotonic()
                        log.info("Speech detected (RMS=%.5f)", rms)

                    now = time.monotonic()
                    buf_duration = total_samples / SAMPLE_RATE
                    if buf_duration > 1.0 and (now - last_partial_time) >= PARTIAL_INTERVAL:
                        last_partial_time = now
                        if not self._partials_allowed(session):
                            continue
                        # Bounded tail keeps partial latency flat on long phrases;
                        # the final transcription still sees the full buffer.
                        tail = max(1, int(PARTIAL_TAIL_SECONDS / CHUNK_DURATION))
                        self._do_partial(np.concatenate(chunks[-tail:]), session)
                else:
                    if is_speaking:
                        if silence_start is None:
                            silence_start = time.monotonic()
                        elif time.monotonic() - silence_start > SILENCE_TIMEOUT:
                            buf_duration = total_samples / SAMPLE_RATE
                            if buf_duration >= MIN_PHRASE_DURATION:
                                log.info("Silence detected, transcribing %.1fs of audio", buf_duration)
                                self._do_complete(np.concatenate(chunks), session,
                                                  peak_rms=utterance_peak, threshold=threshold)
                            chunks = []
                            total_samples = 0
                            utterance_peak = 0.0
                            is_speaking = False
                            silence_start = None

            # Drain any remaining chunks from the queue
            while not self._audio_q.empty():
                try:
                    chunk = self._audio_q.get_nowait()
                    chunks.append(chunk)
                    total_samples += len(chunk)
                    utterance_peak = max(utterance_peak, float(np.sqrt(np.mean(chunk ** 2))))
                except queue.Empty:
                    break
            # The user has stopped: turn the mic off now, not after the final
            # transcription (that can take a minute on a slow PC).
            self._close_stream(stream)

            # Flush all remaining audio on stop (don't require is_speaking —
            # the user releasing the hold key IS the stop signal)
            buf_duration = total_samples / SAMPLE_RATE
            if buf_duration >= MIN_PHRASE_DURATION:
                log.info("Flushing remaining %.1fs of audio on stop", buf_duration)
                self._do_complete(np.concatenate(chunks), session,
                                  peak_rms=utterance_peak, threshold=threshold)
        except Exception as e:
            log.exception("Transcription loop crashed")
            abort_reason = f"Dictation error: {type(e).__name__}: {e}"
        finally:
            with self._lock:
                was_running = self._running
                self._running = False
            self._close_stream(stream)
            if was_running and self._on_abort is not None:
                # The session ended on its own while the app still thought it
                # was live. The session id lets the app ignore aborts that
                # belong to an already-replaced session.
                try:
                    self._on_abort(abort_reason or "Dictation stopped unexpectedly",
                                   session)
                except Exception:
                    log.exception("Session abort callback failed")

    def _deliver(self, session: int, callback, *args) -> None:
        """Deliver a result to the app only if `session` is still current.
        stop() closes the session once its final-flush window is over, so a
        transcription that finishes late can never land in the NEXT
        dictation. The callback runs under the lock so a stop() that closes
        the session can never interleave between the check and the delivery
        (lock order: transcriber._lock before any app-side lock)."""
        with self._lock:
            if session != self._session_id:
                log.info("Dropped result from closed session %d (current: %d)",
                         session, self._session_id)
                return
            if callback is not None:
                callback(*args)

    def _partials_allowed(self, session: int) -> bool:
        """Live preview only when the engine is fast. On a slow PC one partial
        costs as much as the final transcription, so the user would wait for
        both. Before any measurement, trust only a CUDA model host."""
        last = self._last_infer_s
        if last is None:
            allowed = self._host.info.get("device") == "cuda"
        else:
            allowed = last <= PARTIAL_MAX_INFER_S
        if not allowed and self._partials_off_logged != session:
            self._partials_off_logged = session
            if last is None:
                log.info("Live preview off: engine not on CUDA and not measured yet")
            else:
                log.info("Live preview off: last transcription took %.1fs", last)
        return allowed

    def _do_partial(self, audio: np.ndarray, session: int) -> None:
        try:
            t0 = time.perf_counter()
            segments = self._host.transcribe(
                audio, self._settings.language, 1,
                self._settings.initial_prompt or "")
            self._last_infer_s = time.perf_counter() - t0
            text = " ".join(s["text"].strip() for s in segments if s["text"].strip())
            if text:
                # Log the length, never the content — transcripts are
                # private and must not land in eqho.log (F10).
                log.info("Partial transcript: %d chars", len(text))
                self._deliver(session, self._on_partial, text)
        except Exception as e:
            log.error("Partial transcription error: %s", e)

    def _do_complete(self, audio: np.ndarray, session: int,
                     peak_rms: Optional[float] = None,
                     threshold: float = SILENCE_THRESHOLD) -> None:
        if peak_rms is not None and peak_rms < threshold * NEAR_SILENCE_FACTOR:
            log.info("Skipping near-silent buffer (peak RMS %.5f < %.5f) — hallucination bait.",
                     peak_rms, threshold * NEAR_SILENCE_FACTOR)
            return
        try:
            t0 = time.perf_counter()
            segments = self._host.transcribe(
                audio, self._settings.language, 5,
                self._settings.initial_prompt or "")
            infer_dur = time.perf_counter() - t0
            self._last_infer_s = infer_dur
            kept = []
            for s in segments:
                t = s["text"].strip()
                if not t:
                    continue
                if s["no_speech_prob"] > NO_SPEECH_PROB_MAX and s["avg_logprob"] < AVG_LOGPROB_MIN:
                    log.info(
                        "Dropped low-confidence segment (%d chars, no_speech=%.2f logprob=%.2f)",
                        len(t), s["no_speech_prob"], s["avg_logprob"],
                    )
                    continue
                kept.append(t)
            text = " ".join(kept)

            duration = len(audio) / SAMPLE_RATE
            if text and duration < 2.0 and text.lower().strip(' .!?"') in HALLUCINATION_BLOCKLIST:
                log.info("Dropped known hallucination artifact (%d chars)", len(text))
                return

            if text:
                # Length/timing only — never the transcript itself (F10).
                log.info("Complete transcript: %d chars (%.1fs audio, transcribed in %.2fs)",
                         len(text), duration, infer_dur)
                self._deliver(session, self._on_complete, text)
                self._maybe_slow_hint(infer_dur)
        except Exception as e:
            log.error("Transcription error: %s", e)

    def _maybe_slow_hint(self, infer_dur: float) -> None:
        """One tray tip per model, ever, when the model call was slow and a
        better-suited model exists. Never changes the model itself."""
        if infer_dur <= SLOW_HINT_S:
            return
        current = self._settings.model_size
        if getattr(self._settings, "slow_hint_shown_for", "") == current:
            return
        from .modelpick import recommend_model
        from .settings import WHISPER_MODELS
        recommended = recommend_model(self._settings.language)
        if recommended == current:
            return

        def label(key: str) -> str:
            return WHISPER_MODELS.get(key, key).split(" (")[0]

        self._settings.slow_hint_shown_for = current
        self._settings.save()
        log.info("Slow-model tip shown: %s took %.2fs (suggested %s)",
                 current, infer_dur, recommended)
        if self._on_notice:
            self._on_notice(
                f"Eqho is slow on this PC with {label(current)}. "
                f"{label(recommended)} is much faster — you can switch "
                "in Eqho → Models.")

    def stop(self) -> None:
        if not self._running:
            return
        self.finish_stop(self.request_stop(), STOP_JOIN_TIMEOUT)

    def request_stop(self) -> Optional[threading.Thread]:
        """Tell the worker to stop listening and run its final flush. Returns
        at once with the worker (None if nothing was running); the caller
        then MUST call finish_stop() to close the session."""
        with self._lock:
            if not self._running:
                return None
            self._running = False
            worker, self._worker = self._worker, None
        return worker

    def finish_stop(self, worker: Optional[threading.Thread],
                    timeout: float) -> bool:
        """Wait up to `timeout` s for the worker's final flush (the last
        phrase the user spoke), then close the session. Results that still
        arrive afterwards are dropped by the session check in _deliver — they
        can never leak into the next dictation (F2). Returns True when the
        worker finished in time."""
        finished = True
        if worker is not None:
            worker.join(timeout=timeout)
            finished = not worker.is_alive()

        with self._lock:
            self._session_id += 1  # close the session: late results are dropped

        # Close mic AFTER worker is done so no audio chunks are lost
        self._close_stream()

        log.info("Transcription stopped.")
        return finished

    def is_running(self) -> bool:
        return self._running

    def reload_model(self) -> None:
        """Reload after model size change. Seamless — just marks stale; the
        worker's _ensure_model tells the host to kill its child and spawn a
        fresh one with the new model (no app restart, no native crash)."""
        was_running = self._running
        if was_running:
            self.stop()
        with self._model_lock:
            self._model_loaded = False
            self._current_model_size = None
        if was_running:
            self.start()

    def set_engine(self, engine_setting: str) -> bool:
        """Switch the inference engine. Resolves `engine_setting`
        ("auto" | "faster-whisper" | "whisper.cpp") to a concrete backend and,
        if it differs from the one currently running, respawns the model-host
        child on the new backend — the same seamless kill+reload path as a model
        change. Returns True if the backend actually changed."""
        from .model_host import resolve_backend
        new_backend = resolve_backend(engine_setting)
        self._auto_engine = engine_setting not in ("faster-whisper", "whisper.cpp")
        self._auto_fallback_used = False  # a fresh engine choice gets a fresh fallback
        if new_backend == self._host.backend:
            return False
        was_running = self._running
        if was_running:
            self.stop()
        self._host.set_backend(new_backend)
        with self._model_lock:
            self._model_loaded = False
            self._current_model_size = None
        log.info("Inference backend switched to: %s", new_backend)
        if was_running:
            self.start()
        return True

    def shutdown(self) -> None:
        self.stop()
        self._model_loaded = False
        try:
            self._host.shutdown()
        except Exception:
            pass
