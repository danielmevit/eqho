"""Phosphor icon glyphs for the dashboard (https://phosphoricons.com, MIT).

The regular-weight Phosphor.ttf lives in assets/fonts and is registered at
startup by fonts.load_fonts() alongside Inter — icons render as ordinary text
in the "Phosphor" family: crisp at any UI zoom and colorable like any label.
Codepoints below come from @phosphor-icons/web's style.css.
"""

import customtkinter as ctk

from ..theme import FONT_SIZES

ICON_FAMILY = "Phosphor"

ICONS = {
    "general": "",   # sliders-horizontal
    "models": "",    # stack
    "history": "",   # clock-counter-clockwise
    "settings": "",  # gear-six
    "sun": "",       # sun (shown in dark mode → switch to light)
    "moon": "",      # moon (shown in light mode → switch to dark)
    "overlay": "",   # monitor
    "about": "",     # info
    "theme": "",     # palette
    "mic": "",       # microphone
}


def icon(name: str) -> str:
    return ICONS.get(name, "")


def icon_font(size: str = "base", delta: int = 2) -> ctk.CTkFont:
    """Phosphor glyphs sit slightly small next to Inter at equal point size —
    `delta` compensates."""
    return ctk.CTkFont(family=ICON_FAMILY, size=FONT_SIZES[size] + delta)
