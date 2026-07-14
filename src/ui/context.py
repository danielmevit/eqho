"""Shared context handed to every dashboard tab.

Carries settings, live theme colors, and the callbacks tabs need, plus a tiny
pub-sub so tabs communicate via events instead of reaching into each other's
widgets (e.g. Models tab emits "model_changed", General tab updates its own
dropdown).
"""

from typing import Callable

from ..settings import Settings, is_model_cached
from ..theme import ThemeColors


class DashboardContext:
    def __init__(
        self,
        settings: Settings,
        colors_getter: Callable[[], ThemeColors],
        apply_settings: Callable[[bool], None],
        get_col_count: Callable[[], int],
        rebuild_tab: Callable[[str], None],
        set_ui_scale: Callable[[float], None] = lambda scale: None,
        master_getter: Callable[[], object] = lambda: None,
        change_model: Callable[[str], None] = lambda m: None,
        set_theme: Callable[[str], None] = lambda mode: None,
        refresh_status: Callable[[], None] = lambda: None,
    ):
        self.settings = settings
        self._colors_getter = colors_getter
        self.apply_settings = apply_settings  # apply_settings(reload_model: bool)
        self.get_col_count = get_col_count
        self.rebuild_tab = rebuild_tab
        self.set_ui_scale = set_ui_scale
        self.master = master_getter  # the dashboard window — tk Variables MUST bind to it
        self.change_model = change_model  # change_model(new_model) — confirms + restarts
        self.set_theme = set_theme  # set_theme("light"|"dark"|"system") — retheme + rebuild
        self.refresh_status = refresh_status  # update the top bar's status line
        # _subs[event][key] = handler — keyed so a rebuilt tab REPLACES its old
        # subscription instead of stacking dead handlers on destroyed widgets.
        self._subs: dict[str, dict[str, Callable]] = {}

    @property
    def colors(self) -> ThemeColors:
        return self._colors_getter()

    def is_model_cached(self, model_key: str) -> bool:
        return is_model_cached(self.settings, model_key)

    def subscribe(self, event: str, key: str, handler: Callable) -> None:
        self._subs.setdefault(event, {})[key] = handler

    def emit(self, event: str, *args) -> None:
        for handler in list(self._subs.get(event, {}).values()):
            try:
                handler(*args)
            except Exception:
                # A handler bound to a destroyed widget must not break others.
                pass
