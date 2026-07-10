"""Eqho Design System — tokens and theme engine.

Three modes: dark, light, system (auto-detects Windows theme).
Compact pro-tool aesthetic: near-black dark surfaces, hairline 1px borders,
small radii, dense Inter typography. Both themes share the same geometry;
only the palettes differ.
"""

from dataclasses import dataclass
from typing import Optional
import logging

log = logging.getLogger(__name__)

# -- Design tokens (constant across themes) -----------------------------------

RADIUS_SM = 4      # buttons, inputs, chips
RADIUS_MD = 6      # dropdown popups, menus
RADIUS_LG = 8      # cards, panels
RADIUS_XL = 12     # overlay bar, large containers

# Eqho blue. (Toolcraft-blue "#0c8ce9" is a deliberate one-line swap option
# if a deeper, more saturated accent is ever preferred.)
ACCENT = "#58a6ff"
ACCENT_HOVER = "#79b8ff"
ACCENT_MUTED = "#16283f"    # dark mode accent bg
ACCENT_LIGHT_MUTED = "#dbeafe"  # light mode accent bg

SUCCESS = "#3fb950"
SUCCESS_MUTED = "#1a3a2a"
WARNING = "#d29922"
WARNING_MUTED = "#3d2e00"
DANGER = "#f85149"

FONT_FAMILY = "Inter"
FONT_SIZES = {
    "xs": 10,
    "sm": 11,
    "base": 13,
    "lg": 15,
    "xl": 17,
    "2xl": 20,
}

SPACING = {
    "xs": 4,
    "sm": 8,
    "md": 12,
    "lg": 16,
    "xl": 24,
    "2xl": 32,
}


def font(size: str = "base", weight: str | None = None) -> tuple:
    """Font tuple for tk/customtkinter widgets — the ONLY way UI code should
    build fonts, so family/scale changes stay one-file edits."""
    if weight:
        return (FONT_FAMILY, FONT_SIZES[size], weight)
    return (FONT_FAMILY, FONT_SIZES[size])


@dataclass(frozen=True)
class ThemeColors:
    """Color palette for a single theme mode."""
    bg_primary: str       # main window background
    bg_secondary: str     # sidebar, cards
    bg_tertiary: str      # input fields, nested cards
    bg_hover: str         # hover state for interactive elements
    fg_primary: str       # main text
    fg_secondary: str     # labels, descriptions
    fg_muted: str         # placeholders, disabled text
    border: str           # borders, dividers
    border_subtle: str    # very subtle separators
    accent: str           # primary action color
    accent_hover: str
    accent_muted: str     # accent background (tags, badges)
    on_accent: str        # text/icon color that stays readable ON the accent fill
    success: str
    success_muted: str
    warning: str
    warning_muted: str
    danger: str
    # Overlay specific
    overlay_bg: str
    overlay_fg: str
    overlay_accent: str


DARK = ThemeColors(
    bg_primary="#0a0a0b",       # near-black canvas
    bg_secondary="#151517",     # sidebar, cards
    bg_tertiary="#1f1f23",      # inputs, nested surfaces
    bg_hover="#29292e",         # hover states
    fg_primary="#f5f6f8",       # main text
    fg_secondary="#a1a1aa",     # labels, descriptions
    fg_muted="#85858f",         # placeholders, hints (kept readable on cards)
    border="#2a2a30",           # hairline borders on controls
    border_subtle="#232327",    # hairline borders on cards/panels
    accent=ACCENT,
    accent_hover=ACCENT_HOVER,
    accent_muted=ACCENT_MUTED,
    on_accent="#0a0a0b",        # near-black reads best on the light Eqho blue
    success=SUCCESS,
    success_muted=SUCCESS_MUTED,
    warning=WARNING,
    warning_muted=WARNING_MUTED,
    danger=DANGER,
    overlay_bg="#151517",
    overlay_fg="#f5f6f8",
    overlay_accent=ACCENT,
)

