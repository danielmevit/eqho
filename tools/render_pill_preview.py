"""Preview: ~4 s of the voice pill for a synthetic voice, as GIFs (EQ-WAVES2B).

Silence → soft speech → loud speech → silence, pitch moving. Runs the real
path: 50 ms blocks → transcriber.voice_features → pillfx.VoiceFollower →
pillfx.render_rgb. Writes _test/pill-preview-dark.gif and …-light.gif
(default pill size), plus a contact sheet _test/pill-preview-frames.png.

    venv/Scripts/python.exe tools/render_pill_preview.py
"""

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
import _probe_env  # noqa: F401,E402  isolated EQHO_CONFIG_DIR before any src import

import numpy as np  # noqa: E402
from PIL import Image  # noqa: E402

import src.transcriber as tr  # noqa: E402
from src import pillfx  # noqa: E402

W, H = 280, 51          # default pill (100 %)
FPS = 25
SECONDS = 4.0
BG = {0.0: (0.04, 0.05, 0.08), 1.0: (0.93, 0.94, 0.96)}


def synthetic_voice() -> np.ndarray:
    """Silence 0.6 s → soft speech → loud speech → silence; pitch glides."""
    rate = tr.SAMPLE_RATE
    n = int(SECONDS * rate)
    t = np.arange(n) / rate
    f0 = 120 + 90 * (0.5 + 0.5 * np.sin(2 * np.pi * 0.35 * t))  # 120-210 Hz
    ph = 2 * np.pi * np.cumsum(f0) / rate
    voiced = sum(np.sin(k * ph) / k for k in range(1, 9))
    voiced += 0.4 * np.sin(2 * np.pi * (600 + 900 * (t / SECONDS)) * t) * np.sin(ph) ** 2
    syll = np.clip(np.sin(2 * np.pi * 3.5 * t), 0, 1) ** 0.5
    gain = np.interp(t, [0, 0.6, 0.7, 1.6, 1.8, 3.0, 3.2, SECONDS],
                     [0, 0, 0.03, 0.03, 0.18, 0.18, 0, 0])
    rng = np.random.default_rng(7)
    return (gain * voiced * syll + 0.001 * rng.standard_normal(n)).astype(np.float32)


def frames(light: float) -> list[Image.Image]:
    audio = synthetic_voice()
    blk = tr.WAVE_BLOCK
    feats = [tr.voice_features(audio[i:i + blk]) for i in range(0, len(audio) - blk + 1, blk)]
    fol = pillfx.VoiceFollower()
    out = []
    dt = 1.0 / FPS
    t = 0.0
    for f in range(int(SECONDS * FPS)):
        b = int(t / 0.05)
        if b < len(feats):
            fol.feed(*feats[b])
        loud, tone, ph = fol.step(dt)
        arr = pillfx.render_rgb(W, H, t, loud * 0.6, light, BG[light], (loud, tone, ph))
        out.append(Image.fromarray(arr).resize((W * 2, H * 2), Image.LANCZOS))
        t += dt
    return out


def main() -> int:
    outdir = ROOT / "_test"
    outdir.mkdir(exist_ok=True)
    sheet_rows = []
    for light, name in ((0.0, "dark"), (1.0, "light")):
        fr = frames(light)
        path = outdir / f"pill-preview-{name}.gif"
        fr[0].save(path, save_all=True, append_images=fr[1:], duration=int(1000 / FPS), loop=0)
        print(f"wrote {path} ({len(fr)} frames)")
        sheet_rows.append([fr[i] for i in range(0, len(fr), 10)])
    cols = len(sheet_rows[0])
    fw, fh = sheet_rows[0][0].size
    sheet = Image.new("RGB", (fw, fh * cols * 2))
    for r, row in enumerate(sheet_rows):
        for c, im in enumerate(row):
            sheet.paste(im, (0, (r * cols + c) * fh))
    sheet.save(outdir / "pill-preview-frames.png")
    return 0


if __name__ == "__main__":
    sys.exit(main())
