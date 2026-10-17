"""Iridescent pill renderer for the dictation overlay (the v0.8.3 look).

Pure numpy — frames go to tkinter as binary PPM (P6) via ``PhotoImage(data=...)``,
no Pillow in the hot path. The math mirrors Eqho Mobile's AGSL shader
(``RecordingPill.android.kt``) and the reference GIFs in ``ref/anim``: a
breathing glass capsule with undulating chromatic-fringed ribbons, a caustic
arc along the lower edge, a fresnel rim and a glassy sheen from the upper
left. Dark theme = additive glow; light theme = soft pigment on frosted pearl
(additive pastel just washes out to white).

Keep the constants in sync with the mobile shader when tuning — same look on
every surface Eqho ships.

Voice-shaped lines (0.8.11, desktop first): with ``voice`` (smoothed loudness,
tone and an integrated phase) the ribbons flow like water — a few soft sines
that swell with the voice and settle nearly flat in silence. The mobile
shader does not do this yet — it should follow later (same ``voice_curves``).
"""

import numpy as np

__all__ = ["render_rgb", "render_ppm", "hex_rgb01", "voice_curves", "VoiceFollower"]


def hex_rgb01(hex_color: str) -> tuple[float, float, float]:
    """'#rrggbb' → (r, g, b) in 0..1."""
    return tuple(int(hex_color[i:i + 2], 16) / 255.0 for i in (1, 3, 5))


def _gs(x: np.ndarray, s: float) -> np.ndarray:
    return np.exp(-(x * x) / (2.0 * s * s))


def _mix(a, b, t):
    return a + (b - a) * t


class VoiceFollower:
    """Live voice state for the pill: only the CURRENT voice, lightly smoothed.

    ``feed(loud, tone)`` takes one block's features (any thread — a plain
    assignment); ``step(dt)`` (render thread) eases toward them — ~150 ms
    attack, ~450 ms release, tone slower still — and integrates the drift
    phase (faster with more voice). Returns ``(loud, tone, phase)`` for
    ``render_rgb(voice=...)``. Features older than ``stale`` s count as
    silence, so the lines settle when the mic stops."""

    def __init__(self, attack: float = 0.15, release: float = 0.45, stale: float = 0.3):
        self.attack, self.release, self.stale = attack, release, stale
        self._target = (0.0, 0.5)
        self._age = 1e9
        self.loud, self.tone, self.phase = 0.0, 0.5, 0.0

    def feed(self, loud: float, tone: float) -> None:
        self._target = (float(loud), float(tone))
        self._age = 0.0

    def reset(self) -> None:
        self._target, self._age = (0.0, 0.5), 1e9
        self.loud, self.tone = 0.0, 0.5

    def step(self, dt: float):
        dt = min(max(float(dt), 0.0), 0.1)
        self._age += dt
        loud, tone = self._target if self._age <= self.stale else (0.0, self.tone)
        tau = self.attack if loud > self.loud else self.release
        self.loud += (loud - self.loud) * (1.0 - np.exp(-dt / tau))
        if loud > 0.08:  # tone only means something while there is voice
            self.tone += (tone - self.tone) * (1.0 - np.exp(-dt / 0.30))
        self.phase += dt * (0.55 + 2.2 * self.loud)
        return self.loud, self.tone, self.phase


