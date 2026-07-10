"""Regenerate the app icon set from the current logo sources.

Copies the PNG set from `logo/new logo/` into `assets/` (runtime icons) and
`logo/` (tracked copies used by the README), then rebuilds `assets/eqho.ico`
from the 62 px mark. Run with the project venv:

    venv\\Scripts\\python.exe tools\\make_icons.py
"""

import shutil
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / "logo" / "new logo"
ASSETS = ROOT / "assets"
LOGO = ROOT / "logo"

PNG_NAMES = [
    "logo_32_dark.png",
    "logo_32_white.png",
    "logo_62_dark.png",
    "logo_62_white.png",
    "logo_horizontal_dark.png",
    "logo_horizontal_light.png",
]

ICO_SIZES = [(16, 16), (24, 24), (32, 32), (48, 48), (64, 64)]


def main() -> None:
    if not SRC.exists():
        print(f"Source dir not found: {SRC} — nothing to do.")
        return

    for name in PNG_NAMES:
        src = SRC / name
        if not src.exists():
            print(f"skip (missing): {name}")
            continue
        for dest_dir in (ASSETS, LOGO):
            shutil.copy2(src, dest_dir / name)
        print(f"copied: {name} -> assets/, logo/")

    mark = ASSETS / "logo_62_dark.png"
    if mark.exists():
        base = Image.open(mark).convert("RGBA")
        side = max(base.size)
        canvas = Image.new("RGBA", (side, side), (0, 0, 0, 0))
        canvas.paste(base, ((side - base.width) // 2, (side - base.height) // 2))
        canvas.save(ASSETS / "eqho.ico", sizes=ICO_SIZES)
        print(f"wrote: assets/eqho.ico ({', '.join(f'{w}px' for w, _ in ICO_SIZES)})")


if __name__ == "__main__":
    main()
