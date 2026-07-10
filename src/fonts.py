"""Load the bundled Inter fonts at runtime so tkinter can use them without a
system-wide install. The per-OS mechanics live in src/oskit (Windows:
AddFontResourceEx, private to this process; Linux: user fontconfig dir;
macOS: not yet — falls back to the system font)."""

import logging
from pathlib import Path

from . import oskit

log = logging.getLogger(__name__)

_FONTS_DIR = Path(__file__).resolve().parent.parent / "assets" / "fonts"

# Font family name to use in tkinter
FONT_FAMILY = "Inter"
FONT_FALLBACK = "Segoe UI"


def load_fonts() -> bool:
    """Register the bundled fonts. Returns True if any were made available."""
    if not _FONTS_DIR.exists():
        log.warning("Fonts directory not found: %s", _FONTS_DIR)
        return False
    count = oskit.get().load_fonts(_FONTS_DIR)
    if count:
        log.info("Loaded %d Inter font file(s) from assets/fonts.", count)
    return count > 0


def unload_fonts() -> None:
    """Remove fonts registered by load_fonts (no-op where fonts persist per-user)."""
    oskit.get().unload_fonts(_FONTS_DIR)
