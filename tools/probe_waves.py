"""Probe: voice-shaped pill waves (EQ-WAVES), headless.

(a) the mic stream is opened with small blocks (50 ms) and _audio_callback
    re-assembles them into the SAME 0.5 s chunks the worker always got
    (bit-identical to the old one-block-per-chunk stream);
(b) one voice-feature pair [loud, tone] arrives per small block;
(c) the EQHO_TEST_AUDIO feeder still queues 0.5 s chunks and now also feeds
    features;
(d) pillfx (EQ-WAVES2B): louder voice → larger curve amplitude, silence →
    near-flat, consecutive frames change smoothly (no jumps), no voice →
    the classic level-only frame; frame time measured.

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

    # (b) one feature pair per block
    check(len(waves) == n_fed > 0, f"{len(waves)} feature pairs, one per block")
    w0 = waves[5]
    check(w0.shape == (2,) and w0.dtype == np.float32, "features = [loud, tone] float32")
    check(all(0.0 <= float(v) <= 1.0 for f in waves for v in f), "features stay in 0..1")
    quiet = tr.voice_features(0.001 * speech(tr.WAVE_BLOCK))
    loud = tr.voice_features(speech(tr.WAVE_BLOCK * 8)[tr.WAVE_BLOCK * 2:tr.WAVE_BLOCK * 3])
    check(loud[0] > quiet[0] + 0.3, f"louder block -> higher loudness ({loud[0]:.2f} vs {quiet[0]:.2f})")
    rate = tr.SAMPLE_RATE
    tt = np.arange(tr.WAVE_BLOCK) / rate
    lo = tr.voice_features((0.1 * np.sin(2 * np.pi * 200 * tt)).astype(np.float32))
    hi = tr.voice_features((0.1 * np.sin(2 * np.pi * 2000 * tt)).astype(np.float32))
    check(hi[1] > lo[1] + 0.3, f"brighter tone -> higher tone ({hi[1]:.2f} vs {lo[1]:.2f})")

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
          "EQHO_TEST_AUDIO feeder: 0.5 s chunk + per-block features")
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


def follow(feats, seconds: float, fps: int = 25):
    """Run VoiceFollower like the overlay: features every 50 ms, frames at fps."""
    fol = pillfx.VoiceFollower()
    out = []
    for i in range(int(seconds * fps)):
        t = i / fps
        b = min(int(t / 0.05), len(feats) - 1)
        fol.feed(*feats[b])
        out.append(fol.step(1.0 / fps))
    return out


def probe_render() -> None:
    bg = (0.0, 0.0, 0.0)
    blk = tr.WAVE_BLOCK
    sp = speech(blk * 40, seed=3)
    f_voice = [tr.voice_features(sp[i:i + blk]) for i in range(0, len(sp), blk)]
    f_soft = [tr.voice_features(0.15 * sp[i:i + blk]) for i in range(0, len(sp), blk)]
    f_sil = [tr.voice_features((0.0005 * np.random.default_rng(4).standard_normal(blk)).astype(np.float32))] * 40
    v_loud = follow(f_voice, 2.0)
    v_soft = follow(f_soft, 2.0)
    v_sil = follow(f_sil, 2.0)

    def amp(states):
        return float(np.mean([np.abs(pillfx.voice_curves(280, *s)[0]).max() for s in states[25:]]))
    a_l, a_s, a_0 = amp(v_loud), amp(v_soft), amp(v_sil)
    check(a_l > a_s * 1.3 > 0, f"louder -> larger amplitude ({a_l:.3f} > {a_s:.3f})")
    check(a_0 < 0.25 * a_l, f"silence near-flat ({a_0:.3f} vs loud {a_l:.3f})")

    # smooth frame to frame: curve and loudness never jump, phase continuous
    seq = follow(f_sil[:10] + f_voice + f_sil, 4.0)
    curves = [pillfx.voice_curves(280, *s)[0] for s in seq]
    jump = max(float(np.abs(b - a).max()) for a, b in zip(curves, curves[1:]))
    check(jump < 0.08, f"consecutive frames change smoothly (max step {jump:.3f} r)")
    dph = np.diff([s[2] for s in seq])
    check(bool((dph > 0).all() and dph.max() < 0.15), "phase integrates continuously")
    # roundness: curve has at most a few crests (no jagged detail)
    m = curves[60]
    crests = int(((m[1:-1] > m[:-2]) & (m[1:-1] > m[2:])).sum())
    check(crests <= 3, f"at most 3 crests across the pill ({crests})")

    for w, h in ((168, 31), (224, 41), (280, 51), (392, 71)):
        for light in (0.0, 1.0):
            tag = f"{w}x{h} {'light' if light else 'dark'}"
            fv = pillfx.render_rgb(w, h, 1.3, 0.5, light, bg, v_loud[40])
            fs = pillfx.render_rgb(w, h, 1.3, 0.5, light, bg, v_sil[40])
            fn = pillfx.render_rgb(w, h, 1.3, 0.5, light, bg)
            diff = np.abs(fv.astype(int) - fs.astype(int)).mean()
            check(diff > 1.0 and fn.shape == (h, w, 3), f"{tag}: voice frame differs from silent (|d|={diff:.1f})")
            dc = np.abs(np.diff(centroid(fv, light)[w // 8:-w // 8])).max()
            check(dc < 0.08 * h, f"{tag}: line slope gentle (max {dc:.2f} px/col)")

    for w, h in ((224, 41), (280, 51)):
        pillfx.render_ppm(w, h, 0.0, 0.5, 0.0, bg, v_loud[0])
        n = 60
        t0 = time.perf_counter()
        for i in range(n):
            pillfx.render_ppm(w, h, i * 0.04, 0.5, float(i % 2), bg, v_loud[i % len(v_loud)])
        ms = (time.perf_counter() - t0) / n * 1000
        t0 = time.perf_counter()
        for i in range(n):
            pillfx.render_ppm(w, h, i * 0.04, 0.5, float(i % 2), bg)
        ms0 = (time.perf_counter() - t0) / n * 1000
        print(f"FRAME {w}x{h}: {ms:.2f} ms with voice, {ms0:.2f} ms level-only")
        check(ms < 10.0, f"{w}x{h} frame under 10 ms ({ms:.2f} ms)")


def main() -> int:
    probe_chunks()
    probe_render()
    print("\n%d failure(s)" % len(FAILS))
    return 1 if FAILS else 0


if __name__ == "__main__":
    sys.exit(main())