def voice_curves(w: int, loud: float, tone: float, phase: float):
    """Per-column displacement (w,) of the main and the second ribbon, in
    units of the capsule radius — water-like, never sharp.

    A sum of three low-frequency sines (about 1-3 gentle crests across the
    pill), tapered at both ends. ``loud`` 0..1 (smoothed loudness) raises the
    amplitude along an eased, capped curve; ``tone`` 0..1 (smoothed spectral
    brightness) tightens the waves a little (higher voice → livelier, lower →
    longer swells); ``phase`` is integrated by the caller (continuous, faster
    with more voice), so nothing ever jumps. The second ribbon follows with an
    offset phase and a smaller amplitude for depth (not a hard mirror)."""
    loud = min(max(float(loud), 0.0), 1.0)
    tone = min(max(float(tone), 0.0), 1.0)
    xn = np.linspace(-1.0, 1.0, w, dtype=np.float32)
    taper = np.sin((xn + 1.0) * (np.pi / 2)) ** 0.8
    e = loud * loud * (3.0 - 2.0 * loud)          # smoothstep: soft start, soft cap
    amp = 0.05 + 0.50 * e                        # radius units; silence ≈ flat
    k = 1.0 + 0.55 * tone                        # wavelength shift by tone
    p = phase
    main = (np.sin(xn * (np.pi * 1.10 * k) + p)
            + 0.45 * np.sin(xn * (np.pi * 1.85 * k) - p * 1.30 + 0.8)
            + 0.20 * np.sin(xn * (np.pi * 2.70 * k) + p * 0.70 + 2.1)) / 1.65
    second = (np.sin(xn * (np.pi * 1.30 * k) + p * 0.85 + 1.9)
              + 0.40 * np.sin(xn * (np.pi * 2.05 * k) - p * 1.10 + 3.0)) / 1.40
    return ((main * taper) * amp).astype(np.float32), ((second * taper) * (amp * 0.6)).astype(np.float32)


