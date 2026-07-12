"""Custom themed widgets shared across the dashboard."""

import tkinter as tk
from typing import Optional

import customtkinter as ctk

from ..theme import RADIUS_SM, RADIUS_MD, font


# -- Button recipes -------------------------------------------------------------
# The three button styles of the design system. Pass a ThemeColors instance;
# any CTkButton kwarg can be overridden.

def primary_button(parent, colors, **kwargs) -> ctk.CTkButton:
    """Filled accent button for the main action."""
    opts = dict(
        corner_radius=RADIUS_SM,
        height=28,
        font=font("sm"),
        fg_color=colors.accent,
        hover_color=colors.accent_hover,
        text_color=colors.on_accent,
        border_width=0,
    )
    opts.update(kwargs)
    return ctk.CTkButton(parent, **opts)


def secondary_button(parent, colors, **kwargs) -> ctk.CTkButton:
    """Hairline-bordered neutral button."""
    opts = dict(
        corner_radius=RADIUS_SM,
        height=28,
        font=font("sm"),
        fg_color=colors.bg_tertiary,
        hover_color=colors.bg_hover,
        text_color=colors.fg_primary,
        border_width=1,
        border_color=colors.border,
    )
    opts.update(kwargs)
    return ctk.CTkButton(parent, **opts)


def themed_switch(parent, colors, **kwargs) -> ctk.CTkSwitch:
    """Toggle switch — track AND knob stay in the accent-blue family
    (a black knob on the blue track reads wrong, per Daniel)."""
    opts = dict(
        text="",
        onvalue=True, offvalue=False,
        width=44, height=22,
        progress_color=colors.accent,      # ON track
        fg_color=colors.bg_hover,          # OFF track
        button_color=colors.accent_hover,  # knob
        button_hover_color=colors.accent,
    )
    opts.update(kwargs)
    return ctk.CTkSwitch(parent, **opts)


def segmented(parent, colors, **kwargs) -> ctk.CTkSegmentedButton:
    """Segmented control: rounded, inset trough (bg_primary against the card),
    and an accent_selected chip tuned so fg_primary text passes AA contrast."""
    opts = dict(
        font=font("sm"),
        corner_radius=RADIUS_MD,
        fg_color=colors.bg_primary,
        unselected_color=colors.bg_primary,
        unselected_hover_color=colors.bg_hover,
        selected_color=colors.accent_selected,
        selected_hover_color=colors.accent_selected,
        text_color=colors.fg_primary,
    )
    opts.update(kwargs)
    return ctk.CTkSegmentedButton(parent, **opts)


def ghost_button(parent, colors, **kwargs) -> ctk.CTkButton:
    """Borderless transparent button (nav items, inline actions)."""
    opts = dict(
        corner_radius=RADIUS_SM,
        height=28,
        font=font("sm"),
        fg_color="transparent",
        hover_color=colors.bg_hover,
        text_color=colors.fg_secondary,
        border_width=0,
    )
    opts.update(kwargs)
    return ctk.CTkButton(parent, **opts)


