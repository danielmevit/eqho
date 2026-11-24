#!/usr/bin/env python3
"""
packaging/windows/generate_assets.py

Generates Microsoft Store MSIX visual assets (scale and targetsize variants,
including unplated and light-theme unplated variants) from Eqho's highest-resolution
square app icon without modifying any source icons.

Source icon selection order:
  1. assets/eqho.ico (if it contains a 256x256+ square RGBA frame)
  2. assets/eqho.icns (largest 256x256+ square RGBA frame — 1024x1024)
  3. assets/logo_64_dark.png (last-resort fallback)

Usage:
    python3 packaging/windows/generate_assets.py
    python3 packaging/windows/generate_assets.py --check
"""

from __future__ import annotations

import argparse
import io
import math
import sys
from pathlib import Path

from PIL import Image

SCALES: tuple[int, ...] = (100, 125, 150, 200, 400)
TARGET_SIZES: tuple[int, ...] = (16, 20, 24, 32, 40, 48, 64, 256)
MIN_PREFERRED_SIZE = 256


def ceil_scale(base: int, scale_pct: int) -> int:
    return math.ceil(base * scale_pct / 100.0)


def fit_square_rgba(image: Image.Image) -> Image.Image:
    rgba = image.convert("RGBA")
    w, h = rgba.size
    if w == h:
        return rgba
    side = max(w, h)
    canvas = Image.new("RGBA", (side, side), (0, 0, 0, 0))
    canvas.paste(rgba, ((side - w) // 2, (side - h) // 2), rgba)
    return canvas


def _try_load_ico(path: Path, min_size: int) -> Image.Image | None:
    if not path.is_file():
        return None
    with Image.open(path) as im:
        sizes = sorted(
            (s for s in im.info.get("sizes", set()) if s[0] == s[1]),
            key=lambda s: s[0],
            reverse=True,
        )
        if not sizes or sizes[0][0] < min_size:
            return None
        best_size = sizes[0]
        ico_plugin = getattr(im, "ico", None)
        if ico_plugin is not None:
            frame = ico_plugin.getimage(best_size)
            return fit_square_rgba(frame)
        return fit_square_rgba(im)


def _try_load_icns(path: Path, min_size: int) -> Image.Image | None:
    if not path.is_file():
        return None
    with Image.open(path) as im:
        # Each entry in icns info["sizes"] is (width, height, scale).
        raw_sizes = im.info.get("sizes", [])
        candidates = [
            (entry[0] * entry[2], entry)
            for entry in raw_sizes
            if len(entry) == 3 and entry[0] == entry[1]
        ]
        if candidates:
            candidates.sort(key=lambda item: item[0], reverse=True)
            best_px, best_entry = candidates[0]
            if best_px < min_size:
                return None
            icns_plugin = getattr(im, "icns", None)
            if icns_plugin is not None:
                frame = icns_plugin.getimage(best_entry)
                return fit_square_rgba(frame)
        rgba = fit_square_rgba(im)
        if rgba.width >= min_size:
            return rgba
        return None


def load_master_icon(repo_root: Path) -> tuple[Image.Image, str]:
    ico_path = repo_root / "assets" / "eqho.ico"
    icns_path = repo_root / "assets" / "eqho.icns"
    png_fallback = repo_root / "assets" / "logo_64_dark.png"

    ico_img = _try_load_ico(ico_path, MIN_PREFERRED_SIZE)
    if ico_img is not None:
        return ico_img, f"{ico_path.relative_to(repo_root)} ({ico_img.width}x{ico_img.height})"

    icns_img = _try_load_icns(icns_path, MIN_PREFERRED_SIZE)
    if icns_img is not None:
        return icns_img, f"{icns_path.relative_to(repo_root)} ({icns_img.width}x{icns_img.height})"

    if png_fallback.is_file():
        with Image.open(png_fallback) as im:
            rgba = fit_square_rgba(im)
            return rgba, f"{png_fallback.relative_to(repo_root)} ({rgba.width}x{rgba.height})"

    raise FileNotFoundError(
        f"No suitable source icon found under {repo_root / 'assets'} "
        "(checked eqho.ico, eqho.icns, logo_64_dark.png)"
    )


def render_square_icon(master: Image.Image, size: int) -> Image.Image:
    if master.size == (size, size):
        return master.copy()
    return master.resize((size, size), Image.Resampling.LANCZOS)


def render_padded_tile(
    master: Image.Image,
    width: int,
    height: int,
    icon_ratio: float = 0.66,
) -> Image.Image:
    canvas = Image.new("RGBA", (width, height), (0, 0, 0, 0))
    icon_size = max(1, round(min(width, height) * icon_ratio))
    icon = render_square_icon(master, icon_size)
    offset_x = (width - icon_size) // 2
    offset_y = (height - icon_size) // 2
    canvas.paste(icon, (offset_x, offset_y), icon)
    return canvas


def encode_png(image: Image.Image) -> bytes:
    buf = io.BytesIO()
    image.save(buf, format="PNG", compress_level=9)
    return buf.getvalue()


def build_asset_map(master: Image.Image) -> dict[str, bytes]:
    assets: dict[str, bytes] = {}

    # StoreLogo (50x50 base + scales)
    assets["StoreLogo.png"] = encode_png(render_square_icon(master, 50))
    for scale in SCALES:
        sz = ceil_scale(50, scale)
        assets[f"StoreLogo.scale-{scale}.png"] = encode_png(render_square_icon(master, sz))

    # Square44x44Logo (44x44 base + scales + targetsize default/unplated/lightunplated)
    assets["Square44x44Logo.png"] = encode_png(render_square_icon(master, 44))
    for scale in SCALES:
        sz = ceil_scale(44, scale)
        assets[f"Square44x44Logo.scale-{scale}.png"] = encode_png(
            render_square_icon(master, sz)
        )
    for sz in TARGET_SIZES:
        png = encode_png(render_square_icon(master, sz))
        assets[f"Square44x44Logo.targetsize-{sz}.png"] = png
        assets[f"Square44x44Logo.targetsize-{sz}_altform-unplated.png"] = png
        assets[f"Square44x44Logo.targetsize-{sz}_altform-lightunplated.png"] = png

    # Square150x150Logo (150x150 padded tile + scales)
    assets["Square150x150Logo.png"] = encode_png(render_padded_tile(master, 150, 150))
    for scale in SCALES:
        sz = ceil_scale(150, scale)
        assets[f"Square150x150Logo.scale-{scale}.png"] = encode_png(
            render_padded_tile(master, sz, sz)
        )

    # Wide310x150Logo (310x150 padded tile + scales)
    assets["Wide310x150Logo.png"] = encode_png(render_padded_tile(master, 310, 150))
    for scale in SCALES:
        w = ceil_scale(310, scale)
        h = ceil_scale(150, scale)
        assets[f"Wide310x150Logo.scale-{scale}.png"] = encode_png(
            render_padded_tile(master, w, h)
        )

    return assets


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Generate Eqho MSIX logo assets from assets/."
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="Verify committed assets match fresh generation without writing.",
    )
    args = parser.parse_args(argv)

    repo_root = Path(__file__).resolve().parents[2]
    out_dir = Path(__file__).resolve().parent / "assets"

    master, source_label = load_master_icon(repo_root)
    expected = build_asset_map(master)

    if args.check:
        if not out_dir.is_dir():
            print(f"Missing MSIX assets directory: {out_dir}", file=sys.stderr)
            return 1
        actual_files = {p.name for p in out_dir.iterdir() if p.is_file()}
        expected_files = set(expected.keys())
        if actual_files != expected_files:
            missing = sorted(expected_files - actual_files)
            extra = sorted(actual_files - expected_files)
            print(
                f"MSIX assets mismatch: missing={missing}, extra={extra}",
                file=sys.stderr,
            )
            return 1
        for name, data in sorted(expected.items()):
            current = (out_dir / name).read_bytes()
            if current != data:
                print(
                    f"MSIX asset differs from fresh generation: {out_dir / name}",
                    file=sys.stderr,
                )
                return 1
        print(f"generate_assets: --check passed ({len(expected)} assets from {source_label}).")
        return 0

    out_dir.mkdir(parents=True, exist_ok=True)
    for existing in out_dir.iterdir():
        if existing.is_file() and existing.name not in expected:
            existing.unlink()
    for name, data in sorted(expected.items()):
        (out_dir / name).write_bytes(data)
    print(f"generate_assets: wrote {len(expected)} assets from {source_label} to {out_dir}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
