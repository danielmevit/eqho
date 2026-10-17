"""Probe: voice-shaped pill waves (EQ-WAVES), headless.

(a) the mic stream is opened with small blocks (50 ms) and _audio_callback
    re-assembles them into the SAME 0.5 s chunks the worker always got
    (bit-identical to the old one-block-per-chunk stream);
(b) one downsampled waveform arrives per small block, peak-preserving;
(c) the EQHO_TEST_AUDIO feeder still queues 0.5 s chunks and now also feeds
    waveforms;
(d) pillfx: a frame with a synthetic voice waveform differs from the silent
    frame, its line profile correlates with the input, silence → near-flat,
    no waveform → the classic level-only frame; frame time measured.

Exit 0 on success, 1 on any failure.

    venv/Scripts/python.exe tools/probe_waves.py
"""

import queue
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import _probe_env  # noqa: F401,E402  isolated EQHO_CONFIG_DIR before any src import

import numpy as np  # noqa: E402

import src.transcriber as tr  # noqa: E402
from src import pillfx  # noqa: E402

FAILS: list[str] = []


def check(cond: bool, label: str) -> None:
    print(("PASS " if cond else "FAIL ") + label)
    if not cond:
        FAILS.append(label)


def speech(n: int, seed: int = 1) -> np.ndarray:
    """Speech-like signal: 140 Hz glottal pulse train + formants, syllable envelope."""
    rng = np.random.default_rng(seed)
    t = np.arange(n) / tr.SAMPLE_RATE
    f0 = 140 + 20 * np.sin(2 * np.pi * 0.7 * t)
    ph = 2 * np.pi * np.cumsum(f0) / tr.SAMPLE_RATE
    voiced = sum(np.sin(k * ph) / k for k in range(1, 8))
    voiced += 0.5 * np.sin(2 * np.pi * 700 * t) * np.sin(ph) ** 2
    env = np.clip(np.sin(2 * np.pi * 4 * t), 0, 1) ** 0.5
    return (0.12 * voiced * env + 0.002 * rng.standard_normal(n)).astype(np.float32)


def bare_transcriber() -> tr.VoiceTranscriber:
    """A VoiceTranscriber with only the audio-path state (no model host)."""
    vt = tr.VoiceTranscriber.__new__(tr.VoiceTranscriber)
    vt._audio_q = queue.Queue()
    vt._chunk_parts = []
    vt._chunk_fill = 0
    vt._on_waveform = None
    return vt


