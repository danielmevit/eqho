"""Dashboard tabs. TAB_CLASSES preserves the build/navigation order.

The pill nav shows general/models/history; "settings" (gear icon) hosts the
theme mode plus the embedded Overlay and About sections.
"""

from .general import GeneralTab
from .overlay import OverlayTab
from .models import ModelsTab
from .history import HistoryTab
from .about import AboutTab
from .settings import SettingsTab

TAB_CLASSES = {
    "general": GeneralTab,
    "models": ModelsTab,
    "history": HistoryTab,
    "settings": SettingsTab,
}

__all__ = [
    "GeneralTab", "OverlayTab", "ModelsTab", "HistoryTab", "AboutTab",
    "SettingsTab", "TAB_CLASSES",
]
