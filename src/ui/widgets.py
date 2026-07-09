"""Custom themed widgets shared across the dashboard."""

import tkinter as tk
from typing import Optional

import customtkinter as ctk

from ..fonts import FONT_FAMILY
from ..theme import FONT_SIZES, RADIUS_SM


class ThemedDropdown(ctk.CTkFrame):
    """Custom dropdown menu that replaces native tkinter.Menu with themed popups.

    Renders as a button that opens a floating CTkFrame with selectable items.
    Supports rounded corners and full theme control.
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
        dropdown_fg_color=None,
        dropdown_hover_color=None,
        dropdown_text_color=None,
        **kwargs,
    ):
        super().__init__(parent, fg_color="transparent", width=width, height=height)
        self._values = values
        self._variable = variable
        self._command = command
        self._dropdown_open = False
        self._popup = None
        self._font = font or (FONT_FAMILY, FONT_SIZES["sm"])
        self._dropdown_font = dropdown_font or self._font
        self._fg_color = button_color or fg_color or "#333"
        self._hover_color = button_hover_color or "#444"
        self._dd_fg = dropdown_fg_color or "#222"
        self._dd_hover = dropdown_hover_color or "#444"
        self._dd_text = dropdown_text_color or "#eee"
        self._text_color = text_color or "#eee"
        self._corner_radius = corner_radius
        self._btn_width = width
        self._btn_height = height

        current = variable.get() if variable else (values[0] if values else "")

        self._button = ctk.CTkButton(
            self,
            text=f"{current}  ▾",
            width=width,
            height=height,
            corner_radius=corner_radius,
            font=self._font,
            fg_color=self._fg_color,
            text_color=self._text_color,
            hover_color=self._hover_color,
            anchor="w",
            command=self._toggle_popup,
        )
        self._button.pack()

        if self._variable:
            self._variable.trace_add("write", self._on_var_changed)

    def _on_var_changed(self, *args) -> None:
        val = self._variable.get()
        self._button.configure(text=f"{val}  ▾")

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
            corner_radius=self._corner_radius,
            border_width=1,
            border_color=self._hover_color,
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
        else:
            items_parent = container

        current_val = self._variable.get() if self._variable else ""

        for val in self._values:
            is_selected = val == current_val
            btn = ctk.CTkButton(
                items_parent,
                text=val,
                font=self._dropdown_font,
                height=item_h,
                corner_radius=4,
                fg_color=self._dd_hover if is_selected else "transparent",
                text_color=self._dd_text,
                hover_color=self._dd_hover,
                anchor="w",
                command=lambda v=val: self._select_value(v),
            )
            btn.pack(fill="x", padx=4, pady=1)

        # Position below the button
        self.update_idletasks()
        x = self._button.winfo_rootx()
        y = self._button.winfo_rooty() + self._button.winfo_height() + 2
        popup_w = max(self._btn_width, 160)

        # Ensure popup doesn't go off-screen
        screen_h = self.winfo_screenheight()
        if y + popup_h > screen_h - 40:
            y = self._button.winfo_rooty() - popup_h - 2

        self._popup.geometry(f"{popup_w}x{popup_h}+{x}+{y}")
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

        # Close on click outside
        self._popup.bind("<FocusOut>", lambda e: self.after(100, self._close_popup))
        self._popup.focus_set()

    def _select_value(self, val: str) -> None:
        if self._variable:
            self._variable.set(val)
        self._button.configure(text=f"{val}  ▾")
        self._close_popup()
        if self._command:
            self._command(val)

    def _close_popup(self) -> None:
        self._dropdown_open = False
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
        self._button.configure(text=f"{value}  ▾")

    def get(self) -> str:
        return self._variable.get() if self._variable else ""
