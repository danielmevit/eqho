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
"""

import numpy as np

__all__ = ["render_rgb", "render_ppm", "hex_rgb01"]


def hex_rgb01(hex_color: str) -> tuple[float, float, float]:
    """'#rrggbb' → (r, g, b) in 0..1."""
    return tuple(int(hex_color[i:i + 2], 16) / 255.0 for i in (1, 3, 5))


def _gs(x: np.ndarray, s: float) -> np.ndarray:
    return np.exp(-(x * x) / (2.0 * s * s))


def _mix(a, b, t):
    return a + (b - a) * t


def render_rgb(
    w: int,
    h: int,
    t: float,
    level: float,
    light: float,
    bg: tuple[float, float, float],
) -> np.ndarray:
    """One frame as uint8 (h, w, 3). ``light`` is 0 (dark theme) → 1 (light);
    ``level`` is the smoothed mic level 0..1; ``bg`` fills outside the capsule."""
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

    body = _mix(v3(0.055, 0.050, 0.170), v3(0.965, 0.965, 0.985), light)
    tint = _mix(v3(0.160, 0.130, 0.380), v3(0.900, 0.950, 0.860), light)

    wx = uvx + 0.22 * np.sin(1.7 * uvy + t * 0.33)
    wy = uvy + 0.22 * np.sin(1.9 * uvx + t * 0.27)
    drift = 0.5 + 0.5 * np.sin(2.1 * wx + 2.6 * wy + t * 0.21)
    col = body + (tint - body) * (drift * 0.55)[..., None]

    amp = r * (0.19 + 0.22 * lvl)
    th = r * (0.17 + 0.10 * lvl)
    fr = r * (0.050 + 0.045 * lvl)

    c1 = amp * np.sin(uvx * 2.6 - t * 1.15) + amp * 0.45 * np.sin(uvx * 4.9 + t * 0.70)
    y1 = uvy - c1
    b1 = np.stack([_gs(y1 - fr, th), _gs(y1, th), _gs(y1 + fr, th)], axis=-1)
    col = col + b1 * v3(0.560, 0.420, 0.980) * (1.0 - light)
    col = paint(col, v3(0.630, 0.570, 0.880), b1 * (light * 0.60))

    c2 = amp * 0.75 * np.sin(uvx * 3.4 + t * 0.90 + 1.7) - r * 0.16
    y2 = uvy - c2
    b2 = np.stack([_gs(y2 + fr, th * 0.55), _gs(y2, th * 0.55), _gs(y2 - fr, th * 0.55)], axis=-1)
    col = col + b2 * v3(0.250, 0.780, 0.980) * ((1.0 - light) * 0.45)
    col = paint(col, v3(0.550, 0.780, 0.880), b2 * (light * 0.45))

    ad = d + r * 0.24
    aw = r * 0.085
    lower = np.clip((qy - 0.05) / (0.6 - 0.05), 0.0, 1.0)
    lower = lower * lower * (3.0 - 2.0 * lower)
    pulse = 0.6 + 0.4 * np.sin(t * 0.8 + uvx * 1.7)
    arc = np.stack([_gs(ad - fr, aw), _gs(ad, aw), _gs(ad + fr, aw)], axis=-1)
    col = col + arc * v3(0.950, 0.350, 0.750) * ((lower * pulse)[..., None]) * (1.0 - light) * 0.80
    col = paint(col, v3(0.760, 0.700, 0.880), arc * ((lower * pulse)[..., None]) * (light * 0.30))

    fres = (1.0 - np.clip(n_z, 0.0, 1.0)) ** 2.3
    col = col + v3(0.400, 0.420, 1.000) * (fres * (1.0 - light) * 0.85 * (0.7 + 0.5 * lvl))[..., None]
    col = paint(col, v3(0.780, 0.800, 0.940), (fres * light * 0.45)[..., None])

    ld = np.array([-0.45, -0.60, 0.66], dtype=np.float32)
    ld /= np.linalg.norm(ld)
    ndl = np.maximum(n_x * ld[0] + n_y * ld[1] + n_z * ld[2], 0.0)
    sheen = (ndl ** 64.0) * 0.55 + (ndl ** 6.0) * 0.15
    col = col + sheen[..., None] * v3(1.0, 0.97, 0.98) * _mix(1.0, 0.6, light)

    col = paint(col, v3(0.975, 0.975, 0.990), light * 0.16)

    mapped = 1.0 - np.exp(-col * 1.4)
    col = _mix(mapped, np.clip(col, 0.0, 1.0), light)

    aa = 1.5 / h
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
) -> bytes:
    """One frame as a binary PPM — feed straight to ``tk.PhotoImage(data=...)``."""
    arr = render_rgb(w, h, t, level, light, bg)
    return b"P6 %d %d 255\n" % (w, h) + arr.tobytes()
