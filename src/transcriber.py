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


class VoiceTranscriber:
    """Records from mic, detects speech via energy-based VAD, transcribes with faster-whisper."""

    def __init__(self, settings: Settings):
        self._settings = settings
        from .model_host import ModelHost, resolve_backend
        # The model runs in a child process — switching = kill+respawn a fresh
        # child (the only reliable path; a second in-process CUDA model crashes
        # natively). The main app never loads a model. The engine backend
        # auto-resolves (NVIDIA->faster-whisper, else->whisper.cpp if installed).
        backend = resolve_backend(getattr(settings, "engine_backend", "auto"))
        log.info("Inference backend: %s", backend)
        self._host = ModelHost(backend=backend)
        oskit.get().disable_os_mic_ducking()
        self._on_partial: Optional[Callable[[str], None]] = None
        self._on_complete: Optional[Callable[[str], None]] = None
        self._on_status: Optional[Callable[[str], None]] = None
        self._on_level: Optional[Callable[[float], None]] = None
        self._running = False
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
        self._model_dir = settings.resolve_model_dir()
        os.environ["HF_HUB_CACHE"] = str(self._model_dir / "huggingface")

    def set_callbacks(
        self,
        on_partial: Callable[[str], None],
        on_complete: Callable[[str], None],
        on_status: Optional[Callable[[str], None]] = None,
        on_level: Optional[Callable[[float], None]] = None,
    ) -> None:
        self._on_partial = on_partial
        self._on_complete = on_complete
        self._on_status = on_status
        self._on_level = on_level

    def is_model_ready(self) -> bool:
        return self._host.is_ready(self._settings.model_size)

    def consume_mic_error(self) -> Optional[str]:
        """Return and clear the last microphone error (None if there was none)."""
        err, self._mic_error = self._mic_error, None
        return err

    def _ensure_model(self) -> None:
        with self._model_lock:
            target = self._settings.model_size
            if self._host.is_ready(target):
                self._model_loaded = True
                self._current_model_size = target
                return
            self._model_dir = self._settings.resolve_model_dir()
            phase_start = time.monotonic()
            ok = self._host.load_model(target, self._model_dir)
            if ok:
                self._current_model_size = target
                self._model_loaded = True
                log.info("Model ready: %s %s (%.1fs)", target, self._host.info,
                         time.monotonic() - phase_start)
            else:
                self._model_loaded = False
                raise RuntimeError(f"Model host failed to load {target}")

    def transcribe_once(self, audio: np.ndarray) -> str:
        """Synchronously transcribe a float32 mono 16 kHz array (used by --smoke)."""
        self._ensure_model()
        segs = self._host.transcribe(audio, self._settings.language, 1,
                                     self._settings.initial_prompt or "")
        return " ".join(s["text"].strip() for s in segs if s["text"].strip())

    def _open_stream(self, device: Optional[int]) -> sd.InputStream:
        stream = sd.InputStream(
            samplerate=SAMPLE_RATE,
            channels=1,
            dtype="float32",
            blocksize=int(SAMPLE_RATE * CHUNK_DURATION),
            device=device,
            callback=self._audio_callback,
        )
        stream.start()
        return stream

    def _close_stream(self) -> None:
        with self._stream_lock:
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
            self._worker = threading.Thread(target=self._transcription_loop, daemon=True)
            self._worker.start()
            log.info("Transcription started (mic recording).")

    def _audio_callback(self, indata, frames, time_info, status):
        if status:
            log.warning("Audio callback status: %s", status)
        self._audio_q.put(indata[:, 0].copy())

    def _transcription_loop(self) -> None:
        """Loads the model if needed, then accumulates audio, detects speech/silence, and transcribes phrases."""
        if not self.is_model_ready():
            try:
                self._ensure_model()
            except Exception as e:
                log.error("Model load failed: %s", e)
                if self._on_status:
                    self._on_status("Model failed to load — check logs")
                self._running = False
                self._close_stream()
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
                    # Bounded tail keeps partial latency flat on long phrases;
                    # the final transcription still sees the full buffer.
                    tail = max(1, int(PARTIAL_TAIL_SECONDS / CHUNK_DURATION))
                    self._do_partial(np.concatenate(chunks[-tail:]))
            else:
                if is_speaking:
                    if silence_start is None:
                        silence_start = time.monotonic()
                    elif time.monotonic() - silence_start > SILENCE_TIMEOUT:
                        buf_duration = total_samples / SAMPLE_RATE
                        if buf_duration >= MIN_PHRASE_DURATION:
                            log.info("Silence detected, transcribing %.1fs of audio", buf_duration)
                            self._do_complete(np.concatenate(chunks),
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

        # Flush all remaining audio on stop (don't require is_speaking —
        # the user releasing the hold key IS the stop signal)
        buf_duration = total_samples / SAMPLE_RATE
        if buf_duration >= MIN_PHRASE_DURATION:
            log.info("Flushing remaining %.1fs of audio on stop", buf_duration)
            self._do_complete(np.concatenate(chunks),
                              peak_rms=utterance_peak, threshold=threshold)

    def _do_partial(self, audio: np.ndarray) -> None:
        try:
            segments = self._host.transcribe(
                audio, self._settings.language, 1,
                self._settings.initial_prompt or "")
            text = " ".join(s["text"].strip() for s in segments if s["text"].strip())
            if text and self._on_partial:
                log.info("Partial: %s", text)
                self._on_partial(text)
        except Exception as e:
            log.error("Partial transcription error: %s", e)

    def _do_complete(self, audio: np.ndarray, peak_rms: Optional[float] = None,
                     threshold: float = SILENCE_THRESHOLD) -> None:
        if peak_rms is not None and peak_rms < threshold * NEAR_SILENCE_FACTOR:
            log.info("Skipping near-silent buffer (peak RMS %.5f < %.5f) — hallucination bait.",
                     peak_rms, threshold * NEAR_SILENCE_FACTOR)
            return
        try:
            segments = self._host.transcribe(
                audio, self._settings.language, 5,
                self._settings.initial_prompt or "")
            kept = []
            for s in segments:
                t = s["text"].strip()
                if not t:
                    continue
                if s["no_speech_prob"] > NO_SPEECH_PROB_MAX and s["avg_logprob"] < AVG_LOGPROB_MIN:
                    log.info(
                        "Dropped low-confidence segment %r (no_speech=%.2f logprob=%.2f)",
                        t, s["no_speech_prob"], s["avg_logprob"],
                    )
                    continue
                kept.append(t)
            text = " ".join(kept)

            duration = len(audio) / SAMPLE_RATE
            if text and duration < 2.0 and text.lower().strip(' .!?"') in HALLUCINATION_BLOCKLIST:
                log.info("Dropped known hallucination artifact: %r", text)
                return

            if text and self._on_complete:
                log.info("Complete: %s", text)
                self._on_complete(text)
        except Exception as e:
            log.error("Transcription error: %s", e)

    def stop(self) -> None:
        with self._lock:
            if not self._running:
                return
            self._running = False

        # Wait for worker to finish (it drains the queue and flushes audio)
        if self._worker:
            self._worker.join(timeout=5)
            self._worker = None

        # Close mic AFTER worker is done so no audio chunks are lost
        self._close_stream()

        log.info("Transcription stopped.")

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

    def shutdown(self) -> None:
        self.stop()
        self._model_loaded = False
        try:
            self._host.shutdown()
        except Exception:
            pass