class ThemedDropdown(ctk.CTkFrame):
    """Custom dropdown menu that replaces native tkinter.Menu with themed popups.

    Renders as a hairline-bordered button that opens a floating CTkFrame with
    selectable items. All colors must be supplied by the caller (the layout
    factory passes the active ThemeColors values) — there are no hardcoded
    fallback colors.
    """

    def __init__(
        self,
        parent,
        values: list[str],
        variable: Optional[ctk.StringVar] = None,
        command=None,
        width: int = 160,
        height: int = 30,
        corner_radius: int = RADIUS_SM,
        font=None,
        dropdown_font=None,
        fg_color=None,
        text_color=None,
        button_color=None,
        button_hover_color=None,
        border_color=None,
        dropdown_fg_color=None,
        dropdown_hover_color=None,
        dropdown_text_color=None,
        **kwargs,
    ):
        super().__init__(parent, fg_color="transparent", width=width, height=height)
        from ..theme import font as theme_font  # avoid shadowing by the `font` kwarg
        self._values = values
        self._variable = variable
        self._command = command
        self._dropdown_open = False
        self._popup = None
        self._font = font or theme_font("sm")
        self._dropdown_font = dropdown_font or self._font
        self._fg_color = button_color or fg_color
        self._hover_color = button_hover_color
        self._border_color = border_color
        self._dd_fg = dropdown_fg_color
        self._dd_hover = dropdown_hover_color
        self._dd_text = dropdown_text_color
        self._text_color = text_color
        self._corner_radius = corner_radius
        self._btn_width = width
        self._btn_height = height

        current = variable.get() if variable else (values[0] if values else "")

        # Closed state: bordered frame with the value left-aligned and the
        # chevron pinned to the right edge (like a native combobox).
        self._button = ctk.CTkFrame(
            self,
            width=width,
            height=height,
            corner_radius=corner_radius,
            fg_color=self._fg_color,
            border_width=1 if self._border_color else 0,
            border_color=self._border_color,
        )
        self._button.pack()
        self._button.pack_propagate(False)

        self._text_label = ctk.CTkLabel(
            self._button, text=current, font=self._font,
            text_color=self._text_color, fg_color="transparent", anchor="w",
        )
        # 1px downward nudge — Inter's metrics sit text slightly high
        self._text_label.pack(side="left", fill="x", expand=True, padx=(10, 2), pady=(1, 0))

        self._chevron = ctk.CTkLabel(
            self._button, text="▾", font=self._font,
            text_color=self._text_color, fg_color="transparent", width=14,
        )
        self._chevron.pack(side="right", padx=(0, 8), pady=(1, 0))

        for widget in (self._button, self._text_label, self._chevron):
            widget.bind("<Button-1>", lambda e: self._toggle_popup())
            widget.bind("<Enter>", lambda e: self._set_hover(True))
            widget.bind("<Leave>", lambda e: self._set_hover(False))
            try:
                widget.configure(cursor="hand2")
            except Exception:
                pass

        self._item_buttons: list[ctk.CTkButton] = []
        self._scroll_frame = None
        self._active_index = 0

        if self._variable:
            self._variable.trace_add("write", self._on_var_changed)

    def _set_hover(self, hovering: bool) -> None:
        color = self._hover_color if hovering else self._fg_color
        if color:
            self._button.configure(fg_color=color)

    def _on_var_changed(self, *args) -> None:
        self._text_label.configure(text=self._variable.get())

    def _toggle_popup(self) -> None:
        if self._dropdown_open:
            self._close_popup()
        else:
            self._open_popup()

    def _open_popup(self) -> None:
        if self._popup is not None:
            self._close_popup()

        self._dropdown_open = True

        # Create a toplevel for the dropdown
        self._popup = tk.Toplevel(self.winfo_toplevel())
        self._popup.withdraw()
        self._popup.overrideredirect(True)
        self._popup.attributes("-topmost", True)

        # Container frame with rounded appearance
        container = ctk.CTkFrame(
            self._popup,
            fg_color=self._dd_fg,
            corner_radius=RADIUS_MD,
            border_width=1,
            border_color=self._border_color or self._hover_color,
        )
        container.pack(fill="both", expand=True, padx=1, pady=1)

        # Scrollable if many items
        max_visible = 8
        item_h = 28
        visible_count = min(len(self._values), max_visible)
        popup_h = visible_count * item_h + 8

        if len(self._values) > max_visible:
            scroll = ctk.CTkScrollableFrame(
                container, fg_color="transparent",
                height=popup_h,
                scrollbar_button_color=self._hover_color,
            )
            scroll.pack(fill="both", expand=True, padx=2, pady=4)
            items_parent = scroll
            self._scroll_frame = scroll
        else:
            items_parent = container
            self._scroll_frame = None

        current_val = self._variable.get() if self._variable else ""

        self._item_buttons = []
        for val in self._values:
            btn = ctk.CTkButton(
                items_parent,
                text=val,
                font=self._dropdown_font,
                height=item_h,
                corner_radius=RADIUS_SM,
                fg_color="transparent",
                text_color=self._dd_text,
                hover_color=self._dd_hover,
                anchor="w",
                command=lambda v=val: self._select_value(v),
            )
            btn.pack(fill="x", padx=4, pady=1)
            self._item_buttons.append(btn)

        try:
            self._active_index = self._values.index(current_val)
        except ValueError:
            self._active_index = 0
        self._highlight_active()
        self._scroll_active_into_view()

        # Position below the button. The popup is a raw tk.Toplevel, so its
        # geometry is in REAL screen pixels — but its CustomTkinter contents
        # render at the global widget scaling (1.5 at the default 150% zoom).
        # Scale the box to match, or the last item is clipped off the bottom and
        # the item text is truncated (looked like whisper.cpp was "missing").
        self.update_idletasks()
        try:
            scaling = self._get_widget_scaling()
        except Exception:
            scaling = 1.0
        x = self._button.winfo_rootx()
        y = self._button.winfo_rooty() + self._button.winfo_height() + 2
        box_w = int(max(self._btn_width, 160) * scaling)
        box_h = int(popup_h * scaling)

        # Ensure popup doesn't go off-screen
        screen_h = self.winfo_screenheight()
        if y + box_h > screen_h - 40:
            y = self._button.winfo_rooty() - box_h - 2

        self._popup.geometry(f"{box_w}x{box_h}+{x}+{y}")
        self._popup.deiconify()

        # Apply Windows 11 rounded corners
        try:
            import ctypes
            hwnd = ctypes.windll.user32.GetParent(self._popup.winfo_id())
            DWMWA_WINDOW_CORNER_PREFERENCE = 33
            DWMWCP_ROUND = 2
            preference = ctypes.c_int(DWMWCP_ROUND)
            ctypes.windll.dwmapi.DwmSetWindowAttribute(
                hwnd, DWMWA_WINDOW_CORNER_PREFERENCE,
                ctypes.byref(preference), ctypes.sizeof(preference),
            )
        except Exception:
            pass

        # Close on click outside; keyboard navigation while open
        self._popup.bind("<FocusOut>", lambda e: self.after(100, self._close_popup))
        self._popup.bind("<Escape>", lambda e: self._close_popup())
        self._popup.bind("<Up>", lambda e: self._move_active(-1))
        self._popup.bind("<Down>", lambda e: self._move_active(1))
        self._popup.bind("<Return>", lambda e: self._select_active())
        self._popup.focus_set()

    def _highlight_active(self) -> None:
        for i, btn in enumerate(self._item_buttons):
            btn.configure(
                fg_color=self._dd_hover if i == self._active_index else "transparent"
            )

    def _scroll_active_into_view(self) -> None:
        if self._scroll_frame is None or not self._item_buttons:
            return
        try:
            fraction = max(0, self._active_index - 2) / max(1, len(self._item_buttons))
            self._scroll_frame._parent_canvas.yview_moveto(fraction)
        except Exception:
            pass

    def _move_active(self, delta: int) -> None:
        if not self._item_buttons:
            return
        self._active_index = (self._active_index + delta) % len(self._item_buttons)
        self._highlight_active()
        self._scroll_active_into_view()

    def _select_active(self) -> None:
        if self._item_buttons:
            self._select_value(self._values[self._active_index])

    def _select_value(self, val: str) -> None:
        if self._variable:
            self._variable.set(val)
        self._text_label.configure(text=val)
        self._close_popup()
        if self._command:
            self._command(val)

    def _close_popup(self) -> None:
        self._dropdown_open = False
        self._item_buttons = []
        self._scroll_frame = None
        if self._popup is not None:
            try:
                self._popup.destroy()
            except Exception:
                pass
            self._popup = None

    def pack(self, **kwargs):
        super().pack(**kwargs)

    def set(self, value: str) -> None:
        if self._variable:
            self._variable.set(value)
        self._text_label.configure(text=value)

    def get(self) -> str:
        return self._variable.get() if self._variable else ""
