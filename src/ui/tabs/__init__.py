"""Dashboard tabs. TAB_CLASSES preserves the build/navigation order."""

from .general import GeneralTab
from .overlay import OverlayTab
from .models import ModelsTab
from .history import HistoryTab
from .about import AboutTab

TAB_CLASSES = {
    "general": GeneralTab,
    "overlay": OverlayTab,
    "models": ModelsTab,
    "history": HistoryTab,
    "about": AboutTab,
}

__all__ = ["GeneralTab", "OverlayTab", "ModelsTab", "HistoryTab", "AboutTab", "TAB_CLASSES"]
