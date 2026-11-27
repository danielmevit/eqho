//! Native Linux X11/XWayland 32-bit ARGB composited floating dictation overlay:
//! - Iridescent animated pill (`eqho_core::pillfx::render_premul_bgra`)
//! - Upward-growing live transcription panel above (or below) the pill
//! - Hover-reveal & pinnable under-pill interactive control bar (`✕`, timer/`Writing…`,
//!   language picker, `Batch`/`Live`, pin, `✓`)
//! - Searchable 99-language popup menu with flag bitmaps and peeking recent-language chips
//! - Floating failed-paste recovery action card (`Show text` / `Copy` / `Discard`)
//!
//! Dynamically loads `libX11.so.6`, optional `libXrandr.so.2` / `libXext.so.6`, and optional
//! `libcairo.so.2` / `libpangocairo-1.0.so.0` / `libpango-1.0.so.0` / `libgobject-2.0.so.0` via
//! `libloading` and keeps them loaded for the process lifetime. All windows use
//! `override_redirect = 1` so hovering or clicking overlay controls never steals keyboard focus
//! from the user's active application. Degrades silently to no-op when `$DISPLAY` is absent.

use std::collections::HashMap;
use std::ffi::{CString, c_char, c_int, c_long, c_uchar, c_uint, c_ulong, c_void};
use std::ptr::{null, null_mut};
use std::sync::{Arc, OnceLock, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use eqho_core::pillfx::{pill_size, render_premul_bgra};
use libloading::Library;

use crate::canvas::PremulCanvas;
use crate::pure::{
    self, FRAME_MS, OverlayConfig, OverlayRect, PANEL_MAX_W, PANEL_PADDING_X, PANEL_PADDING_Y,
    PillAnimator, WorkArea,
};
use crate::underbar::{
    self, FINISHING_LABEL, LANG_ROW_HEIGHT, LangPopupLayout, LocalRect, POPUP_PADDING_X, PopupHit,
    RECENT_FADE_WIDTH, RECOVERY_CARD_RADIUS, RecoveryCardButton, RecoveryCardLayout, Rgba,
    UNDERBAR_HOVER_ALPHA_RATIO, UNDERBAR_RADIUS, UNDERBAR_REST_ALPHA_RATIO, UnderbarControl,
    UnderbarLayout, UnderbarPlacement, UnderbarTheme, UnderbarVisibility,
};
use crate::{FlagBitmapProvider, OverlayAction, OverlayActionCallback};

const INPUT_OUTPUT: c_uint = 1;
const TRUE_COLOR: c_int = 4;
const Z_PIXMAP: c_int = 2;
const CW_BACK_PIXEL: c_ulong = 1 << 1;
const CW_BORDER_PIXEL: c_ulong = 1 << 3;
const CW_OVERRIDE_REDIRECT: c_ulong = 1 << 9;
const CW_EVENT_MASK: c_ulong = 1 << 11;
const CW_COLORMAP: c_ulong = 1 << 13;
const ALLOC_NONE: c_int = 0;
const PROP_MODE_REPLACE: c_int = 0;
const XA_ATOM: c_ulong = 4;
const XA_CARDINAL: c_ulong = 6;
const SHAPE_SET: c_int = 0;
const SHAPE_INPUT: c_int = 2;

const KEY_PRESS_MASK: c_long = 1 << 0;
const BUTTON_PRESS_MASK: c_long = 1 << 2;
const ENTER_WINDOW_MASK: c_long = 1 << 4;
const LEAVE_WINDOW_MASK: c_long = 1 << 5;
const POINTER_MOTION_MASK: c_long = 1 << 6;

const KEY_PRESS: c_int = 2;
const BUTTON_PRESS: c_int = 4;
const MOTION_NOTIFY: c_int = 6;
const ENTER_NOTIFY: c_int = 7;
const LEAVE_NOTIFY: c_int = 8;

const BUTTON1_MASK: c_uint = 1 << 8;
const BUTTON3_MASK: c_uint = 1 << 10;

const XK_BACKSPACE: c_ulong = 0xFF08;
const XK_RETURN: c_ulong = 0xFF0D;
const XK_ESCAPE: c_ulong = 0xFF1B;
const XK_UP: c_ulong = 0xFF52;
const XK_DOWN: c_ulong = 0xFF54;

const CAIRO_FORMAT_ARGB32: c_int = 0;
const PANGO_SCALE: c_int = 1024;
const PANGO_ALIGN_LEFT: c_int = 0;
const PANGO_ALIGN_CENTER: c_int = 1;
const PANGO_WRAP_WORD_CHAR: c_int = 2;
const PANGO_ELLIPSIZE_END: c_int = 3;

#[repr(C)]
struct XVisualInfo {
    visual: *mut c_void,
    visualid: c_ulong,
    screen: c_int,
    depth: c_int,
    class: c_int,
    red_mask: c_ulong,
    green_mask: c_ulong,
    blue_mask: c_ulong,
    colormap_size: c_int,
    bits_per_rgb: c_int,
}

#[repr(C)]
struct XSetWindowAttributes {
    background_pixmap: c_ulong,
    background_pixel: c_ulong,
    border_pixmap: c_ulong,
    border_pixel: c_ulong,
    bit_gravity: c_int,
    win_gravity: c_int,
    backing_store: c_int,
    backing_planes: c_ulong,
    backing_pixel: c_ulong,
    save_under: c_int,
    event_mask: c_long,
    do_not_propagate_mask: c_long,
    override_redirect: c_int,
    colormap: c_ulong,
    cursor: c_ulong,
}

#[repr(C)]
struct XRectangle {
    x: i16,
    y: i16,
    width: u16,
    height: u16,
}

#[repr(C)]
struct XImage {
    width: c_int,
    height: c_int,
    xoffset: c_int,
    format: c_int,
    data: *mut c_char,
    byte_order: c_int,
    bitmap_unit: c_int,
    bitmap_bit_order: c_int,
    bitmap_pad: c_int,
    depth: c_int,
    bytes_per_line: c_int,
    bits_per_pixel: c_int,
    red_mask: c_ulong,
    green_mask: c_ulong,
    blue_mask: c_ulong,
    obdata: *mut c_char,
}

#[repr(C)]
struct XrrMonitorInfo {
    name: c_ulong,
    primary: c_int,
    automatic: c_int,
    noutput: c_int,
    x: c_int,
    y: c_int,
    width: c_int,
    height: c_int,
    mwidth: c_int,
    mheight: c_int,
    outputs: *mut c_ulong,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct XAnyEvent {
    r#type: c_int,
    serial: c_ulong,
    send_event: c_int,
    display: *mut c_void,
    window: c_ulong,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct XKeyEvent {
    r#type: c_int,
    serial: c_ulong,
    send_event: c_int,
    display: *mut c_void,
    window: c_ulong,
    root: c_ulong,
    subwindow: c_ulong,
    time: c_ulong,
    x: c_int,
    y: c_int,
    x_root: c_int,
    y_root: c_int,
    state: c_uint,
    keycode: c_uint,
    same_screen: c_int,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct XButtonEvent {
    r#type: c_int,
    serial: c_ulong,
    send_event: c_int,
    display: *mut c_void,
    window: c_ulong,
    root: c_ulong,
    subwindow: c_ulong,
    time: c_ulong,
    x: c_int,
    y: c_int,
    x_root: c_int,
    y_root: c_int,
    state: c_uint,
    button: c_uint,
    same_screen: c_int,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct XMotionEvent {
    r#type: c_int,
    serial: c_ulong,
    send_event: c_int,
    display: *mut c_void,
    window: c_ulong,
    root: c_ulong,
    subwindow: c_ulong,
    time: c_ulong,
    x: c_int,
    y: c_int,
    x_root: c_int,
    y_root: c_int,
    state: c_uint,
    is_hint: c_char,
    same_screen: c_int,
}

#[repr(C)]
union XEvent {
    r#type: c_int,
    xany: XAnyEvent,
    xkey: XKeyEvent,
    xbutton: XButtonEvent,
    xmotion: XMotionEvent,
    _pad: [c_long; 24],
}

pub(crate) enum LinuxOverlayCmd {
    Show(String),
    UpdateText(String),
    SetLevel(f32),
    SetWaveform(f32, f32),
    SetFinishing(bool),
    UpdateSessionMeta {
        language: String,
        delivery_mode: String,
        pinned: bool,
        recent_languages: Vec<String>,
    },
    UpdateConfig(OverlayConfig),
    ShowRecovery {
        text: String,
        paste_target: Option<String>,
    },
    HideRecovery,
    Hide,
    Shutdown,
}

#[derive(Debug)]
pub(crate) struct LinuxOverlayClient {
    tx: mpsc::Sender<LinuxOverlayCmd>,
    thread: Option<JoinHandle<()>>,
}

impl LinuxOverlayClient {
    pub(crate) fn start(
        config: OverlayConfig,
        on_action: Option<OverlayActionCallback>,
        flag_provider: Option<FlagBitmapProvider>,
    ) -> Option<Self> {
        std::env::var_os("DISPLAY")?;
        let (tx, rx) = mpsc::channel::<LinuxOverlayCmd>();
        let handle = thread::Builder::new()
            .name("eqho-overlay-linux".to_owned())
            .spawn(move || run_linux_overlay_thread(config, on_action, flag_provider, rx))
            .ok()?;
        Some(Self {
            tx,
            thread: Some(handle),
        })
    }

    pub(crate) fn send(&self, cmd: LinuxOverlayCmd) {
        let _ = self.tx.send(cmd);
    }

    pub(crate) fn shutdown(&mut self) {
        if let Some(handle) = self.thread.take() {
            let _ = self.tx.send(LinuxOverlayCmd::Shutdown);
            let _ = handle.join();
        }
    }
}

impl Drop for LinuxOverlayClient {
    fn drop(&mut self) {
        self.shutdown();
    }
}

type XrrGetMonitorsFn =
    unsafe extern "C" fn(*mut c_void, c_ulong, c_int, *mut c_int) -> *mut XrrMonitorInfo;
type XrrFreeMonitorsFn = unsafe extern "C" fn(*mut XrrMonitorInfo);
type XShapeCombineRectanglesFn = unsafe extern "C" fn(
    *mut c_void,
    c_ulong,
    c_int,
    c_int,
    c_int,
    *const XRectangle,
    c_int,
    c_int,
    c_int,
);

/// Process-lifetime X11/XRandR/XShape bindings so extension hooks are never `dlclose`-d before
/// `XCloseDisplay`.
struct X11OverlayApi {
    _lib: Library,
    _xrr: Option<Library>,
    _xext: Option<Library>,
    open_display: unsafe extern "C" fn(*const c_char) -> *mut c_void,
    close_display: unsafe extern "C" fn(*mut c_void) -> c_int,
    default_screen: unsafe extern "C" fn(*mut c_void) -> c_int,
    root_window: unsafe extern "C" fn(*mut c_void, c_int) -> c_ulong,
    display_width: unsafe extern "C" fn(*mut c_void, c_int) -> c_int,
    display_height: unsafe extern "C" fn(*mut c_void, c_int) -> c_int,
    intern_atom: unsafe extern "C" fn(*mut c_void, *const c_char, c_int) -> c_ulong,
    change_property: unsafe extern "C" fn(
        *mut c_void,
        c_ulong,
        c_ulong,
        c_ulong,
        c_int,
        c_int,
        *const c_uchar,
        c_int,
    ) -> c_int,
    get_window_property: unsafe extern "C" fn(
        *mut c_void,
        c_ulong,
        c_ulong,
        c_long,
        c_long,
        c_int,
        c_ulong,
        *mut c_ulong,
        *mut c_int,
        *mut c_ulong,
        *mut c_ulong,
        *mut *mut c_uchar,
    ) -> c_int,
    query_pointer: unsafe extern "C" fn(
        *mut c_void,
        c_ulong,
        *mut c_ulong,
        *mut c_ulong,
        *mut c_int,
        *mut c_int,
        *mut c_int,
        *mut c_int,
        *mut c_uint,
    ) -> c_int,
    match_visual_info:
        unsafe extern "C" fn(*mut c_void, c_int, c_int, c_int, *mut XVisualInfo) -> c_int,
    create_colormap: unsafe extern "C" fn(*mut c_void, c_ulong, *mut c_void, c_int) -> c_ulong,
    free_colormap: unsafe extern "C" fn(*mut c_void, c_ulong) -> c_int,
    create_window: unsafe extern "C" fn(
        *mut c_void,
        c_ulong,
        c_int,
        c_int,
        c_uint,
        c_uint,
        c_uint,
        c_int,
        c_uint,
        *mut c_void,
        c_ulong,
        *mut XSetWindowAttributes,
    ) -> c_ulong,
    destroy_window: unsafe extern "C" fn(*mut c_void, c_ulong) -> c_int,
    move_resize_window:
        unsafe extern "C" fn(*mut c_void, c_ulong, c_int, c_int, c_uint, c_uint) -> c_int,
    map_raised: unsafe extern "C" fn(*mut c_void, c_ulong) -> c_int,
    unmap_window: unsafe extern "C" fn(*mut c_void, c_ulong) -> c_int,
    select_input: unsafe extern "C" fn(*mut c_void, c_ulong, c_long) -> c_int,
    pending: unsafe extern "C" fn(*mut c_void) -> c_int,
    next_event: unsafe extern "C" fn(*mut c_void, *mut XEvent) -> c_int,
    lookup_string:
        unsafe extern "C" fn(*mut XKeyEvent, *mut c_char, c_int, *mut c_ulong, *mut c_void) -> c_int,
    create_gc: unsafe extern "C" fn(*mut c_void, c_ulong, c_ulong, *mut c_void) -> *mut c_void,
    free_gc: unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int,
    create_image: unsafe extern "C" fn(
        *mut c_void,
        *mut c_void,
        c_uint,
        c_int,
        c_int,
        *mut c_char,
        c_uint,
        c_uint,
        c_int,
        c_int,
    ) -> *mut XImage,
    put_image: unsafe extern "C" fn(
        *mut c_void,
        c_ulong,
        *mut c_void,
        *mut XImage,
        c_int,
        c_int,
        c_int,
        c_int,
        c_uint,
        c_uint,
    ) -> c_int,
    flush: unsafe extern "C" fn(*mut c_void) -> c_int,
    free: unsafe extern "C" fn(*mut c_void) -> c_int,
    xrr_get_monitors: Option<XrrGetMonitorsFn>,
    xrr_free_monitors: Option<XrrFreeMonitorsFn>,
    xshape_combine_rectangles: Option<XShapeCombineRectanglesFn>,
}

// SAFETY: `X11OverlayApi` holds immutable function pointers and process-lifetime library handles.
unsafe impl Send for X11OverlayApi {}
unsafe impl Sync for X11OverlayApi {}

static X11_OVERLAY_API: OnceLock<Option<X11OverlayApi>> = OnceLock::new();

impl X11OverlayApi {
    fn load() -> Option<&'static Self> {
        X11_OVERLAY_API
            .get_or_init(|| {
                // SAFETY: Standard Xlib/XRandR/XShape functions on Linux.
                unsafe {
                    let lib = Library::new("libX11.so.6").ok()?;
                    let xrr = Library::new("libXrandr.so.2").ok();
                    let (xrr_get_monitors, xrr_free_monitors) = match xrr.as_ref() {
                        Some(l) => match (
                            l.get::<XrrGetMonitorsFn>(b"XRRGetMonitors\0"),
                            l.get::<XrrFreeMonitorsFn>(b"XRRFreeMonitors\0"),
                        ) {
                            (Ok(g), Ok(f)) => (Some(*g), Some(*f)),
                            _ => (None, None),
                        },
                        None => (None, None),
                    };
                    let xext = Library::new("libXext.so.6").ok();
                    let xshape_combine_rectangles = xext.as_ref().and_then(|l| {
                        l.get::<XShapeCombineRectanglesFn>(b"XShapeCombineRectangles\0")
                            .ok()
                            .map(|s| *s)
                    });

                    Some(Self {
                        open_display: *lib.get(b"XOpenDisplay\0").ok()?,
                        close_display: *lib.get(b"XCloseDisplay\0").ok()?,
                        default_screen: *lib.get(b"XDefaultScreen\0").ok()?,
                        root_window: *lib.get(b"XRootWindow\0").ok()?,
                        display_width: *lib.get(b"XDisplayWidth\0").ok()?,
                        display_height: *lib.get(b"XDisplayHeight\0").ok()?,
                        intern_atom: *lib.get(b"XInternAtom\0").ok()?,
                        change_property: *lib.get(b"XChangeProperty\0").ok()?,
                        get_window_property: *lib.get(b"XGetWindowProperty\0").ok()?,
                        query_pointer: *lib.get(b"XQueryPointer\0").ok()?,
                        match_visual_info: *lib.get(b"XMatchVisualInfo\0").ok()?,
                        create_colormap: *lib.get(b"XCreateColormap\0").ok()?,
                        free_colormap: *lib.get(b"XFreeColormap\0").ok()?,
                        create_window: *lib.get(b"XCreateWindow\0").ok()?,
                        destroy_window: *lib.get(b"XDestroyWindow\0").ok()?,
                        move_resize_window: *lib.get(b"XMoveResizeWindow\0").ok()?,
                        map_raised: *lib.get(b"XMapRaised\0").ok()?,
                        unmap_window: *lib.get(b"XUnmapWindow\0").ok()?,
                        select_input: *lib.get(b"XSelectInput\0").ok()?,
                        pending: *lib.get(b"XPending\0").ok()?,
                        next_event: *lib.get(b"XNextEvent\0").ok()?,
                        lookup_string: *lib.get(b"XLookupString\0").ok()?,
                        create_gc: *lib.get(b"XCreateGC\0").ok()?,
                        free_gc: *lib.get(b"XFreeGC\0").ok()?,
                        create_image: *lib.get(b"XCreateImage\0").ok()?,
                        put_image: *lib.get(b"XPutImage\0").ok()?,
                        flush: *lib.get(b"XFlush\0").ok()?,
                        free: *lib.get(b"XFree\0").ok()?,
                        xrr_get_monitors,
                        xrr_free_monitors,
                        xshape_combine_rectangles,
                        _lib: lib,
                        _xrr: xrr,
                        _xext: xext,
                    })
                }
            })
            .as_ref()
    }
}

/// Process-lifetime Cairo + Pango bindings for razor-sharp native HiDPI UTF-8 text rendering
/// directly onto `PremulCanvas` (whose premultiplied BGRA8 layout matches `CAIRO_FORMAT_ARGB32`).
struct PangoCairoApi {
    _cairo: Library,
    _pango: Library,
    _pangocairo: Library,
    _gobject: Library,
    image_surface_create_for_data:
        unsafe extern "C" fn(*mut c_uchar, c_int, c_int, c_int, c_int) -> *mut c_void,
    surface_flush: unsafe extern "C" fn(*mut c_void),
    surface_destroy: unsafe extern "C" fn(*mut c_void),
    cairo_create: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    cairo_destroy: unsafe extern "C" fn(*mut c_void),
    cairo_save: unsafe extern "C" fn(*mut c_void),
    cairo_restore: unsafe extern "C" fn(*mut c_void),
    cairo_scale: unsafe extern "C" fn(*mut c_void, f64, f64),
    cairo_rectangle: unsafe extern "C" fn(*mut c_void, f64, f64, f64, f64),
    cairo_clip: unsafe extern "C" fn(*mut c_void),
    cairo_move_to: unsafe extern "C" fn(*mut c_void, f64, f64),
    cairo_set_source_rgba: unsafe extern "C" fn(*mut c_void, f64, f64, f64, f64),
    pango_cairo_create_layout: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    pango_cairo_show_layout: unsafe extern "C" fn(*mut c_void, *mut c_void),
    pango_font_description_from_string: unsafe extern "C" fn(*const c_char) -> *mut c_void,
    pango_font_description_free: unsafe extern "C" fn(*mut c_void),
    pango_layout_set_font_description: unsafe extern "C" fn(*mut c_void, *const c_void),
    pango_layout_set_text: unsafe extern "C" fn(*mut c_void, *const c_char, c_int),
    pango_layout_set_width: unsafe extern "C" fn(*mut c_void, c_int),
    pango_layout_set_wrap: unsafe extern "C" fn(*mut c_void, c_int),
    pango_layout_set_ellipsize: unsafe extern "C" fn(*mut c_void, c_int),
    pango_layout_set_alignment: unsafe extern "C" fn(*mut c_void, c_int),
    pango_layout_set_single_paragraph_mode: unsafe extern "C" fn(*mut c_void, c_int),
    pango_layout_get_pixel_size: unsafe extern "C" fn(*mut c_void, *mut c_int, *mut c_int),
    g_object_unref: unsafe extern "C" fn(*mut c_void),
}

// SAFETY: `PangoCairoApi` holds immutable function pointers and process-lifetime library handles.
unsafe impl Send for PangoCairoApi {}
unsafe impl Sync for PangoCairoApi {}

static PANGO_CAIRO_API: OnceLock<Option<PangoCairoApi>> = OnceLock::new();

impl PangoCairoApi {
    fn load() -> Option<&'static Self> {
        PANGO_CAIRO_API
            .get_or_init(|| {
                // SAFETY: Standard Cairo/Pango/GObject C ABI entry points on Linux desktops.
                unsafe {
                    let cairo = Library::new("libcairo.so.2").ok()?;
                    let pango = Library::new("libpango-1.0.so.0").ok()?;
                    let pangocairo = Library::new("libpangocairo-1.0.so.0").ok()?;
                    let gobject = Library::new("libgobject-2.0.so.0").ok()?;
                    Some(Self {
                        image_surface_create_for_data: *cairo
                            .get(b"cairo_image_surface_create_for_data\0")
                            .ok()?,
                        surface_flush: *cairo.get(b"cairo_surface_flush\0").ok()?,
                        surface_destroy: *cairo.get(b"cairo_surface_destroy\0").ok()?,
                        cairo_create: *cairo.get(b"cairo_create\0").ok()?,
                        cairo_destroy: *cairo.get(b"cairo_destroy\0").ok()?,
                        cairo_save: *cairo.get(b"cairo_save\0").ok()?,
                        cairo_restore: *cairo.get(b"cairo_restore\0").ok()?,
                        cairo_scale: *cairo.get(b"cairo_scale\0").ok()?,
                        cairo_rectangle: *cairo.get(b"cairo_rectangle\0").ok()?,
                        cairo_clip: *cairo.get(b"cairo_clip\0").ok()?,
                        cairo_move_to: *cairo.get(b"cairo_move_to\0").ok()?,
                        cairo_set_source_rgba: *cairo.get(b"cairo_set_source_rgba\0").ok()?,
                        pango_cairo_create_layout: *pangocairo
                            .get(b"pango_cairo_create_layout\0")
                            .ok()?,
                        pango_cairo_show_layout: *pangocairo
                            .get(b"pango_cairo_show_layout\0")
                            .ok()?,
                        pango_font_description_from_string: *pango
                            .get(b"pango_font_description_from_string\0")
                            .ok()?,
                        pango_font_description_free: *pango
                            .get(b"pango_font_description_free\0")
                            .ok()?,
                        pango_layout_set_font_description: *pango
                            .get(b"pango_layout_set_font_description\0")
                            .ok()?,
                        pango_layout_set_text: *pango.get(b"pango_layout_set_text\0").ok()?,
                        pango_layout_set_width: *pango.get(b"pango_layout_set_width\0").ok()?,
                        pango_layout_set_wrap: *pango.get(b"pango_layout_set_wrap\0").ok()?,
                        pango_layout_set_ellipsize: *pango
                            .get(b"pango_layout_set_ellipsize\0")
                            .ok()?,
                        pango_layout_set_alignment: *pango
                            .get(b"pango_layout_set_alignment\0")
                            .ok()?,
                        pango_layout_set_single_paragraph_mode: *pango
                            .get(b"pango_layout_set_single_paragraph_mode\0")
                            .ok()?,
                        pango_layout_get_pixel_size: *pango
                            .get(b"pango_layout_get_pixel_size\0")
                            .ok()?,
                        g_object_unref: *gobject.get(b"g_object_unref\0").ok()?,
                        _cairo: cairo,
                        _pango: pango,
                        _pangocairo: pangocairo,
                        _gobject: gobject,
                    })
                }
            })
            .as_ref()
    }
}

#[derive(Clone, Copy)]
enum TextAlign {
    Left,
    Center,
}

struct TextDrawCmd<'a> {
    text: &'a str,
    rect: LocalRect,
    font_px: u32,
    color: Rgba,
    align: TextAlign,
    wrapped: bool,
}

fn panel_pango_font_desc(family: &str, style: &str, font_px: u32) -> String {
    let pango_family = match family {
        "bahnschrift" => "Ubuntu, Sans",
        "arial" => "Arial, Liberation Sans, Sans",
        "verdana" => "Verdana, DejaVu Sans, Sans",
        "georgia" => "Georgia, DejaVu Serif, Serif",
        "consolas" => "Consolas, DejaVu Sans Mono, Monospace",
        _ => "Sans",
    };
    let pango_style = match style {
        "bold" => " Bold",
        "italic" => " Italic",
        "bold-italic" => " Bold Italic",
        _ => "",
    };
    format!("{pango_family}{pango_style} {font_px}px")
}

/// Measure wrapped UTF-8 text inside `max_width_px` logical pixels using Pango.
fn measure_wrapped_text(text: &str, font_desc: &str, font_px: u32, max_width_px: i32) -> (i32, i32) {
    let fallback = || {
        let chars = text.chars().count().max(1) as i32;
        let char_w = ((font_px as f32) * 0.56).max(6.0) as i32;
        let max_chars_per_line = (max_width_px / char_w).max(1);
        let lines = ((chars + max_chars_per_line - 1) / max_chars_per_line).max(1);
        let w = (chars.min(max_chars_per_line) * char_w).min(max_width_px);
        let h = lines * ((font_px as f32) * 1.35).ceil() as i32;
        (w.max(1), h.max(1))
    };
    let Some(pc) = PangoCairoApi::load() else {
        return fallback();
    };
    let Ok(font_cstr) = CString::new(font_desc) else {
        return fallback();
    };
    // SAFETY: Draws nothing; uses a 4-byte 1×1 ARGB surface solely to measure Pango layout extents.
    unsafe {
        let mut scratch = [0u8; 4];
        let surf =
            (pc.image_surface_create_for_data)(scratch.as_mut_ptr(), CAIRO_FORMAT_ARGB32, 1, 1, 4);
        if surf.is_null() {
            return fallback();
        }
        let cr = (pc.cairo_create)(surf);
        let layout = (pc.pango_cairo_create_layout)(cr);
        let fdesc = (pc.pango_font_description_from_string)(font_cstr.as_ptr());
        if !fdesc.is_null() {
            (pc.pango_layout_set_font_description)(layout, fdesc);
        }
        (pc.pango_layout_set_width)(layout, max_width_px.max(1) * PANGO_SCALE);
        (pc.pango_layout_set_wrap)(layout, PANGO_WRAP_WORD_CHAR);
        (pc.pango_layout_set_alignment)(layout, PANGO_ALIGN_LEFT);
        (pc.pango_layout_set_text)(layout, text.as_ptr().cast(), text.len() as c_int);
        let mut w: c_int = 0;
        let mut h: c_int = 0;
        (pc.pango_layout_get_pixel_size)(layout, &mut w, &mut h);
        if !fdesc.is_null() {
            (pc.pango_font_description_free)(fdesc);
        }
        (pc.g_object_unref)(layout);
        (pc.cairo_destroy)(cr);
        (pc.surface_destroy)(surf);
        (w.max(1), h.max(1))
    }
}

/// Composite crisp native-DPI Pango/Cairo text commands directly onto a physical premultiplied-BGRA
/// canvas whose logical coordinate space is `(logical_w, logical_h)`.
fn draw_pango_texts(
    phys: &mut PremulCanvas,
    logical_w: u32,
    logical_h: u32,
    alpha_mul: f32,
    cmds: &[TextDrawCmd<'_>],
) {
    draw_pango_texts_with_font(phys, logical_w, logical_h, alpha_mul, None, cmds);
}

fn draw_pango_texts_with_font(
    phys: &mut PremulCanvas,
    logical_w: u32,
    logical_h: u32,
    alpha_mul: f32,
    font_desc_override: Option<&str>,
    cmds: &[TextDrawCmd<'_>],
) {
    if cmds.is_empty() || phys.width == 0 || phys.height == 0 || logical_w == 0 || logical_h == 0 {
        return;
    }
    let Some(pc) = PangoCairoApi::load() else {
        return;
    };
    let sx = f64::from(phys.width) / f64::from(logical_w);
    let sy = f64::from(phys.height) / f64::from(logical_h);
    let amul = f64::from(alpha_mul.clamp(0.0, 1.0));

    // SAFETY: `phys.pixels` holds `phys.width * phys.height * 4` bytes matching `CAIRO_FORMAT_ARGB32`.
    unsafe {
        let surf = (pc.image_surface_create_for_data)(
            phys.pixels.as_mut_ptr(),
            CAIRO_FORMAT_ARGB32,
            phys.width as c_int,
            phys.height as c_int,
            (phys.width * 4) as c_int,
        );
        if surf.is_null() {
            return;
        }
        let cr = (pc.cairo_create)(surf);
        (pc.cairo_scale)(cr, sx, sy);

        for cmd in cmds {
            if cmd.text.is_empty() || cmd.rect.width == 0 || cmd.rect.height == 0 {
                continue;
            }
            let desc_owned;
            let desc_str = match font_desc_override {
                Some(custom) => custom,
                None => {
                    desc_owned = format!("Sans {}px", cmd.font_px);
                    &desc_owned
                }
            };
            let Ok(font_cstr) = CString::new(desc_str) else {
                continue;
            };
            let layout = (pc.pango_cairo_create_layout)(cr);
            let fdesc = (pc.pango_font_description_from_string)(font_cstr.as_ptr());
            if !fdesc.is_null() {
                (pc.pango_layout_set_font_description)(layout, fdesc);
            }
            (pc.pango_layout_set_width)(layout, (cmd.rect.width as c_int) * PANGO_SCALE);
            if cmd.wrapped {
                (pc.pango_layout_set_wrap)(layout, PANGO_WRAP_WORD_CHAR);
            } else {
                (pc.pango_layout_set_single_paragraph_mode)(layout, 1);
                (pc.pango_layout_set_ellipsize)(layout, PANGO_ELLIPSIZE_END);
            }
            (pc.pango_layout_set_alignment)(
                layout,
                match cmd.align {
                    TextAlign::Left => PANGO_ALIGN_LEFT,
                    TextAlign::Center => PANGO_ALIGN_CENTER,
                },
            );
            (pc.pango_layout_set_text)(layout, cmd.text.as_ptr().cast(), cmd.text.len() as c_int);

            let mut tw: c_int = 0;
            let mut th: c_int = 0;
            (pc.pango_layout_get_pixel_size)(layout, &mut tw, &mut th);

            let y_off = if cmd.wrapped {
                f64::from(cmd.rect.y)
            } else {
                f64::from(cmd.rect.y)
                    + ((f64::from(cmd.rect.height) - f64::from(th)) * 0.5).max(0.0)
            };

            (pc.cairo_save)(cr);
            (pc.cairo_rectangle)(
                cr,
                f64::from(cmd.rect.x),
                f64::from(cmd.rect.y),
                f64::from(cmd.rect.width),
                f64::from(cmd.rect.height),
            );
            (pc.cairo_clip)(cr);
            (pc.cairo_set_source_rgba)(
                cr,
                f64::from(cmd.color.r) / 255.0,
                f64::from(cmd.color.g) / 255.0,
                f64::from(cmd.color.b) / 255.0,
                (f64::from(cmd.color.a) / 255.0) * amul,
            );
            (pc.cairo_move_to)(cr, f64::from(cmd.rect.x), y_off);
            (pc.pango_cairo_show_layout)(cr, layout);
            (pc.cairo_restore)(cr);

            if !fdesc.is_null() {
                (pc.pango_font_description_free)(fdesc);
            }
            (pc.g_object_unref)(layout);
        }

        (pc.cairo_destroy)(cr);
        (pc.surface_flush)(surf);
        (pc.surface_destroy)(surf);
    }
}

/// Parse `Xft.dpi:` from X11 `RESOURCE_MANAGER` text into a HiDPI display scale (`1.0..=2.5`).
fn parse_xft_dpi_scale(resources: &str) -> Option<f64> {
    for line in resources.lines() {
        if let Some((k, v)) = line.split_once(':')
            && k.trim().eq_ignore_ascii_case("xft.dpi")
            && let Ok(dpi) = v.trim().parse::<f64>()
            && dpi.is_finite()
            && dpi >= 72.0
        {
            return Some((dpi / 96.0).clamp(1.0, 2.5));
        }
    }
    None
}

unsafe fn query_dpi_scale(api: &X11OverlayApi, dpy: *mut c_void, root: c_ulong) -> f64 {
    // SAFETY: Caller provides a valid X11 `dpy` and `root`.
    unsafe {
        let rm_atom = (api.intern_atom)(dpy, c"RESOURCE_MANAGER".as_ptr(), 1);
        if rm_atom == 0 {
            return 1.0;
        }
        let mut actual_type: c_ulong = 0;
        let mut actual_format: c_int = 0;
        let mut nitems: c_ulong = 0;
        let mut bytes_after: c_ulong = 0;
        let mut prop: *mut c_uchar = null_mut();
        let st = (api.get_window_property)(
            dpy,
            root,
            rm_atom,
            0,
            16_384,
            0,
            0,
            &mut actual_type,
            &mut actual_format,
            &mut nitems,
            &mut bytes_after,
            &mut prop,
        );
        let scale = if st == 0 && !prop.is_null() && actual_format == 8 {
            let bytes = std::slice::from_raw_parts(prop, nitems as usize);
            let text = String::from_utf8_lossy(bytes);
            parse_xft_dpi_scale(&text).unwrap_or(1.0)
        } else {
            1.0
        };
        if !prop.is_null() {
            (api.free)(prop.cast());
        }
        scale
    }
}

unsafe fn query_active_work_area_phys(
    api: &X11OverlayApi,
    dpy: *mut c_void,
    screen: c_int,
    root: c_ulong,
    monitor_mode: &str,
) -> WorkArea {
    // SAFETY: Caller provides a valid X11 `dpy`, `screen`, and `root`.
    unsafe {
        if let (Some(get_mons), Some(free_mons)) = (api.xrr_get_monitors, api.xrr_free_monitors) {
            let mut root_ret: c_ulong = 0;
            let mut child_ret: c_ulong = 0;
            let mut rx: c_int = 0;
            let mut ry: c_int = 0;
            let mut wx: c_int = 0;
            let mut wy: c_int = 0;
            let mut mask: c_uint = 0;
            let _ = (api.query_pointer)(
                dpy,
                root,
                &mut root_ret,
                &mut child_ret,
                &mut rx,
                &mut ry,
                &mut wx,
                &mut wy,
                &mut mask,
            );
            let mut n: c_int = 0;
            let ptr = get_mons(dpy, root, 1, &mut n);
            if !ptr.is_null() && n > 0 {
                let slice = std::slice::from_raw_parts(ptr, n as usize);
                let hit = if monitor_mode == "primary" {
                    slice.iter().find(|m| m.primary != 0).or_else(|| slice.first())
                } else {
                    slice
                        .iter()
                        .find(|m| {
                            rx >= m.x && rx < m.x + m.width && ry >= m.y && ry < m.y + m.height
                        })
                        .or_else(|| slice.iter().find(|m| m.primary != 0))
                        .or_else(|| slice.first())
                };
                let area = hit
                    .filter(|m| m.width > 0 && m.height > 0)
                    .map(|m| WorkArea {
                        left: m.x,
                        top: m.y,
                        right: m.x + m.width,
                        bottom: m.y + m.height,
                    });
                free_mons(ptr);
                if let Some(a) = area {
                    return a;
                }
            }
        }
        WorkArea {
            left: 0,
            top: 0,
            right: (api.display_width)(dpy, screen),
            bottom: (api.display_height)(dpy, screen),
        }
    }
}

fn phys_to_logical_area(phys: WorkArea, scale: f64) -> WorkArea {
    let s = if scale.is_finite() {
        scale.clamp(1.0, 2.5)
    } else {
        1.0
    };
    WorkArea {
        left: (f64::from(phys.left) / s).round() as i32,
        top: (f64::from(phys.top) / s).round() as i32,
        right: (f64::from(phys.right) / s).round() as i32,
        bottom: (f64::from(phys.bottom) / s).round() as i32,
    }
}

fn logical_to_phys_rect(r: OverlayRect, scale: f64) -> OverlayRect {
    let s = if scale.is_finite() {
        scale.clamp(1.0, 2.5)
    } else {
        1.0
    };
    OverlayRect {
        x: (f64::from(r.x) * s).round() as i32,
        y: (f64::from(r.y) * s).round() as i32,
        width: (f64::from(r.width) * s).round().max(1.0) as u32,
        height: (f64::from(r.height) * s).round().max(1.0) as u32,
    }
}

fn phys_to_logical_coord(v: i32, scale: f64) -> i32 {
    let s = if scale.is_finite() {
        scale.clamp(1.0, 2.5)
    } else {
        1.0
    };
    (f64::from(v) / s).floor() as i32
}

unsafe fn set_window_click_through(
    api: &X11OverlayApi,
    dpy: *mut c_void,
    win: c_ulong,
    click_through: bool,
    phys_w: u32,
    phys_h: u32,
) {
    // SAFETY: Configures XShape `ShapeInput` region on a valid X11 window.
    unsafe {
        if let Some(shape_rects) = api.xshape_combine_rectangles {
            if click_through {
                shape_rects(dpy, win, SHAPE_INPUT, 0, 0, null(), 0, SHAPE_SET, 0);
            } else {
                let rect = XRectangle {
                    x: 0,
                    y: 0,
                    width: phys_w.clamp(1, u32::from(u16::MAX)) as u16,
                    height: phys_h.clamp(1, u32::from(u16::MAX)) as u16,
                };
                shape_rects(dpy, win, SHAPE_INPUT, 0, 0, &rect, 1, SHAPE_SET, 0);
            }
        }
    }
}

unsafe fn apply_overlay_window_hints(
    api: &X11OverlayApi,
    dpy: *mut c_void,
    win: c_ulong,
    click_through: bool,
) {
    // SAFETY: Caller provides a valid X11 `dpy` and newly created `win`.
    unsafe {
        let wm_type = (api.intern_atom)(dpy, c"_NET_WM_WINDOW_TYPE".as_ptr(), 0);
        let type_dock = (api.intern_atom)(dpy, c"_NET_WM_WINDOW_TYPE_DOCK".as_ptr(), 0);
        if wm_type != 0 && type_dock != 0 {
            let val: [c_ulong; 1] = [type_dock];
            (api.change_property)(
                dpy,
                win,
                wm_type,
                XA_ATOM,
                32,
                PROP_MODE_REPLACE,
                val.as_ptr().cast(),
                1,
            );
        }

        let wm_state = (api.intern_atom)(dpy, c"_NET_WM_STATE".as_ptr(), 0);
        if wm_state != 0 {
            let states: [c_ulong; 4] = [
                (api.intern_atom)(dpy, c"_NET_WM_STATE_ABOVE".as_ptr(), 0),
                (api.intern_atom)(dpy, c"_NET_WM_STATE_SKIP_TASKBAR".as_ptr(), 0),
                (api.intern_atom)(dpy, c"_NET_WM_STATE_SKIP_PAGER".as_ptr(), 0),
                (api.intern_atom)(dpy, c"_NET_WM_STATE_STICKY".as_ptr(), 0),
            ];
            let valid: Vec<c_ulong> = states.into_iter().filter(|&a| a != 0).collect();
            if !valid.is_empty() {
                (api.change_property)(
                    dpy,
                    win,
                    wm_state,
                    XA_ATOM,
                    32,
                    PROP_MODE_REPLACE,
                    valid.as_ptr().cast(),
                    valid.len() as c_int,
                );
            }
        }

        let wm_pid = (api.intern_atom)(dpy, c"_NET_WM_PID".as_ptr(), 0);
        if wm_pid != 0 {
            let pid = c_ulong::from(std::process::id());
            (api.change_property)(
                dpy,
                win,
                wm_pid,
                XA_CARDINAL,
                32,
                PROP_MODE_REPLACE,
                (&raw const pid).cast(),
                1,
            );
        }

        if click_through {
            set_window_click_through(api, dpy, win, true, 1, 1);
        }
    }
}

unsafe fn create_overlay_win(
    api: &X11OverlayApi,
    dpy: *mut c_void,
    root: c_ulong,
    vinfo: &XVisualInfo,
    colormap: c_ulong,
    click_through: bool,
    event_mask: c_long,
) -> (c_ulong, *mut c_void) {
    // SAFETY: Creates a 32-bit ARGB borderless non-activating `override_redirect` window + GC.
    unsafe {
        let mut attrs: XSetWindowAttributes = std::mem::zeroed();
        attrs.background_pixel = 0;
        attrs.border_pixel = 0;
        attrs.override_redirect = 1;
        attrs.colormap = colormap;
        attrs.event_mask = event_mask;

        let win = (api.create_window)(
            dpy,
            root,
            0,
            0,
            16,
            16,
            0,
            vinfo.depth,
            INPUT_OUTPUT,
            vinfo.visual,
            CW_BACK_PIXEL
                | CW_BORDER_PIXEL
                | CW_OVERRIDE_REDIRECT
                | CW_EVENT_MASK
                | CW_COLORMAP,
            &mut attrs,
        );
        apply_overlay_window_hints(api, dpy, win, click_through);
        let gc = (api.create_gc)(dpy, win, 0, null_mut());
        (win, gc)
    }
}

#[allow(clippy::too_many_arguments)]
unsafe fn blit_pixels(
    api: &X11OverlayApi,
    dpy: *mut c_void,
    vinfo: &XVisualInfo,
    win: c_ulong,
    gc: *mut c_void,
    width: u32,
    height: u32,
    pixels: &mut [u8],
) {
    let expected = (width as usize) * (height as usize) * 4;
    if width == 0 || height == 0 || pixels.len() < expected {
        return;
    }
    // SAFETY: `XCreateImage` wraps `pixels` without copying; we free only the XImage header struct
    // via `XFree` (never `XDestroyImage`, which would `free()` Rust's buffer).
    unsafe {
        let ximg = (api.create_image)(
            dpy,
            vinfo.visual,
            vinfo.depth as c_uint,
            Z_PIXMAP,
            0,
            pixels.as_mut_ptr().cast(),
            width,
            height,
            32,
            (width * 4) as c_int,
        );
        if !ximg.is_null() {
            (api.put_image)(dpy, win, gc, ximg, 0, 0, 0, 0, width, height);
            (api.flush)(dpy);
            (api.free)(ximg.cast());
        }
    }
}

struct LinuxOverlayState {
    api: &'static X11OverlayApi,
    dpy: *mut c_void,
    screen: c_int,
    root: c_ulong,
    vinfo: XVisualInfo,
    colormap: c_ulong,

    pill_win: c_ulong,
    pill_gc: *mut c_void,
    panel_win: c_ulong,
    panel_gc: *mut c_void,
    bar_win: c_ulong,
    bar_gc: *mut c_void,
    popup_win: c_ulong,
    popup_gc: *mut c_void,
    recovery_win: c_ulong,
    recovery_gc: *mut c_void,

    config: OverlayConfig,
    on_action: Option<OverlayActionCallback>,
    flag_provider: Option<FlagBitmapProvider>,
    flag_cache: HashMap<String, Option<(u32, u32, Vec<u8>)>>,

    dpi_scale: f64,
    active_work_area: WorkArea,
    pill_rect: OverlayRect,
    pill_shown: bool,
    visible: bool,
    animator: PillAnimator,
    last_tick: Instant,
    session_start: Instant,
    last_elapsed_secs: u64,

    // Pointer hover flags driven by X11 Enter/Leave events (crucial on GNOME Wayland where
    // `XQueryPointer` only updates when a window receives pointer input).
    pill_entered: bool,
    bar_entered: bool,
    popup_entered: bool,

    // Live transcription panel (above bottom pill / below top pill)
    panel_shown: bool,
    panel_text: String,
    panel_rect: OverlayRect,
    panel_logical_canvas: PremulCanvas,
    panel_phys_canvas: PremulCanvas,
    last_panel_alpha: u8,

    // Hover-reveal / pinnable under-pill interactive control capsule
    visibility: UnderbarVisibility,
    bar_shown: bool,
    bar_layout: UnderbarLayout,
    bar_placement: UnderbarPlacement,
    bar_logical_canvas: PremulCanvas,
    bar_phys_canvas: PremulCanvas,
    hovered_bar_control: Option<UnderbarControl>,
    bar_hover_blend: f32,
    last_bar_alpha: u8,
    bar_dirty: bool,

    // Searchable 99-language popup menu
    popup_shown: bool,
    popup_rect: OverlayRect,
    popup_layout: LangPopupLayout,
    popup_logical_canvas: PremulCanvas,
    popup_phys_canvas: PremulCanvas,
    strip_logical_canvas: PremulCanvas,
    strip_phys_canvas: PremulCanvas,
    search_query: String,
    list_scroll: usize,
    recent_scroll: usize,
    hovered_popup_hit: Option<PopupHit>,
    keyboard_selected_row: Option<usize>,
    popup_dirty: bool,
    prev_mouse_down: bool,

    // Floating failed-paste recovery card
    recovery_shown: bool,
    recovery_rect: OverlayRect,
    recovery_layout: RecoveryCardLayout,
    recovery_paste_target: Option<String>,
    recovery_logical_canvas: PremulCanvas,
    recovery_phys_canvas: PremulCanvas,
    hovered_recovery_btn: Option<RecoveryCardButton>,
}

impl LinuxOverlayState {
    fn get_flag(&mut self, code: &str) -> Option<(u32, u32, Vec<u8>)> {
        let key = code.trim().to_ascii_lowercase();
        if key.is_empty() {
            return None;
        }
        if let Some(cached) = self.flag_cache.get(&key) {
            return cached.clone();
        }
        let fetched = self.flag_provider.as_ref().and_then(|p| p(&key));
        self.flag_cache.insert(key, fetched.clone());
        fetched
    }

    fn emit_action(&self, action: OverlayAction) {
        if let Some(cb) = self.on_action.as_ref().map(Arc::clone) {
            cb(action);
        }
    }

    fn update_pill_geometry(&mut self) {
        // SAFETY: Valid X11 display and root window owned by this thread.
        unsafe {
            self.dpi_scale = query_dpi_scale(self.api, self.dpy, self.root);
            let phys_area = query_active_work_area_phys(
                self.api,
                self.dpy,
                self.screen,
                self.root,
                &self.config.monitor,
            );
            self.active_work_area = phys_to_logical_area(phys_area, self.dpi_scale);
        }
        let (pw, ph) = pill_size(self.config.pill_scale);
        let (px, py) = underbar::calc_pill_position_with_controls(
            &self.config.position,
            pw,
            ph,
            self.active_work_area,
            self.config.controls_enabled,
        );
        self.pill_rect = OverlayRect {
            x: px,
            y: py,
            width: pw,
            height: ph,
        };
        let phys = logical_to_phys_rect(self.pill_rect, self.dpi_scale);
        // SAFETY: Resizes pill window and configures click-through vs hover sensitivity.
        unsafe {
            (self.api.move_resize_window)(
                self.dpy,
                self.pill_win,
                phys.x,
                phys.y,
                phys.width,
                phys.height,
            );
            set_window_click_through(
                self.api,
                self.dpy,
                self.pill_win,
                !self.config.controls_enabled,
                phys.width,
                phys.height,
            );
            let pill_mask = if self.config.controls_enabled {
                ENTER_WINDOW_MASK | LEAVE_WINDOW_MASK | POINTER_MOTION_MASK
            } else {
                0
            };
            (self.api.select_input)(self.dpy, self.pill_win, pill_mask);
        }
        self.recompute_bar_geometry();
    }

    fn recompute_bar_geometry(&mut self) {
        self.bar_layout = UnderbarLayout::compute(false);
        self.bar_placement = underbar::calc_underbar_placement(
            self.pill_rect,
            self.bar_layout.width,
            self.active_work_area,
        );
        self.bar_dirty = true;
    }

    fn recompute_popup_layout(&mut self) {
        let filtered = underbar::filter_languages(&self.search_query);
        self.popup_layout = LangPopupLayout::compute(
            filtered.len(),
            &self.config.recent_languages,
            self.recent_scroll,
        );
        self.list_scroll = self.list_scroll.min(self.popup_layout.max_list_scroll());
        self.recent_scroll = self.popup_layout.recent_strip.scroll_index;
        self.popup_rect = underbar::calc_lang_popup_rect(
            self.bar_placement.bar_rect,
            self.bar_layout.language,
            self.popup_layout.width,
            self.popup_layout.height,
            self.active_work_area,
        );
        self.popup_dirty = true;
    }

    fn hide_panel_now(&mut self) {
        if self.panel_shown {
            // SAFETY: Unmaps live transcription panel window.
            unsafe {
                (self.api.unmap_window)(self.dpy, self.panel_win);
                (self.api.flush)(self.dpy);
            }
            self.panel_shown = false;
        }
        self.panel_text.clear();
    }

    fn hide_recovery_now(&mut self) {
        if self.recovery_shown {
            // SAFETY: Unmaps floating recovery action card.
            unsafe {
                (self.api.unmap_window)(self.dpy, self.recovery_win);
                (self.api.flush)(self.dpy);
            }
            self.recovery_shown = false;
            self.hovered_recovery_btn = None;
        }
    }

    fn close_lang_popup(&mut self) {
        self.visibility.set_lang_menu_open(false);
        if self.popup_shown {
            // SAFETY: Unmaps language dropdown window.
            unsafe {
                (self.api.unmap_window)(self.dpy, self.popup_win);
                (self.api.flush)(self.dpy);
            }
            self.popup_shown = false;
            self.popup_entered = false;
            self.hovered_popup_hit = None;
            self.keyboard_selected_row = None;
            self.search_query.clear();
            self.list_scroll = 0;
            self.recent_scroll = 0;
            self.bar_dirty = true;
        }
    }

    fn hide_bar_now(&mut self) {
        self.close_lang_popup();
        if self.bar_shown {
            // SAFETY: Unmaps under-pill bar window.
            unsafe {
                (self.api.unmap_window)(self.dpy, self.bar_win);
                (self.api.flush)(self.dpy);
            }
            self.bar_shown = false;
            self.bar_entered = false;
            self.hovered_bar_control = None;
        }
    }

    fn sync_bar_visibility(&mut self) {
        let should_show = self.visible && self.config.controls_enabled && self.visibility.visible;
        let changed = should_show != self.bar_shown;
        if should_show {
            if !self.bar_shown {
                self.bar_shown = true;
                self.bar_dirty = true;
                let phys = logical_to_phys_rect(self.bar_placement.bar_rect, self.dpi_scale);
                // SAFETY: Positions and raises under-pill bar without activating.
                unsafe {
                    (self.api.move_resize_window)(
                        self.dpy,
                        self.bar_win,
                        phys.x,
                        phys.y,
                        phys.width,
                        phys.height,
                    );
                    (self.api.map_raised)(self.dpy, self.bar_win);
                }
                self.render_bar_frame();
            }
        } else if self.bar_shown {
            self.hide_bar_now();
        }
        if changed && self.panel_shown {
            self.layout_and_show_panel();
        }
    }

    fn cmd_show(&mut self, text: String) {
        if !self.config.enabled {
            return;
        }
        self.hide_recovery_now();
        self.hide_panel_now();
        self.close_lang_popup();
        self.update_pill_geometry();

        if !self.visible {
            self.visible = true;
            self.session_start = Instant::now();
            self.last_elapsed_secs = 0;
            self.animator.reset();
            self.animator.fade_target = 1.0;
            self.last_tick = Instant::now();
            self.pill_entered = false;
            self.bar_entered = false;
            self.popup_entered = false;
            self.visibility
                .reset_for_session(self.config.controls_enabled, self.config.controls_pinned);
            if !self.pill_shown {
                // SAFETY: Maps iridescent pill overlay above all windows without stealing focus.
                unsafe {
                    (self.api.map_raised)(self.dpy, self.pill_win);
                }
                self.pill_shown = true;
            }
            self.render_pill_frame(0.0);
            self.sync_bar_visibility();
        } else {
            self.animator.fade_target = 1.0;
            self.animator.alpha_logical = 1.0;
            self.render_pill_frame(0.0);
            self.sync_bar_visibility();
        }
        self.cmd_update_text(text);
    }

    fn cmd_update_text(&mut self, text: String) {
        if !self.visible {
            return;
        }
        if !self.config.show_text || !pure::should_show_panel(&text) {
            self.hide_panel_now();
            return;
        }
        self.panel_text = pure::tail_text(text.trim());
        self.layout_and_show_panel();
    }

    fn cmd_set_finishing(&mut self, finishing: bool) {
        if finishing {
            self.close_lang_popup();
        }
        self.visibility.set_finishing(finishing);
        self.bar_dirty = true;
        if self.visible {
            self.sync_bar_visibility();
            if self.bar_shown {
                self.render_bar_frame();
            }
        }
    }

    fn cmd_update_session_meta(
        &mut self,
        language: String,
        delivery_mode: String,
        pinned: bool,
        recent_languages: Vec<String>,
    ) {
        self.config.language = language;
        self.config.delivery_mode = delivery_mode;
        self.config.controls_pinned = pinned;
        self.config.recent_languages = recent_languages;
        self.visibility.set_pinned(pinned);
        self.recompute_bar_geometry();
        if self.visible {
            self.sync_bar_visibility();
            if self.bar_shown {
                let phys = logical_to_phys_rect(self.bar_placement.bar_rect, self.dpi_scale);
                // SAFETY: Updates underbar bounds in place.
                unsafe {
                    (self.api.move_resize_window)(
                        self.dpy,
                        self.bar_win,
                        phys.x,
                        phys.y,
                        phys.width,
                        phys.height,
                    );
                }
                self.render_bar_frame();
            }
            if self.popup_shown {
                self.recompute_popup_layout();
                self.position_and_render_popup();
            }
        }
    }

    fn cmd_update_config(&mut self, cfg: OverlayConfig) {
        let was_enabled = self.config.enabled;
        self.config = cfg;
        self.visibility
            .set_controls_enabled(self.config.controls_enabled);
        self.visibility.set_pinned(self.config.controls_pinned);
        if !self.config.enabled && was_enabled && self.visible {
            self.cmd_hide();
            return;
        }
        if !self.config.show_text {
            self.hide_panel_now();
        }
        if !self.config.controls_enabled {
            self.hide_bar_now();
        }
        if self.visible {
            self.update_pill_geometry();
            self.render_pill_frame(0.0);
            self.sync_bar_visibility();
            if self.bar_shown {
                let phys = logical_to_phys_rect(self.bar_placement.bar_rect, self.dpi_scale);
                // SAFETY: Updates underbar window geometry.
                unsafe {
                    (self.api.move_resize_window)(
                        self.dpy,
                        self.bar_win,
                        phys.x,
                        phys.y,
                        phys.width,
                        phys.height,
                    );
                }
                self.render_bar_frame();
            }
            if self.popup_shown {
                self.recompute_popup_layout();
                self.position_and_render_popup();
            }
            if self.panel_shown {
                self.layout_and_show_panel();
            }
        }
    }

    fn cmd_show_recovery(&mut self, _text: String, paste_target: Option<String>) {
        if !self.config.enabled {
            return;
        }
        self.update_pill_geometry();
        self.recovery_paste_target = paste_target;
        self.recovery_layout =
            RecoveryCardLayout::compute_for(self.recovery_paste_target.is_some());
        self.recovery_rect = underbar::calc_recovery_card_rect(
            self.pill_rect,
            self.recovery_layout.width,
            self.recovery_layout.height,
            self.active_work_area,
        );
        self.hovered_recovery_btn = None;
        self.recovery_shown = true;
        let phys = logical_to_phys_rect(self.recovery_rect, self.dpi_scale);
        // SAFETY: Surfaces floating failed-paste recovery card without stealing focus.
        unsafe {
            (self.api.move_resize_window)(
                self.dpy,
                self.recovery_win,
                phys.x,
                phys.y,
                phys.width,
                phys.height,
            );
            (self.api.map_raised)(self.dpy, self.recovery_win);
        }
        self.render_recovery_frame();
    }

    fn cmd_hide(&mut self) {
        self.hide_panel_now();
        self.hide_bar_now();
        self.visibility.reset_for_hide();
        self.pill_entered = false;
        if !self.visible {
            return;
        }
        self.visible = false;
        self.animator.level_raw = 0.0;
        self.animator.lvl_fast = 0.0;
        self.animator.lvl_slow = 0.0;
        self.animator.voice.reset();
        self.animator.voice_seen = false;
        self.animator.fade_target = 0.0;
    }

    fn panel_font_px(&self) -> u32 {
        ((f32::from(self.config.font_size.clamp(10, 28) as u16)) * (96.0 / 72.0))
            .round()
            .clamp(12.0, 38.0) as u32
    }

    fn layout_and_show_panel(&mut self) {
        let font_px = self.panel_font_px();
        let font_desc =
            panel_pango_font_desc(&self.config.font_family, &self.config.font_style, font_px);
        let max_text_w = (PANEL_MAX_W as i32 - 2 * PANEL_PADDING_X).max(1);
        let (text_w, text_h) = measure_wrapped_text(&self.panel_text, &font_desc, font_px, max_text_w);

        let panel_w = (text_w + 2 * PANEL_PADDING_X)
            .max(self.pill_rect.width as i32)
            .min(PANEL_MAX_W as i32) as u32;
        let panel_h = (text_h + 2 * PANEL_PADDING_Y).max(1) as u32;
        let active_bar = self.bar_shown.then_some(self.bar_placement.bar_rect);
        let rect = underbar::calc_panel_rect_with_underbar(
            &self.config.position,
            self.pill_rect,
            active_bar,
            panel_w,
            panel_h,
            self.active_work_area,
        );
        self.panel_rect = rect;
        let phys = logical_to_phys_rect(rect, self.dpi_scale);
        // SAFETY: Resizes and raises click-through live transcription panel without focus steal.
        unsafe {
            (self.api.move_resize_window)(
                self.dpy,
                self.panel_win,
                phys.x,
                phys.y,
                phys.width,
                phys.height,
            );
            if !self.panel_shown {
                (self.api.map_raised)(self.dpy, self.panel_win);
            }
        }
        self.panel_shown = true;
        self.render_panel_frame();
    }

    fn render_panel_frame(&mut self) {
        let lw = self.panel_rect.width.max(1);
        let lh = self.panel_rect.height.max(1);
        let phys = logical_to_phys_rect(self.panel_rect, self.dpi_scale);
        let dark = self.config.dark_theme;

        // Scale background and border alpha smoothly from 0 (100% see-through) up to full opacity
        // while keeping transcribed text glyphs 100% opaque and legible.
        let opacity = (self.config.panel_opacity as f32).clamp(0.0, 1.0);
        let bg_alpha = (opacity * if dark { 236.0 } else { 244.0 })
            .round()
            .clamp(0.0, 255.0) as u8;
        let border_alpha = (opacity * if dark { 48.0 } else { 40.0 })
            .round()
            .clamp(0.0, 255.0) as u8;
        let panel_bg = if dark {
            Rgba::new(0x14, 0x14, 0x18, bg_alpha)
        } else {
            Rgba::new(0xFF, 0xFF, 0xFF, bg_alpha)
        };
        let panel_border = (border_alpha > 0).then(|| {
            if dark {
                Rgba::new(0xFF, 0xFF, 0xFF, border_alpha)
            } else {
                Rgba::new(0x1A, 0x1C, 0x1F, border_alpha)
            }
        });
        let fg_color = if dark {
            Rgba::rgb(0xF5, 0xF6, 0xF8)
        } else {
            Rgba::rgb(0x1A, 0x1C, 0x1F)
        };

        self.panel_logical_canvas.reset(lw, lh);
        if bg_alpha > 0 || panel_border.is_some() {
            self.panel_logical_canvas.fill_rounded_rect(
                LocalRect::new(0, 0, lw, lh),
                8.0,
                panel_bg,
                panel_border,
            );
        }

        let fade_f = self.animator.alpha_logical.clamp(0.0, 1.0);
        self.panel_logical_canvas
            .resample_into(&mut self.panel_phys_canvas, phys.width, phys.height, fade_f);

        if !self.panel_text.is_empty() {
            let text_w = lw.saturating_sub((2 * PANEL_PADDING_X) as u32).max(1);
            let text_h = lh.saturating_sub((2 * PANEL_PADDING_Y) as u32).max(1);
            let font_px = self.panel_font_px();
            let font_desc =
                panel_pango_font_desc(&self.config.font_family, &self.config.font_style, font_px);
            let mut cmds = Vec::with_capacity(2);
            if opacity < 0.35 {
                let shadow_alpha = (((0.35 - opacity) / 0.35) * 145.0)
                    .round()
                    .clamp(0.0, 145.0) as u8;
                if shadow_alpha > 0 {
                    let shadow_color = if dark {
                        Rgba::new(0x00, 0x00, 0x00, shadow_alpha)
                    } else {
                        Rgba::new(0xFF, 0xFF, 0xFF, shadow_alpha)
                    };
                    cmds.push(TextDrawCmd {
                        text: &self.panel_text,
                        rect: LocalRect::new(PANEL_PADDING_X, PANEL_PADDING_Y + 1, text_w, text_h),
                        font_px,
                        color: shadow_color,
                        align: TextAlign::Left,
                        wrapped: true,
                    });
                }
            }
            cmds.push(TextDrawCmd {
                text: &self.panel_text,
                rect: LocalRect::new(PANEL_PADDING_X, PANEL_PADDING_Y, text_w, text_h),
                font_px,
                color: fg_color,
                align: TextAlign::Left,
                wrapped: true,
            });
            draw_pango_texts_with_font(
                &mut self.panel_phys_canvas,
                lw,
                lh,
                fade_f,
                Some(&font_desc),
                &cmds,
            );
        }

        // SAFETY: Blits rendered premultiplied-BGRA frame to `panel_win`.
        unsafe {
            blit_pixels(
                self.api,
                self.dpy,
                &self.vinfo,
                self.panel_win,
                self.panel_gc,
                phys.width,
                phys.height,
                &mut self.panel_phys_canvas.pixels,
            );
        }
        self.last_panel_alpha = (fade_f * 255.0).round() as u8;
    }

    fn effective_bar_alpha(&self) -> u8 {
        let a = self.animator.alpha_logical.clamp(0.0, 1.0);
        let ratio = UNDERBAR_REST_ALPHA_RATIO
            + (UNDERBAR_HOVER_ALPHA_RATIO - UNDERBAR_REST_ALPHA_RATIO)
                * self.bar_hover_blend.clamp(0.0, 1.0);
        (a * ratio * 255.0).round().clamp(0.0, 245.0) as u8
    }

    fn render_bar_frame(&mut self) {
        let layout = self.bar_layout;
        let theme = UnderbarTheme::for_theme(self.config.dark_theme);
        let finishing = self.visibility.finishing;
        let pinned = self.config.controls_pinned;
        let hovered = if finishing {
            None
        } else {
            self.hovered_bar_control
        };

        let elapsed_text = underbar::format_elapsed(self.last_elapsed_secs);
        let lang_code = underbar::format_language_code(&self.config.language);
        let delivery_label = underbar::delivery_mode_label(&self.config.delivery_mode);

        self.bar_logical_canvas.reset(layout.width, layout.height);

        // Capsule background + crisp 1px border.
        self.bar_logical_canvas.fill_rounded_rect(
            LocalRect::new(0, 0, layout.width, layout.height),
            UNDERBAR_RADIUS,
            theme.bar_bg,
            Some(theme.bar_border),
        );

        // 1. Cancel `✕` button.
        if hovered == Some(UnderbarControl::Cancel) {
            self.bar_logical_canvas
                .fill_rounded_rect(layout.cancel, 10.0, theme.hover_bg, None);
        }
        let cancel_fg = if finishing {
            theme.text_muted.with_alpha(110)
        } else if hovered == Some(UnderbarControl::Cancel) {
            theme.text_primary
        } else {
            theme.text_muted
        };
        self.bar_logical_canvas.draw_icon_x(
            (layout.cancel.x as f32) + (layout.cancel.width as f32) * 0.5,
            (layout.cancel.y as f32) + (layout.cancel.height as f32) * 0.5,
            3.4,
            1.35,
            cancel_fg,
        );

        // 2. Vertical 1px divider hairline.
        self.bar_logical_canvas
            .fill_rect(LocalRect::new(layout.divider_x, 6, 1, 14), theme.divider);

        let mut text_cmds: Vec<TextDrawCmd<'_>> = Vec::with_capacity(4);

        if finishing {
            let msg_rect = LocalRect::new(
                layout.timer.x,
                layout.timer.y,
                (layout.done.x - layout.timer.x - 4).max(1) as u32,
                layout.timer.height,
            );
            text_cmds.push(TextDrawCmd {
                text: FINISHING_LABEL,
                rect: msg_rect,
                font_px: 11,
                color: theme.text_primary,
                align: TextAlign::Center,
                wrapped: false,
            });
        } else {
            // 3. Live elapsed dictation timer (`M:SS`).
            text_cmds.push(TextDrawCmd {
                text: &elapsed_text,
                rect: layout.timer,
                font_px: 11,
                color: theme.text_muted,
                align: TextAlign::Center,
                wrapped: false,
            });

            // 4. Compact language chip (`EN ▾`).
            let lang_active = self.popup_shown || hovered == Some(UnderbarControl::Language);
            if lang_active {
                self.bar_logical_canvas
                    .fill_rounded_rect(layout.language, 9.0, theme.hover_bg, None);
            }
            let chevron_cx = (layout.language.right() - 7) as f32;
            let code_w = layout.language.width.saturating_sub(14).max(1);
            let code_rect = LocalRect::new(
                layout.language.x + 3,
                layout.language.y,
                code_w,
                layout.language.height,
            );
            text_cmds.push(TextDrawCmd {
                text: &lang_code,
                rect: code_rect,
                font_px: 11,
                color: if self.popup_shown {
                    theme.accent_text
                } else {
                    theme.text_primary
                },
                align: TextAlign::Center,
                wrapped: false,
            });
            self.bar_logical_canvas.draw_icon_chevron(
                chevron_cx,
                (layout.language.y as f32) + (layout.language.height as f32) * 0.5,
                self.popup_shown,
                if self.popup_shown {
                    theme.accent_text
                } else {
                    theme.text_muted
                },
            );

            // 5. Delivery mode toggle chip (`Batch` / `Live`).
            let is_live = underbar::normalize_delivery_mode(&self.config.delivery_mode) == "live";
            if hovered == Some(UnderbarControl::DeliveryMode) {
                self.bar_logical_canvas.fill_rounded_rect(
                    layout.delivery_mode,
                    9.0,
                    theme.hover_bg,
                    None,
                );
            }
            text_cmds.push(TextDrawCmd {
                text: delivery_label,
                rect: layout.delivery_mode,
                font_px: 11,
                color: if is_live {
                    theme.accent_text
                } else {
                    theme.text_primary
                },
                align: TextAlign::Center,
                wrapped: false,
            });

            // 6. Pushpin icon (pinned colors ONLY the pin vector bright blue, no background circle).
            if hovered == Some(UnderbarControl::Pin) {
                self.bar_logical_canvas
                    .fill_rounded_rect(layout.pin, 9.0, theme.hover_bg, None);
            }
            let pin_color = if pinned {
                theme.pin_active
            } else if hovered == Some(UnderbarControl::Pin) {
                theme.text_primary
            } else {
                theme.pin_inactive
            };
            self.bar_logical_canvas.draw_icon_pushpin(
                (layout.pin.x as f32) + (layout.pin.width as f32) * 0.5,
                (layout.pin.y as f32) + (layout.pin.height as f32) * 0.5,
                pin_color,
            );
        }

        // 7. Done `✓` button.
        let done_bg = if finishing {
            theme.done_bg.with_alpha(150)
        } else if hovered == Some(UnderbarControl::Done) {
            theme.done_hover_bg
        } else {
            theme.done_bg
        };
        self.bar_logical_canvas
            .fill_rounded_rect(layout.done, 10.0, done_bg, None);
        self.bar_logical_canvas.draw_icon_check(
            (layout.done.x as f32) + (layout.done.width as f32) * 0.5,
            (layout.done.y as f32) + (layout.done.height as f32) * 0.5,
            1.0,
            1.55,
            theme.done_fg,
        );

        let bar_alpha = self.effective_bar_alpha();
        let alpha_f = f32::from(bar_alpha) / 255.0;
        let phys = logical_to_phys_rect(self.bar_placement.bar_rect, self.dpi_scale);
        self.bar_logical_canvas
            .resample_into(&mut self.bar_phys_canvas, phys.width, phys.height, alpha_f);
        draw_pango_texts(
            &mut self.bar_phys_canvas,
            layout.width,
            layout.height,
            alpha_f,
            &text_cmds,
        );

        // SAFETY: Blits rendered premultiplied-BGRA frame to `bar_win`.
        unsafe {
            blit_pixels(
                self.api,
                self.dpy,
                &self.vinfo,
                self.bar_win,
                self.bar_gc,
                phys.width,
                phys.height,
                &mut self.bar_phys_canvas.pixels,
            );
        }
        self.last_bar_alpha = bar_alpha;
        self.bar_dirty = false;
    }

    fn position_and_render_popup(&mut self) {
        let phys = logical_to_phys_rect(self.popup_rect, self.dpi_scale);
        // SAFETY: Resizes and raises language popup without stealing focus.
        unsafe {
            (self.api.move_resize_window)(
                self.dpy,
                self.popup_win,
                phys.x,
                phys.y,
                phys.width,
                phys.height,
            );
            (self.api.map_raised)(self.dpy, self.popup_win);
        }
        self.render_popup_frame();
    }

    fn render_popup_frame(&mut self) {
        let filtered = underbar::filter_languages(&self.search_query);
        let layout = self.popup_layout.clone();
        let theme = UnderbarTheme::for_theme(self.config.dark_theme);
        let active_code = self.config.language.trim().to_ascii_lowercase();
        let has_query = !self.search_query.is_empty();

        let end_row = (self.list_scroll + layout.visible_rows).min(filtered.len());
        let mut row_flags: Vec<Option<(u32, u32, Vec<u8>)>> =
            Vec::with_capacity(end_row.saturating_sub(self.list_scroll));
        for entry in filtered.iter().skip(self.list_scroll).take(layout.visible_rows) {
            row_flags.push(self.get_flag(entry.code));
        }
        let mut recent_flags: Vec<Option<(u32, u32, Vec<u8>)>> =
            Vec::with_capacity(layout.recent_strip.chips.len());
        for chip in &layout.recent_strip.chips {
            recent_flags.push(self.get_flag(&chip.code));
        }

        self.popup_logical_canvas.reset(layout.width, layout.height);

        // Outer rounded card + 1px neutral border.
        self.popup_logical_canvas.fill_rounded_rect(
            LocalRect::new(0, 0, layout.width, layout.height),
            8.0,
            theme.popup_bg,
            Some(theme.popup_border),
        );

        // Top search input bar.
        self.popup_logical_canvas.fill_rounded_rect(
            layout.search_rect,
            6.0,
            theme.search_bg,
            Some(if has_query {
                theme.accent_text
            } else {
                theme.search_border
            }),
        );
        self.popup_logical_canvas.draw_icon_search(
            (layout.search_rect.x + 11) as f32,
            (layout.search_rect.y as f32) + (layout.search_rect.height as f32) * 0.5,
            theme.text_muted,
        );

        let mut owned_texts: Vec<(String, LocalRect, u32, Rgba, TextAlign)> = Vec::new();
        let query_right = if has_query {
            layout.clear_rect.x - 2
        } else {
            layout.search_rect.right() - 6
        };
        let query_rect = LocalRect::new(
            layout.search_rect.x + 23,
            layout.search_rect.y,
            (query_right - (layout.search_rect.x + 23)).max(1) as u32,
            layout.search_rect.height,
        );
        let (search_str, search_color) = if has_query {
            (self.search_query.clone(), theme.text_primary)
        } else {
            ("Search languages…".to_owned(), theme.text_muted)
        };
        owned_texts.push((search_str, query_rect, 12, search_color, TextAlign::Left));

        if has_query {
            if self.hovered_popup_hit == Some(PopupHit::ClearSearch) {
                self.popup_logical_canvas.fill_rounded_rect(
                    layout.clear_rect,
                    6.0,
                    theme.hover_bg,
                    None,
                );
            }
            self.popup_logical_canvas.draw_icon_x(
                (layout.clear_rect.x as f32) + (layout.clear_rect.width as f32) * 0.5,
                (layout.clear_rect.y as f32) + (layout.clear_rect.height as f32) * 0.5,
                3.0,
                1.25,
                theme.text_muted,
            );
        }

        // Scrollable 99-language list rows.
        if filtered.is_empty() {
            owned_texts.push((
                "No matching languages".to_owned(),
                layout.list_rect,
                12,
                theme.text_muted,
                TextAlign::Center,
            ));
        } else {
            let needs_scrollbar = filtered.len() > layout.visible_rows;
            let row_w = if needs_scrollbar {
                layout.list_rect.width.saturating_sub(7)
            } else {
                layout.list_rect.width
            };

            for (vis_idx, entry) in filtered
                .iter()
                .skip(self.list_scroll)
                .take(layout.visible_rows)
                .enumerate()
            {
                let abs_idx = self.list_scroll + vis_idx;
                let row_y = layout.list_rect.y + (vis_idx as i32) * (LANG_ROW_HEIGHT as i32);
                let row_rect = LocalRect::new(layout.list_rect.x, row_y, row_w, LANG_ROW_HEIGHT);
                let is_active = entry.code.eq_ignore_ascii_case(&active_code);
                let is_hovered = self.hovered_popup_hit == Some(PopupHit::LanguageRow(abs_idx))
                    || self.keyboard_selected_row == Some(abs_idx);

                if is_hovered {
                    self.popup_logical_canvas.fill_rounded_rect(
                        row_rect,
                        6.0,
                        theme.row_hover_bg,
                        None,
                    );
                }

                let name_x =
                    if let Some((fw, fh, rgba)) = row_flags.get(vis_idx).and_then(|o| o.as_ref()) {
                        let flag_rect = LocalRect::new(row_rect.x + 6, row_rect.y + 8, 18, 12);
                        self.popup_logical_canvas.blit_flag_rgba(
                            flag_rect,
                            *fw,
                            *fh,
                            rgba,
                            theme.flag_rim,
                        );
                        flag_rect.right() + 7
                    } else {
                        let badge_rect = LocalRect::new(row_rect.x + 6, row_rect.y + 6, 22, 16);
                        self.popup_logical_canvas.fill_rounded_rect(
                            badge_rect,
                            4.0,
                            theme.chip_bg,
                            Some(theme.bar_border),
                        );
                        owned_texts.push((
                            underbar::format_language_code(entry.code),
                            badge_rect,
                            10,
                            theme.text_muted,
                            TextAlign::Center,
                        ));
                        badge_rect.right() + 6
                    };

                let check_space = if is_active { 20 } else { 6 };
                let name_w = (row_rect.right() - check_space - name_x).max(1) as u32;
                let name_rect = LocalRect::new(name_x, row_rect.y, name_w, row_rect.height);
                owned_texts.push((
                    entry.name.to_owned(),
                    name_rect,
                    12,
                    if is_active {
                        theme.accent_text
                    } else {
                        theme.text_primary
                    },
                    TextAlign::Left,
                ));

                if is_active {
                    self.popup_logical_canvas.draw_icon_check(
                        (row_rect.right() - 10) as f32,
                        (row_rect.y as f32) + (row_rect.height as f32) * 0.5,
                        0.95,
                        1.45,
                        theme.accent_text,
                    );
                }
            }

            if needs_scrollbar {
                let track_h = layout.list_rect.height as f32;
                let ratio = (layout.visible_rows as f32) / (filtered.len() as f32);
                let thumb_h = (track_h * ratio).max(20.0).min(track_h);
                let max_scroll = layout.max_list_scroll().max(1) as f32;
                let scroll_frac = (self.list_scroll as f32) / max_scroll;
                let thumb_y =
                    layout.list_rect.y + ((track_h - thumb_h) * scroll_frac).round() as i32;
                let thumb_rect = LocalRect::new(
                    layout.list_rect.right() - 4,
                    thumb_y,
                    3,
                    thumb_h.round() as u32,
                );
                self.popup_logical_canvas.fill_rounded_rect(
                    thumb_rect,
                    1.5,
                    theme.scrollbar_thumb,
                    None,
                );
            }
        }

        // Divider above recent strip (if present).
        if layout.recent_strip.height > 0 {
            self.popup_logical_canvas.fill_rect(
                LocalRect::new(
                    POPUP_PADDING_X,
                    layout.recent_strip_y,
                    layout.recent_strip.row_width,
                    1,
                ),
                theme.divider,
            );
        }

        let phys = logical_to_phys_rect(self.popup_rect, self.dpi_scale);
        self.popup_logical_canvas.resample_into(
            &mut self.popup_phys_canvas,
            phys.width,
            phys.height,
            1.0,
        );

        let text_cmds: Vec<TextDrawCmd<'_>> = owned_texts
            .iter()
            .map(|(s, r, px, col, align)| TextDrawCmd {
                text: s.as_str(),
                rect: *r,
                font_px: *px,
                color: *col,
                align: *align,
                wrapped: false,
            })
            .collect();
        draw_pango_texts(
            &mut self.popup_phys_canvas,
            layout.width,
            layout.height,
            1.0,
            &text_cmds,
        );

        // Render bottom recent-languages chip strip at physical resolution so both chips and crisp
        // Pango chip labels receive smooth peeking edge fades.
        if layout.recent_strip.height > 0 {
            let strip = &layout.recent_strip;
            let chip_row_h = underbar::RECENT_CHIP_HEIGHT;
            let chip_row_y = layout.recent_strip_y + 6;
            self.strip_logical_canvas.reset(strip.row_width, chip_row_h);

            let mut strip_texts: Vec<(String, LocalRect, Rgba, TextAlign)> = Vec::new();
            for (idx, chip) in strip.chips.iter().enumerate() {
                if chip.x + chip.width as i32 <= 0 || chip.x >= strip.row_width as i32 {
                    continue;
                }
                let chip_rect = LocalRect::new(chip.x, 0, chip.width, chip.height);
                let is_active = chip.code.eq_ignore_ascii_case(&active_code);
                let is_hovered = self.hovered_popup_hit == Some(PopupHit::RecentChip(idx));
                let bg = if is_hovered {
                    theme.chip_hover_bg
                } else {
                    theme.chip_bg
                };
                let border = if is_active {
                    Some(theme.accent_text)
                } else {
                    Some(theme.bar_border)
                };
                self.strip_logical_canvas
                    .fill_rounded_rect(chip_rect, 6.0, bg, border);

                let code_upper = underbar::format_language_code(&chip.code);
                let fg = if is_active {
                    theme.accent_text
                } else {
                    theme.text_primary
                };
                if let Some((fw, fh, rgba)) = recent_flags.get(idx).and_then(|o| o.as_ref()) {
                    let content_w = 14 + 4 + 20;
                    let start_x = chip_rect.x + ((chip_rect.width as i32 - content_w) / 2).max(4);
                    let flag_rect = LocalRect::new(start_x, 6, 14, 10);
                    self.strip_logical_canvas
                        .blit_flag_rgba(flag_rect, *fw, *fh, rgba, theme.flag_rim);
                    let label_rect = LocalRect::new(
                        flag_rect.right() + 4,
                        0,
                        (chip_rect.right() - (flag_rect.right() + 4) - 2).max(1) as u32,
                        chip_rect.height,
                    );
                    strip_texts.push((code_upper, label_rect, fg, TextAlign::Left));
                } else {
                    strip_texts.push((code_upper, chip_rect, fg, TextAlign::Center));
                }
            }

            let sx = f64::from(phys.width) / f64::from(layout.width.max(1));
            let sy = f64::from(phys.height) / f64::from(layout.height.max(1));
            let strip_pw = (f64::from(strip.row_width) * sx).round().max(1.0) as u32;
            let strip_ph = (f64::from(chip_row_h) * sy).round().max(1.0) as u32;
            self.strip_logical_canvas
                .resample_into(&mut self.strip_phys_canvas, strip_pw, strip_ph, 1.0);
            let strip_cmds: Vec<TextDrawCmd<'_>> = strip_texts
                .iter()
                .map(|(s, r, col, align)| TextDrawCmd {
                    text: s.as_str(),
                    rect: *r,
                    font_px: 10,
                    color: *col,
                    align: *align,
                    wrapped: false,
                })
                .collect();
            draw_pango_texts(
                &mut self.strip_phys_canvas,
                strip.row_width,
                chip_row_h,
                1.0,
                &strip_cmds,
            );

            let dst_x = (f64::from(POPUP_PADDING_X) * sx).round() as i32;
            let dst_y = (f64::from(chip_row_y) * sy).round() as i32;
            let fade_l = if strip.show_left_fade {
                (f64::from(RECENT_FADE_WIDTH) * sx).round() as u32
            } else {
                0
            };
            let fade_r = if strip.show_right_fade {
                (f64::from(RECENT_FADE_WIDTH) * sx).round() as u32
            } else {
                0
            };
            self.popup_phys_canvas.composite_with_edge_fade(
                &self.strip_phys_canvas,
                dst_x,
                dst_y,
                fade_l,
                fade_r,
            );
        }

        // SAFETY: Blits rendered premultiplied-BGRA frame to `popup_win`.
        unsafe {
            blit_pixels(
                self.api,
                self.dpy,
                &self.vinfo,
                self.popup_win,
                self.popup_gc,
                phys.width,
                phys.height,
                &mut self.popup_phys_canvas.pixels,
            );
        }
        self.popup_dirty = false;
    }

    fn render_recovery_frame(&mut self) {
        let layout = self.recovery_layout;
        let theme = UnderbarTheme::for_theme(self.config.dark_theme);
        let dark = self.config.dark_theme;

        let card_bg = if dark {
            Rgba::new(0x1B, 0x1A, 0x16, 240)
        } else {
            Rgba::new(0xFB, 0xF9, 0xF3, 244)
        };
        let card_border = if dark {
            Rgba::new(0x3D, 0x3A, 0x30, 236)
        } else {
            Rgba::new(0xD8, 0xD2, 0xC0, 240)
        };
        let dot_yellow = if dark {
            Rgba::rgb(0xD2, 0x99, 0x22)
        } else {
            Rgba::rgb(0x9A, 0x67, 0x00)
        };

        self.recovery_logical_canvas
            .reset(layout.width, layout.height);
        self.recovery_logical_canvas.fill_rounded_rect(
            LocalRect::new(0, 0, layout.width, layout.height),
            RECOVERY_CARD_RADIUS,
            card_bg,
            Some(card_border),
        );

        // Status dot (7x7 circle)
        self.recovery_logical_canvas.fill_rounded_rect(
            LocalRect::new(layout.dot_cx - 3, layout.dot_cy - 3, 7, 7),
            3.5,
            dot_yellow,
            None,
        );

        if layout.has_paste_target {
            // 1. Paste to <App> button (Eqho blue accent)
            let paste_bg = if self.hovered_recovery_btn == Some(RecoveryCardButton::PastePrevious) {
                theme.done_hover_bg
            } else {
                theme.done_bg
            };
            self.recovery_logical_canvas.fill_rounded_rect(
                layout.paste_previous,
                6.0,
                paste_bg,
                None,
            );

            // 2. Copy button (outline)
            let copy_bg = if self.hovered_recovery_btn == Some(RecoveryCardButton::Copy) {
                theme.chip_hover_bg
            } else {
                theme.chip_bg
            };
            self.recovery_logical_canvas.fill_rounded_rect(
                layout.copy,
                6.0,
                copy_bg,
                Some(theme.bar_border),
            );
        } else {
            // 1. Show text button
            let show_bg = if self.hovered_recovery_btn == Some(RecoveryCardButton::ShowText) {
                theme.chip_hover_bg
            } else {
                theme.chip_bg
            };
            self.recovery_logical_canvas.fill_rounded_rect(
                layout.show_text,
                6.0,
                show_bg,
                Some(theme.bar_border),
            );

            // 2. Copy button (Eqho blue accent)
            let copy_bg = if self.hovered_recovery_btn == Some(RecoveryCardButton::Copy) {
                theme.done_hover_bg
            } else {
                theme.done_bg
            };
            self.recovery_logical_canvas
                .fill_rounded_rect(layout.copy, 6.0, copy_bg, None);
        }

        // 3. Discard button
        let disc_bg = if self.hovered_recovery_btn == Some(RecoveryCardButton::Discard) {
            theme.chip_hover_bg
        } else {
            theme.chip_bg
        };
        self.recovery_logical_canvas.fill_rounded_rect(
            layout.discard,
            6.0,
            disc_bg,
            Some(theme.bar_border),
        );

        let phys = logical_to_phys_rect(self.recovery_rect, self.dpi_scale);
        self.recovery_logical_canvas.resample_into(
            &mut self.recovery_phys_canvas,
            phys.width,
            phys.height,
            1.0,
        );
        let paste_label = underbar::format_paste_previous_button_label(
            self.recovery_paste_target.as_deref().unwrap_or(""),
        );
        let cmds = if layout.has_paste_target {
            [
                TextDrawCmd {
                    text: "Not pasted — no text box focused",
                    rect: layout.title_rect,
                    font_px: 11,
                    color: theme.text_primary,
                    align: TextAlign::Left,
                    wrapped: false,
                },
                TextDrawCmd {
                    text: &paste_label,
                    rect: layout.paste_previous,
                    font_px: 11,
                    color: theme.done_fg,
                    align: TextAlign::Center,
                    wrapped: false,
                },
                TextDrawCmd {
                    text: "Copy",
                    rect: layout.copy,
                    font_px: 11,
                    color: theme.text_primary,
                    align: TextAlign::Center,
                    wrapped: false,
                },
                TextDrawCmd {
                    text: "Discard",
                    rect: layout.discard,
                    font_px: 11,
                    color: theme.text_muted,
                    align: TextAlign::Center,
                    wrapped: false,
                },
            ]
        } else {
            [
                TextDrawCmd {
                    text: "Couldn't paste automatically",
                    rect: layout.title_rect,
                    font_px: 11,
                    color: theme.text_primary,
                    align: TextAlign::Left,
                    wrapped: false,
                },
                TextDrawCmd {
                    text: "Show text",
                    rect: layout.show_text,
                    font_px: 11,
                    color: theme.text_primary,
                    align: TextAlign::Center,
                    wrapped: false,
                },
                TextDrawCmd {
                    text: "Copy",
                    rect: layout.copy,
                    font_px: 11,
                    color: theme.done_fg,
                    align: TextAlign::Center,
                    wrapped: false,
                },
                TextDrawCmd {
                    text: "Discard",
                    rect: layout.discard,
                    font_px: 11,
                    color: theme.text_muted,
                    align: TextAlign::Center,
                    wrapped: false,
                },
            ]
        };
        draw_pango_texts(
            &mut self.recovery_phys_canvas,
            layout.width,
            layout.height,
            1.0,
            &cmds,
        );

        // SAFETY: Blits rendered premultiplied-BGRA frame to `recovery_win`.
        unsafe {
            blit_pixels(
                self.api,
                self.dpy,
                &self.vinfo,
                self.recovery_win,
                self.recovery_gc,
                phys.width,
                phys.height,
                &mut self.recovery_phys_canvas.pixels,
            );
        }
    }

    fn render_pill_frame(&mut self, dt_s: f32) {
        let frame = self.animator.step(dt_s, self.config.show_level);
        let _ = self.animator.step_fade(dt_s);

        let phys = logical_to_phys_rect(self.pill_rect, self.dpi_scale);
        let light = if self.config.dark_theme { 0.0_f32 } else { 1.0_f32 };
        let opacity = f32::from(self.animator.effective_pill_alpha(&self.config)) / 255.0;
        let mut pixels = render_premul_bgra(
            phys.width,
            phys.height,
            frame.phase,
            frame.level,
            light,
            opacity,
            frame.wave,
        );

        // SAFETY: Blits rendered pill pixels to `pill_win`.
        unsafe {
            blit_pixels(
                self.api,
                self.dpy,
                &self.vinfo,
                self.pill_win,
                self.pill_gc,
                phys.width,
                phys.height,
                &mut pixels,
            );
        }

        if self.panel_shown {
            let fade_alpha = (self.animator.alpha_logical.clamp(0.0, 1.0) * 255.0).round() as u8;
            if fade_alpha != self.last_panel_alpha {
                self.render_panel_frame();
            }
        }
    }

    fn open_lang_popup(&mut self) {
        if !self.visible || !self.config.controls_enabled || self.visibility.finishing {
            return;
        }
        self.search_query.clear();
        self.list_scroll = 0;
        self.recent_scroll = 0;
        self.hovered_popup_hit = None;
        self.keyboard_selected_row = None;
        self.visibility.set_lang_menu_open(true);
        self.sync_bar_visibility();

        self.recompute_popup_layout();
        self.popup_shown = true;
        self.prev_mouse_down = self.query_pointer_logical().2;
        self.position_and_render_popup();
        self.bar_dirty = true;
        self.render_bar_frame();
    }

    fn toggle_lang_popup(&mut self) {
        if self.popup_shown {
            self.close_lang_popup();
            self.render_bar_frame();
        } else {
            self.open_lang_popup();
        }
    }

    fn select_language_and_close(&mut self, code: String) {
        self.config.language = code.clone();
        self.close_lang_popup();
        self.recompute_bar_geometry();
        if self.bar_shown {
            self.render_bar_frame();
        }
        self.emit_action(OverlayAction::SelectLanguage(code));
    }

    fn on_bar_click(&mut self, lx: i32, ly: i32) {
        if self.visibility.finishing {
            return;
        }
        let Some(ctrl) = self.bar_layout.hit_test(lx, ly) else {
            return;
        };
        match ctrl {
            UnderbarControl::Cancel => {
                self.close_lang_popup();
                self.emit_action(OverlayAction::Cancel);
            }
            UnderbarControl::Done => {
                self.close_lang_popup();
                self.emit_action(OverlayAction::Done);
            }
            UnderbarControl::Pin => {
                let next_pinned = !self.config.controls_pinned;
                self.config.controls_pinned = next_pinned;
                self.visibility.set_pinned(next_pinned);
                self.render_bar_frame();
                self.emit_action(OverlayAction::TogglePin(next_pinned));
            }
            UnderbarControl::DeliveryMode => {
                let next_mode = underbar::next_delivery_mode(&self.config.delivery_mode).to_owned();
                self.config.delivery_mode = next_mode.clone();
                self.render_bar_frame();
                self.emit_action(OverlayAction::ToggleDeliveryMode(next_mode));
            }
            UnderbarControl::Language => {
                self.toggle_lang_popup();
            }
        }
    }

    fn on_popup_click(&mut self, lx: i32, ly: i32) {
        let Some(hit) = self
            .popup_layout
            .hit_test(lx, ly, !self.search_query.is_empty(), self.list_scroll)
        else {
            return;
        };
        match hit {
            PopupHit::SearchInput => {}
            PopupHit::ClearSearch => {
                self.search_query.clear();
                self.list_scroll = 0;
                self.keyboard_selected_row = None;
                self.recompute_popup_layout();
                self.position_and_render_popup();
            }
            PopupHit::LanguageRow(idx) => {
                let filtered = underbar::filter_languages(&self.search_query);
                if let Some(entry) = filtered.get(idx) {
                    self.select_language_and_close(entry.code.to_owned());
                }
            }
            PopupHit::RecentChip(idx) => {
                if let Some(chip) = self.popup_layout.recent_strip.chips.get(idx) {
                    let code = chip.code.clone();
                    self.select_language_and_close(code);
                }
            }
        }
    }

    fn on_popup_wheel(&mut self, lx: i32, ly: i32, delta: i16, horizontal: bool) {
        let notches = underbar::wheel_delta_to_notches(delta);
        if notches == 0 {
            return;
        }
        let in_recent = self
            .popup_layout
            .recent_strip_rect()
            .is_some_and(|r| r.contains(lx, ly));

        if in_recent || horizontal {
            if self.popup_layout.recent_strip.scrollable {
                let step = if horizontal { -notches } else { notches };
                let next = underbar::step_recent_scroll(
                    self.recent_scroll,
                    step,
                    self.popup_layout.recent_strip.max_scroll,
                );
                if next != self.recent_scroll {
                    self.recent_scroll = next;
                    self.recompute_popup_layout();
                    self.render_popup_frame();
                }
            }
            return;
        }

        let max_scroll = self.popup_layout.max_list_scroll();
        if max_scroll > 0 {
            let step = notches * 3;
            let next = (self.list_scroll as isize + step as isize).clamp(0, max_scroll as isize)
                as usize;
            if next != self.list_scroll {
                self.list_scroll = next;
                self.render_popup_frame();
            }
        }
    }

    fn on_popup_key(&mut self, mut key_ev: XKeyEvent) {
        if !self.popup_shown {
            return;
        }
        let mut buf = [0 as c_char; 16];
        let mut keysym: c_ulong = 0;
        // SAFETY: Valid `XKeyEvent` received from `XNextEvent`.
        let n = unsafe {
            (self.api.lookup_string)(
                &mut key_ev,
                buf.as_mut_ptr(),
                buf.len() as c_int,
                &mut keysym,
                null_mut(),
            )
        };
        if keysym == XK_ESCAPE {
            self.close_lang_popup();
            self.render_bar_frame();
            return;
        }
        if keysym == XK_BACKSPACE {
            if self.search_query.pop().is_some() {
                self.list_scroll = 0;
                self.keyboard_selected_row = None;
                self.recompute_popup_layout();
                self.position_and_render_popup();
            }
            return;
        }
        if keysym == XK_RETURN {
            let filtered = underbar::filter_languages(&self.search_query);
            let idx = self.keyboard_selected_row.unwrap_or(0);
            if let Some(entry) = filtered.get(idx) {
                self.select_language_and_close(entry.code.to_owned());
            }
            return;
        }
        if keysym == XK_DOWN || keysym == XK_UP {
            let filtered = underbar::filter_languages(&self.search_query);
            if filtered.is_empty() {
                return;
            }
            let max_idx = filtered.len() - 1;
            let next = match self.keyboard_selected_row {
                None => 0,
                Some(cur) if keysym == XK_DOWN => (cur + 1).min(max_idx),
                Some(cur) => cur.saturating_sub(1),
            };
            self.keyboard_selected_row = Some(next);
            if next < self.list_scroll {
                self.list_scroll = next;
            } else if next >= self.list_scroll + self.popup_layout.visible_rows {
                self.list_scroll = next + 1 - self.popup_layout.visible_rows;
            }
            self.render_popup_frame();
            return;
        }
        if n > 0 {
            let slice: Vec<u8> = buf[..(n as usize).min(buf.len())]
                .iter()
                .map(|&c| c as u8)
                .collect();
            if let Ok(s) = std::str::from_utf8(&slice) {
                let mut changed = false;
                for ch in s.chars() {
                    if !ch.is_control() && self.search_query.chars().count() < 32 {
                        self.search_query.push(ch);
                        changed = true;
                    }
                }
                if changed {
                    self.list_scroll = 0;
                    self.keyboard_selected_row = Some(0);
                    self.recompute_popup_layout();
                    self.position_and_render_popup();
                }
            }
        }
    }

    fn on_recovery_click(&mut self, lx: i32, ly: i32) {
        let Some(btn) = self.recovery_layout.hit_test(lx, ly) else {
            return;
        };
        self.hide_recovery_now();
        match btn {
            RecoveryCardButton::PastePrevious => {
                self.emit_action(OverlayAction::RecoveryPastePrevious);
            }
            RecoveryCardButton::ShowText => self.emit_action(OverlayAction::RecoveryShowText),
            RecoveryCardButton::Copy => self.emit_action(OverlayAction::RecoveryCopy),
            RecoveryCardButton::Discard => self.emit_action(OverlayAction::RecoveryDiscard),
        }
    }

    fn query_pointer_logical(&self) -> (i32, i32, bool) {
        let mut root_ret: c_ulong = 0;
        let mut child_ret: c_ulong = 0;
        let mut rx: c_int = 0;
        let mut ry: c_int = 0;
        let mut wx: c_int = 0;
        let mut wy: c_int = 0;
        let mut mask: c_uint = 0;
        // SAFETY: Queries X11 pointer state on root window.
        let ok = unsafe {
            (self.api.query_pointer)(
                self.dpy,
                self.root,
                &mut root_ret,
                &mut child_ret,
                &mut rx,
                &mut ry,
                &mut wx,
                &mut wy,
                &mut mask,
            ) != 0
        };
        if !ok {
            return (i32::MIN, i32::MIN, false);
        }
        let lx = phys_to_logical_coord(rx, self.dpi_scale);
        let ly = phys_to_logical_coord(ry, self.dpi_scale);
        let down = (mask & (BUTTON1_MASK | BUTTON3_MASK)) != 0;
        (lx, ly, down)
    }

    fn drain_x11_events(&mut self) {
        // SAFETY: Pumps all queued X11 input events for our overlay windows.
        unsafe {
            let mut ev: XEvent = std::mem::zeroed();
            while (self.api.pending)(self.dpy) > 0 {
                (self.api.next_event)(self.dpy, &mut ev);
                let etype = ev.r#type;
                let win = ev.xany.window;

                if win == self.pill_win {
                    match etype {
                        ENTER_NOTIFY | MOTION_NOTIFY => self.pill_entered = true,
                        LEAVE_NOTIFY => self.pill_entered = false,
                        _ => {}
                    }
                } else if win == self.bar_win {
                    match etype {
                        ENTER_NOTIFY => self.bar_entered = true,
                        LEAVE_NOTIFY => {
                            self.bar_entered = false;
                            if self.hovered_bar_control.is_some() {
                                self.hovered_bar_control = None;
                                if self.bar_shown {
                                    self.render_bar_frame();
                                }
                            }
                        }
                        MOTION_NOTIFY => {
                            self.bar_entered = true;
                            let lx = phys_to_logical_coord(ev.xmotion.x, self.dpi_scale);
                            let ly = phys_to_logical_coord(ev.xmotion.y, self.dpi_scale);
                            let next = self.bar_layout.hit_test(lx, ly);
                            if next != self.hovered_bar_control {
                                self.hovered_bar_control = next;
                                if self.bar_shown {
                                    self.render_bar_frame();
                                }
                            }
                        }
                        BUTTON_PRESS if ev.xbutton.button == 1 => {
                            let lx = phys_to_logical_coord(ev.xbutton.x, self.dpi_scale);
                            let ly = phys_to_logical_coord(ev.xbutton.y, self.dpi_scale);
                            self.on_bar_click(lx, ly);
                        }
                        _ => {}
                    }
                } else if win == self.popup_win {
                    match etype {
                        ENTER_NOTIFY => self.popup_entered = true,
                        LEAVE_NOTIFY => {
                            self.popup_entered = false;
                            if self.hovered_popup_hit.is_some() {
                                self.hovered_popup_hit = None;
                                if self.popup_shown {
                                    self.render_popup_frame();
                                }
                            }
                        }
                        MOTION_NOTIFY => {
                            self.popup_entered = true;
                            let lx = phys_to_logical_coord(ev.xmotion.x, self.dpi_scale);
                            let ly = phys_to_logical_coord(ev.xmotion.y, self.dpi_scale);
                            let next = self.popup_layout.hit_test(
                                lx,
                                ly,
                                !self.search_query.is_empty(),
                                self.list_scroll,
                            );
                            if next != self.hovered_popup_hit {
                                self.hovered_popup_hit = next;
                                self.keyboard_selected_row = None;
                                if self.popup_shown {
                                    self.render_popup_frame();
                                }
                            }
                        }
                        BUTTON_PRESS => {
                            let lx = phys_to_logical_coord(ev.xbutton.x, self.dpi_scale);
                            let ly = phys_to_logical_coord(ev.xbutton.y, self.dpi_scale);
                            match ev.xbutton.button {
                                1 => self.on_popup_click(lx, ly),
                                4 => self.on_popup_wheel(lx, ly, 120, false),
                                5 => self.on_popup_wheel(lx, ly, -120, false),
                                6 => self.on_popup_wheel(lx, ly, -120, true),
                                7 => self.on_popup_wheel(lx, ly, 120, true),
                                _ => {}
                            }
                        }
                        KEY_PRESS => self.on_popup_key(ev.xkey),
                        _ => {}
                    }
                } else if win == self.recovery_win {
                    match etype {
                        LEAVE_NOTIFY if self.hovered_recovery_btn.is_some() => {
                            self.hovered_recovery_btn = None;
                            if self.recovery_shown {
                                self.render_recovery_frame();
                            }
                        }
                        MOTION_NOTIFY => {
                            let lx = phys_to_logical_coord(ev.xmotion.x, self.dpi_scale);
                            let ly = phys_to_logical_coord(ev.xmotion.y, self.dpi_scale);
                            let next = self.recovery_layout.hit_test(lx, ly);
                            if next != self.hovered_recovery_btn {
                                self.hovered_recovery_btn = next;
                                if self.recovery_shown {
                                    self.render_recovery_frame();
                                }
                            }
                        }
                        BUTTON_PRESS if ev.xbutton.button == 1 => {
                            let lx = phys_to_logical_coord(ev.xbutton.x, self.dpi_scale);
                            let ly = phys_to_logical_coord(ev.xbutton.y, self.dpi_scale);
                            self.on_recovery_click(lx, ly);
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    fn on_tick(&mut self) {
        let now = Instant::now();
        let dt_dur = now.saturating_duration_since(self.last_tick);
        let dt = dt_dur.as_secs_f32().clamp(0.001, 0.1);
        let dt_ms = dt_dur.as_millis() as u64;
        self.last_tick = now;

        if self.pill_shown {
            self.render_pill_frame(dt);
        }

        if self.visible && self.config.controls_enabled {
            let (px, py, mouse_down) = self.query_pointer_logical();

            if self.popup_shown {
                if mouse_down && !self.prev_mouse_down && px != i32::MIN {
                    let in_bar =
                        underbar::point_in_overlay_rect(px, py, self.bar_placement.bar_rect);
                    let in_popup = underbar::point_in_overlay_rect(px, py, self.popup_rect);
                    if !in_bar && !in_popup {
                        self.close_lang_popup();
                    }
                }
                self.prev_mouse_down = mouse_down;
            }

            let in_zone = self.pill_entered
                || self.bar_entered
                || self.popup_entered
                || (px != i32::MIN
                    && underbar::is_cursor_in_hover_zone(
                        px,
                        py,
                        self.pill_rect,
                        self.bar_placement.bridge_rect,
                        self.bar_shown.then_some(self.bar_placement.bar_rect),
                        self.popup_shown.then_some(self.popup_rect),
                    ));

            self.visibility.tick(dt_ms, in_zone);
            self.sync_bar_visibility();

            if self.bar_shown {
                let interacting =
                    self.bar_entered || self.hovered_bar_control.is_some() || self.popup_shown;
                let target_blend = if interacting { 1.0_f32 } else { 0.0_f32 };
                let step = (dt * 7.5).clamp(0.0, 1.0);
                let diff = target_blend - self.bar_hover_blend;
                if diff.abs() <= step {
                    self.bar_hover_blend = target_blend;
                } else {
                    self.bar_hover_blend += diff.signum() * step;
                }

                let elapsed = now.saturating_duration_since(self.session_start).as_secs();
                let bar_alpha = self.effective_bar_alpha();
                if elapsed != self.last_elapsed_secs
                    || bar_alpha != self.last_bar_alpha
                    || self.bar_dirty
                {
                    self.last_elapsed_secs = elapsed;
                    self.render_bar_frame();
                }
            }
            if self.popup_shown && self.popup_dirty {
                self.render_popup_frame();
            }
        }

        if !self.visible && self.pill_shown && self.animator.alpha_logical <= 0.001 {
            self.hide_bar_now();
            self.hide_panel_now();
            // SAFETY: Unmaps pill window after fade-out completes.
            unsafe {
                (self.api.unmap_window)(self.dpy, self.pill_win);
                (self.api.flush)(self.dpy);
            }
            self.pill_shown = false;
        }
    }

    fn handle_cmd(&mut self, cmd: LinuxOverlayCmd) -> bool {
        match cmd {
            LinuxOverlayCmd::Show(text) => self.cmd_show(text),
            LinuxOverlayCmd::UpdateText(text) => self.cmd_update_text(text),
            LinuxOverlayCmd::SetLevel(lvl) => self.animator.set_level(lvl),
            LinuxOverlayCmd::SetWaveform(l, t) => self.animator.feed_voice(l, t),
            LinuxOverlayCmd::SetFinishing(f) => self.cmd_set_finishing(f),
            LinuxOverlayCmd::UpdateSessionMeta {
                language,
                delivery_mode,
                pinned,
                recent_languages,
            } => self.cmd_update_session_meta(language, delivery_mode, pinned, recent_languages),
            LinuxOverlayCmd::UpdateConfig(cfg) => self.cmd_update_config(cfg),
            LinuxOverlayCmd::ShowRecovery { text, paste_target } => {
                self.cmd_show_recovery(text, paste_target);
            }
            LinuxOverlayCmd::HideRecovery => self.hide_recovery_now(),
            LinuxOverlayCmd::Hide => self.cmd_hide(),
            LinuxOverlayCmd::Shutdown => return true,
        }
        false
    }

    fn destroy(&mut self) {
        self.hide_recovery_now();
        self.hide_panel_now();
        self.hide_bar_now();
        // SAFETY: Tears down all X11 resources owned exclusively by this thread.
        unsafe {
            if self.pill_shown {
                (self.api.unmap_window)(self.dpy, self.pill_win);
            }
            for (win, gc) in [
                (self.recovery_win, self.recovery_gc),
                (self.popup_win, self.popup_gc),
                (self.bar_win, self.bar_gc),
                (self.panel_win, self.panel_gc),
                (self.pill_win, self.pill_gc),
            ] {
                (self.api.free_gc)(self.dpy, gc);
                (self.api.destroy_window)(self.dpy, win);
            }
            (self.api.free_colormap)(self.dpy, self.colormap);
            (self.api.close_display)(self.dpy);
        }
    }
}

fn run_linux_overlay_thread(
    config: OverlayConfig,
    on_action: Option<OverlayActionCallback>,
    flag_provider: Option<FlagBitmapProvider>,
    rx: mpsc::Receiver<LinuxOverlayCmd>,
) {
    let Some(api) = X11OverlayApi::load() else {
        return;
    };
    // SAFETY: Dedicated X11 connection owned exclusively by this overlay thread.
    let mut state = unsafe {
        let dpy = (api.open_display)(null());
        if dpy.is_null() {
            return;
        }
        let screen = (api.default_screen)(dpy);
        let root = (api.root_window)(dpy, screen);

        let mut vinfo: XVisualInfo = std::mem::zeroed();
        let has_argb = (api.match_visual_info)(dpy, screen, 32, TRUE_COLOR, &mut vinfo) != 0;
        if !has_argb && (api.match_visual_info)(dpy, screen, 24, TRUE_COLOR, &mut vinfo) == 0 {
            (api.close_display)(dpy);
            return;
        }

        let colormap = (api.create_colormap)(dpy, root, vinfo.visual, ALLOC_NONE);
        let pill_mask = if config.controls_enabled {
            ENTER_WINDOW_MASK | LEAVE_WINDOW_MASK | POINTER_MOTION_MASK
        } else {
            0
        };
        let interactive_mask =
            BUTTON_PRESS_MASK | POINTER_MOTION_MASK | ENTER_WINDOW_MASK | LEAVE_WINDOW_MASK;

        let (pill_win, pill_gc) = create_overlay_win(
            api,
            dpy,
            root,
            &vinfo,
            colormap,
            !config.controls_enabled,
            pill_mask,
        );
        let (panel_win, panel_gc) =
            create_overlay_win(api, dpy, root, &vinfo, colormap, true, 0);
        let (bar_win, bar_gc) =
            create_overlay_win(api, dpy, root, &vinfo, colormap, false, interactive_mask);
        let (popup_win, popup_gc) = create_overlay_win(
            api,
            dpy,
            root,
            &vinfo,
            colormap,
            false,
            interactive_mask | KEY_PRESS_MASK,
        );
        let (recovery_win, recovery_gc) =
            create_overlay_win(api, dpy, root, &vinfo, colormap, false, interactive_mask);
        (api.flush)(dpy);

        let visibility = UnderbarVisibility::new(config.controls_enabled, config.controls_pinned);
        let bar_layout = UnderbarLayout::compute(false);
        let recovery_layout = RecoveryCardLayout::compute();
        let popup_layout = LangPopupLayout::compute(0, &[], 0);
        let zero_rect = OverlayRect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        };
        let zero_area = WorkArea {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        let bar_placement = underbar::calc_underbar_placement(zero_rect, bar_layout.width, zero_area);

        LinuxOverlayState {
            api,
            dpy,
            screen,
            root,
            vinfo,
            colormap,
            pill_win,
            pill_gc,
            panel_win,
            panel_gc,
            bar_win,
            bar_gc,
            popup_win,
            popup_gc,
            recovery_win,
            recovery_gc,
            config,
            on_action,
            flag_provider,
            flag_cache: HashMap::new(),
            dpi_scale: 1.0,
            active_work_area: zero_area,
            pill_rect: zero_rect,
            pill_shown: false,
            visible: false,
            animator: PillAnimator::new(),
            last_tick: Instant::now(),
            session_start: Instant::now(),
            last_elapsed_secs: 0,
            pill_entered: false,
            bar_entered: false,
            popup_entered: false,
            panel_shown: false,
            panel_text: String::new(),
            panel_rect: zero_rect,
            panel_logical_canvas: PremulCanvas::new(1, 1),
            panel_phys_canvas: PremulCanvas::new(1, 1),
            last_panel_alpha: 0,
            visibility,
            bar_shown: false,
            bar_layout,
            bar_placement,
            bar_logical_canvas: PremulCanvas::new(1, 1),
            bar_phys_canvas: PremulCanvas::new(1, 1),
            hovered_bar_control: None,
            bar_hover_blend: 0.0,
            last_bar_alpha: 0,
            bar_dirty: false,
            popup_shown: false,
            popup_rect: zero_rect,
            popup_layout,
            popup_logical_canvas: PremulCanvas::new(1, 1),
            popup_phys_canvas: PremulCanvas::new(1, 1),
            strip_logical_canvas: PremulCanvas::new(1, 1),
            strip_phys_canvas: PremulCanvas::new(1, 1),
            search_query: String::new(),
            list_scroll: 0,
            recent_scroll: 0,
            hovered_popup_hit: None,
            keyboard_selected_row: None,
            popup_dirty: false,
            prev_mouse_down: false,
            recovery_shown: false,
            recovery_rect: zero_rect,
            recovery_layout,
            recovery_paste_target: None,
            recovery_logical_canvas: PremulCanvas::new(1, 1),
            recovery_phys_canvas: PremulCanvas::new(1, 1),
            hovered_recovery_btn: None,
        }
    };

    state.update_pill_geometry();

    loop {
        let active =
            state.pill_shown || state.recovery_shown || state.animator.alpha_logical > 0.001;
        let timeout = if state.pill_shown || state.animator.alpha_logical > 0.001 {
            Duration::from_millis(FRAME_MS)
        } else if state.recovery_shown {
            Duration::from_millis(16)
        } else {
            Duration::from_millis(60)
        };

        let first_cmd = rx.recv_timeout(timeout).ok();
        let mut should_exit = false;
        if let Some(cmd) = first_cmd
            && state.handle_cmd(cmd)
        {
            should_exit = true;
        }
        if !should_exit {
            while let Ok(cmd) = rx.try_recv() {
                if state.handle_cmd(cmd) {
                    should_exit = true;
                    break;
                }
            }
        }
        if should_exit {
            break;
        }

        state.drain_x11_events();

        if active || state.pill_shown || state.visible {
            state.on_tick();
        } else {
            state.last_tick = Instant::now();
        }
    }

    state.destroy();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_xft_dpi_scale_and_maps_logical_phys_rects() {
        let rm = "*customization:\t-color\nXft.dpi:\t192\nXft.hinting:\t1\n";
        assert_eq!(parse_xft_dpi_scale(rm), Some(2.0));
        assert_eq!(parse_xft_dpi_scale("Xft.dpi: 244\n"), Some(2.5));
        assert_eq!(parse_xft_dpi_scale("Xft.dpi: 96\n"), Some(1.0));
        assert_eq!(parse_xft_dpi_scale("Xcursor.size: 48\n"), None);

        let phys = WorkArea {
            left: 0,
            top: 0,
            right: 3072,
            bottom: 1728,
        };
        let logical = phys_to_logical_area(phys, 2.0);
        assert_eq!(logical.width(), 1536);
        assert_eq!(logical.height(), 864);

        let rect = OverlayRect {
            x: 100,
            y: 200,
            width: 224,
            height: 41,
        };
        let scaled = logical_to_phys_rect(rect, 2.0);
        assert_eq!(scaled.x, 200);
        assert_eq!(scaled.y, 400);
        assert_eq!(scaled.width, 448);
        assert_eq!(scaled.height, 82);
        assert_eq!(phys_to_logical_coord(205, 2.0), 102);
    }

    #[test]
    fn measures_wrapped_panel_text_non_empty() {
        let desc = panel_pango_font_desc("consolas", "bold-italic", 18);
        assert!(desc.contains("Bold Italic 18px"));
        let (w, h) = measure_wrapped_text("Hello Eqho dictation on Linux", &desc, 18, 400);
        assert!(w > 10 && w <= 400);
        assert!(h >= 12);
    }
}