def probe_chunks() -> None:
    chunk_len = int(tr.SAMPLE_RATE * tr.CHUNK_DURATION)
    check(tr.CHUNK_DURATION == 0.5, "CHUNK_DURATION semantics unchanged (0.5 s)")
    check(chunk_len % tr.WAVE_BLOCK == 0, f"WAVE_BLOCK {tr.WAVE_BLOCK} divides the chunk")

    # (a) the stream is opened with small blocks
    opened = {}

    class FakeStream:
        def __init__(self, **kw):
            opened.update(kw)

        def start(self):
            pass

    real = tr.sd.InputStream
    tr.sd.InputStream = FakeStream
    try:
        vt = bare_transcriber()
        tr.os.environ.pop("EQHO_TEST_AUDIO", None)
        vt._open_stream(None)
    finally:
        tr.sd.InputStream = real
    check(opened.get("blocksize") == tr.WAVE_BLOCK,
          f"mic opened with blocksize={opened.get('blocksize')} (50 ms)")

    # Fake mic: 3.2 s of speech in small blocks (and one odd-sized block).
    audio = speech(int(3.2 * tr.SAMPLE_RATE))
    vt = bare_transcriber()
    waves = []
    vt._on_waveform = waves.append
    sizes = [tr.WAVE_BLOCK] * 20 + [300, tr.WAVE_BLOCK - 300] + [tr.WAVE_BLOCK] * 100
    pos = 0
    n_fed = 0
    for sz in sizes:
        if pos + sz > len(audio):
            break
        vt._audio_callback(audio[pos:pos + sz, None], sz, None, None)
        pos += sz
        n_fed += 1
    got = []
    while not vt._audio_q.empty():
        got.append(vt._audio_q.get_nowait())
    old = [audio[i:i + chunk_len] for i in range(0, pos - chunk_len + 1, chunk_len)]
    check(len(got) == len(old) == pos // chunk_len, f"{len(got)} chunks for {pos} samples")
    check(all(len(c) == chunk_len for c in got), "every chunk is exactly 0.5 s")
    check(all(np.array_equal(a, b) for a, b in zip(got, old)),
          "chunks bit-identical to the old 0.5 s blocks")

    # (b) one waveform per block, peak-preserving
    check(len(waves) == n_fed > 0, f"{len(waves)} waveforms, one per block")
    w0 = waves[5]
    blk = audio[5 * tr.WAVE_BLOCK:6 * tr.WAVE_BLOCK]
    check(w0.shape == (tr.WAVE_POINTS,) and w0.dtype == np.float32, "waveform = 48 float32 points")
    check(abs(np.max(np.abs(w0)) - np.max(np.abs(blk))) < 1e-6, "downsample keeps the block peak")

    # a raising waveform callback must not break the audio path
    vt2 = bare_transcriber()
    vt2._on_waveform = lambda w: 1 / 0
    vt2._audio_callback(audio[:chunk_len, None], chunk_len, None, None)
    check(vt2._audio_q.qsize() == 1, "callback errors never reach the audio thread")

    # (c) test feeder
    q: queue.Queue = queue.Queue()
    fw = []
    src = tr.TestAudioSource("unused.wav", q, on_block=fw.append)
    src._put(audio[:chunk_len].copy())
    check(q.qsize() == 1 and len(q.get()) == chunk_len and len(fw) == chunk_len // tr.WAVE_BLOCK,
          "EQHO_TEST_AUDIO feeder: 0.5 s chunk + per-block waveforms")
    src2 = tr.TestAudioSource("unused.wav", q)  # old signature still works
    src2._put(audio[:chunk_len].copy())
    check(q.qsize() == 1, "TestAudioSource without a waveform hook unchanged")


def centroid(img: np.ndarray, light: float) -> np.ndarray:
    """Per-column y-centroid of the ribbon ink (glow on dark, pigment on light)."""
    lum = img.astype(np.float32).mean(axis=2)
    if light:
        lum = 255.0 - lum
    ys = np.arange(img.shape[0], dtype=np.float32)[:, None]
    return (lum * ys).sum(0) / np.maximum(lum.sum(0), 1e-6)


def probe_render() -> None:
    bg = (0.0, 0.0, 0.0)
    voice = np.concatenate([tr.downsample_wave(b) for b in
                            speech(2 * tr.WAVE_BLOCK, seed=3).reshape(2, -1)])
    silent = np.concatenate([tr.downsample_wave(b) for b in
                             (0.001 * np.random.default_rng(4).standard_normal(
                                 2 * tr.WAVE_BLOCK)).astype(np.float32).reshape(2, -1)])
    for w, h in ((224, 41), (280, 51)):
        for light in (0.0, 1.0):
            tag = f"{w}x{h} {'light' if light else 'dark'}"
            f_none = pillfx.render_rgb(w, h, 1.3, 0.5, light, bg)
            f_sil = pillfx.render_rgb(w, h, 1.3, 0.5, light, bg, silent)
            f_voice = pillfx.render_rgb(w, h, 1.3, 0.5, light, bg, voice)
            diff = np.abs(f_voice.astype(int) - f_sil.astype(int)).mean()
            check(diff > 3.0, f"{tag}: voice frame differs from silent (mean |d|={diff:.1f})")
            prof = pillfx.wave_profile(voice, w)
            dc = centroid(f_voice, light) - centroid(f_sil, light)
            mid = slice(w // 8, w - w // 8)
            corr = float(np.corrcoef(prof[mid], dc[mid])[0, 1])
            check(corr > 0.5, f"{tag}: line profile correlates with the input (r={corr:.2f})")
            check(f_none.shape == (h, w, 3), f"{tag}: level-only frame still renders")
    # silence → near-flat
    sp = pillfx.wave_profile(silent, 280)
    vp = pillfx.wave_profile(voice, 280)
    check(np.abs(sp).max() < 0.05 * max(np.abs(vp).max(), 1e-6) + 0.02,
          f"silence near-flat (max {np.abs(sp).max():.3f} vs voice {np.abs(vp).max():.3f})")
    ref = np.corrcoef(np.interp(np.linspace(0, len(voice) - 1, 280), np.arange(len(voice)), voice),
                      vp)[0, 1]
    check(ref > 0.6, f"profile follows the raw waveform (r={ref:.2f})")

    # frame time
    for w, h in ((224, 41), (280, 51)):
        pillfx.render_ppm(w, h, 0.0, 0.5, 0.0, bg, voice)
        n = 60
        t0 = time.perf_counter()
        for i in range(n):
            pillfx.render_ppm(w, h, i * 0.04, 0.5, float(i % 2), bg, voice)
        ms = (time.perf_counter() - t0) / n * 1000
        t0 = time.perf_counter()
        for i in range(n):
            pillfx.render_ppm(w, h, i * 0.04, 0.5, float(i % 2), bg)
        ms0 = (time.perf_counter() - t0) / n * 1000
        print(f"FRAME {w}x{h}: {ms:.2f} ms with waveform, {ms0:.2f} ms level-only")
        check(ms < 10.0, f"{w}x{h} frame well under 10 ms ({ms:.2f} ms)")


def main() -> int:
    probe_chunks()
    probe_render()
    print("\n%d failure(s)" % len(FAILS))
    return 1 if FAILS else 0


if __name__ == "__main__":
    sys.exit(main())