def render_rgb(
    w: int,
    h: int,
    t: float,
    level: float,
    light: float,
    bg: tuple[float, float, float],
    voice=None,
) -> np.ndarray:
    """One frame as uint8 (h, w, 3). ``light`` is 0 (dark theme) → 1 (light);
    ``level`` is the smoothed mic level 0..1; ``bg`` fills outside the capsule;
    ``voice`` (optional ``(loud, tone, phase)``) shapes the lines along x —
    without it the pill animates from ``level`` alone, as before."""
    ys, xs = np.mgrid[0:h, 0:w].astype(np.float32)
    uvx = xs / w - 0.5
    uvy = ys / h - 0.5
    aspect = w / h
    uvx *= aspect

    lvl = float(max(0.0, min(1.0, level)))

    r = 0.36 + 0.015 * np.sin(t * 1.6) + 0.045 * lvl
    hw = max(aspect * 0.5 - r - 0.05, 0.0)
    px = uvx - np.clip(uvx, -hw, hw)
    py = uvy
    d = np.sqrt(px * px + py * py) - r

    qx, qy = px / r, py / r
    nz = np.sqrt(np.maximum(1.0 - (qx * qx + qy * qy), 0.0))
    nlen = np.sqrt(qx * qx + qy * qy + np.maximum(nz, 1e-3) ** 2)
    n_x, n_y, n_z = qx / nlen, qy / nlen, np.maximum(nz, 1e-3) / nlen

    def v3(x: float, y: float, z: float) -> np.ndarray:
        return np.array([x, y, z], dtype=np.float32)[None, None, :]

    def paint(col, target, factor):
        """mix(col, target, factor) with a per-pixel (h,w,3) factor."""
        return col + (target - col) * factor

    # Brand-blue palette (theme ACCENT #58a6ff / light #0969da): navy body,
    # indigo drift, azure + ice ribbons, electric arc — blues only, no purple.
    body = _mix(v3(0.035, 0.060, 0.140), v3(0.955, 0.970, 0.995), light)
    tint = _mix(v3(0.080, 0.170, 0.420), v3(0.800, 0.890, 0.990), light)

    wx = uvx + 0.22 * np.sin(1.7 * uvy + t * 0.33)
    wy = uvy + 0.22 * np.sin(1.9 * uvx + t * 0.27)
    drift = 0.5 + 0.5 * np.sin(2.1 * wx + 2.6 * wy + t * 0.21)
    col = body + (tint - body) * (drift * 0.55)[..., None]

    amp = r * (0.19 + 0.22 * lvl)
    th = r * (0.17 + 0.10 * lvl)
    fr = r * (0.050 + 0.045 * lvl)

    if voice is not None:
        # Water-like: soft low-frequency swells driven by the voice features.
        loud, tone, vph = voice
        m1, m2 = voice_curves(w, loud, tone, vph)
        c1 = (m1 * r)[None, :]
        c2 = (m2 * r)[None, :] - r * 0.16
        th = r * (0.17 + 0.08 * float(loud))
    else:
        c1 = amp * np.sin(uvx * 2.6 - t * 1.15) + amp * 0.45 * np.sin(uvx * 4.9 + t * 0.70)
        c2 = amp * 0.75 * np.sin(uvx * 3.4 + t * 0.90 + 1.7) - r * 0.16
    y1 = uvy - c1
    b1 = np.stack([_gs(y1 - fr, th), _gs(y1, th), _gs(y1 + fr, th)], axis=-1)
    col = col + b1 * v3(0.250, 0.550, 1.000) * ((1.0 - light) * 0.80)
    col = paint(col, v3(0.100, 0.420, 0.850), b1 * (light * 0.78))

    y2 = uvy - c2
    b2 = np.stack([_gs(y2 + fr, th * 0.55), _gs(y2, th * 0.55), _gs(y2 - fr, th * 0.55)], axis=-1)
    col = col + b2 * v3(0.450, 0.850, 1.000) * ((1.0 - light) * 0.35)
    col = paint(col, v3(0.150, 0.620, 0.920), b2 * (light * 0.60))

    ad = d + r * 0.24
    aw = r * 0.085
    lower = np.clip((qy - 0.05) / (0.6 - 0.05), 0.0, 1.0)
    lower = lower * lower * (3.0 - 2.0 * lower)
    pulse = 0.6 + 0.4 * np.sin(t * 0.8 + uvx * 1.7)
    arc = np.stack([_gs(ad - fr, aw), _gs(ad, aw), _gs(ad + fr, aw)], axis=-1)
    col = col + arc * v3(0.250, 0.450, 1.000) * ((lower * pulse)[..., None]) * (1.0 - light) * 0.85
    col = paint(col, v3(0.250, 0.500, 0.920), arc * ((lower * pulse)[..., None]) * (light * 0.42))

    fres = (1.0 - np.clip(n_z, 0.0, 1.0)) ** 2.3
    col = col + v3(0.300, 0.550, 1.000) * (fres * (1.0 - light) * 0.65 * (0.7 + 0.5 * lvl))[..., None]
    col = paint(col, v3(0.450, 0.620, 0.920), (fres * light * 0.55)[..., None])

    ld = np.array([-0.45, -0.60, 0.66], dtype=np.float32)
    ld /= np.linalg.norm(ld)
    ndl = np.maximum(n_x * ld[0] + n_y * ld[1] + n_z * ld[2], 0.0)
    # Full-strength sheen in BOTH themes — the light pill shines like the dark one.
    sheen = (ndl ** 64.0) * 0.42 + (ndl ** 6.0) * 0.12
    col = col + sheen[..., None] * v3(1.0, 0.97, 0.98) * _mix(1.0, 0.95, light)

    # Light theme stays vibrant — only a whisper of frost (0.16 washed it out).
    col = paint(col, v3(0.975, 0.975, 0.990), light * 0.06)

    mapped = 1.0 - np.exp(-col * 1.25)
    col = _mix(mapped, np.clip(col, 0.0, 1.0), light)

    aa = 1.5 / h
    # White border on the light pill (applied after tone-mapping so it stays crisp).
    bw = 3.0 / h
    stroke = np.clip((d + bw) / aa, 0.0, 1.0) * np.clip(-d / aa, 0.0, 1.0)
    col = paint(col, v3(0.995, 0.995, 1.000), (stroke * light * 0.85)[..., None])

    # NOTE: desktop translucency (the light pill at 90%) is applied as WINDOW
    # alpha in overlay.py — the pill sits on a color-keyed window, so blending
    # toward `bg` here would blend toward the key color, not the desktop.
    mask = np.clip(1.0 - np.clip((d + aa) / (2 * aa), 0.0, 1.0), 0.0, 1.0)[..., None]
    bg_arr = np.array(bg, dtype=np.float32)
    out = bg_arr[None, None, :] * (1.0 - mask) + col * mask
    return (np.clip(out, 0.0, 1.0) * 255).astype(np.uint8)


def render_ppm(
    w: int,
    h: int,
    t: float,
    level: float,
    light: float,
    bg: tuple[float, float, float],
    voice=None,
) -> bytes:
    """One frame as a binary PPM — feed straight to ``tk.PhotoImage(data=...)``."""
    arr = render_rgb(w, h, t, level, light, bg, voice)
    return b"P6 %d %d 255\n" % (w, h) + arr.tobytes()