LIGHT = ThemeColors(
    bg_primary="#f5f6f8",       # light gray canvas
    bg_secondary="#ffffff",     # white cards, sidebar
    bg_tertiary="#ecedf0",      # inputs, nested surfaces
    bg_hover="#e2e4e8",         # hover states
    fg_primary="#1a1c1f",       # main text (near-black)
    fg_secondary="#4b4f58",     # labels, descriptions
    fg_muted="#74747e",         # placeholders, hints
    border="#d4d6dc",           # hairline borders on controls
    border_subtle="#e4e6ea",    # hairline borders on cards/panels
    accent="#0969da",
    accent_hover="#0550ae",
    accent_muted=ACCENT_LIGHT_MUTED,
    on_accent="#ffffff",        # white reads best on the deep light-mode blue
    success="#1a7f37",
    success_muted="#dafbe1",
    warning="#9a6700",
    warning_muted="#fff8c5",
    danger="#cf222e",
    overlay_bg="#ffffff",
    overlay_fg="#1a1c1f",
    overlay_accent="#0969da",
)


def get_system_theme() -> str:
    """Detect Windows light/dark mode. Returns 'dark' or 'light'."""
    try:
        import winreg
        key = winreg.OpenKey(
            winreg.HKEY_CURRENT_USER,
            r"SOFTWARE\Microsoft\Windows\CurrentVersion\Themes\Personalize",
        )
        value, _ = winreg.QueryValueEx(key, "AppsUseLightTheme")
        winreg.CloseKey(key)
        return "light" if value == 1 else "dark"
    except Exception:
        return "dark"


def get_colors(mode: str) -> ThemeColors:
    """Get the color palette for the given mode ('dark', 'light', 'system')."""
    if mode == "system":
        mode = get_system_theme()
    return LIGHT if mode == "light" else DARK


# -- Model metadata for the UI ------------------------------------------------

MODEL_INFO = {
    "distil-large-v3": {
        "name": "Distil Large v3",
        "size": "~1.5 GB",
        "lang": "English optimized",
        "device": "GPU accelerated",
        "rec": "Recommended for most users",
        "icon": "✓",
    },
    "distil-medium.en": {
        "name": "Distil Medium EN",
        "size": "~750 MB",
        "lang": "English optimized",
        "device": "GPU accelerated",
        "rec": "Lighter, still great for English",
        "icon": "✓",
    },
    "distil-small.en": {
        "name": "Distil Small EN",
        "size": "~330 MB",
        "lang": "English optimized",
        "device": "GPU accelerated",
        "rec": "Fastest English, slightly lower accuracy",
        "icon": "✓",
    },
    "large-v3-turbo": {
        "name": "Large v3 Turbo",
        "size": "~1.6 GB",
        "lang": "100+ languages",
        "device": "GPU accelerated",
        "rec": "Best multilingual option",
        "icon": "✓",
    },
    "medium": {
        "name": "Medium",
        "size": "~1.5 GB",
        "lang": "100+ languages",
        "device": "GPU accelerated",
        "rec": "Solid multilingual fallback",
        "icon": "✓",
    },
    "small": {
        "name": "Small",
        "size": "~950 MB",
        "lang": "100+ languages",
        "device": "GPU accelerated",
        "rec": "Balanced speed and accuracy",
        "icon": "✓",
    },
    "base": {
        "name": "Base",
        "size": "~300 MB",
        "lang": "100+ languages",
        "device": "GPU accelerated",
        "rec": "Lightweight multilingual",
        "icon": "✓",
    },
    "tiny": {
        "name": "Tiny",
        "size": "~150 MB",
        "lang": "100+ languages",
        "device": "GPU accelerated",
        "rec": "Fastest, least accurate",
        "icon": "✓",
    },
    "large-v3": {
        "name": "Large v3",
        "size": "~3.1 GB",
        "lang": "100+ languages",
        "device": "CPU only (too large for 6 GB VRAM)",
        "rec": "Highest accuracy, significantly slower",
        "icon": "⚠",
    },
}
