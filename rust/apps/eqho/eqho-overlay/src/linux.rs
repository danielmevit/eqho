//! Native Linux X11/XWayland 32-bit ARGB composited floating iridescent pill overlay.
//!
//! Dynamically loads `libX11.so.6` (plus optional `libXrandr.so.2` for active-monitor bounds and
//! `libXext.so.6` for click-through `ShapeInput`) via `libloading` and keeps the libraries loaded
//! for the process lifetime. Creates a borderless, `override_redirect`, always-on-top,
//! click-through, non-activating 32-bit TrueColor window that blits
//! `eqho_core::pillfx::render_premul_bgra` without ever stealing keyboard focus.
//! Degrades silently to no-op when `$DISPLAY` is absent (headless / pure Wayland).

use std::ffi::{c_char, c_int, c_long, c_uchar, c_uint, c_ulong, c_void};
use std::ptr::{null, null_mut};
use std::sync::{OnceLock, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use eqho_core::pillfx::{pill_size, render_premul_bgra};
use libloading::Library;

use crate::pure::{
    FRAME_MS, OverlayConfig, PillAnimator, WorkArea, calc_pill_position,
};

const INPUT_OUTPUT: c_uint = 1;
const TRUE_COLOR: c_int = 4;
const Z_PIXMAP: c_int = 2;
const CW_BACK_PIXEL: c_ulong = 1 << 1;
const CW_BORDER_PIXEL: c_ulong = 1 << 3;
const CW_OVERRIDE_REDIRECT: c_ulong = 1 << 9;
const CW_COLORMAP: c_ulong = 1 << 13;
const ALLOC_NONE: c_int = 0;
const PROP_MODE_REPLACE: c_int = 0;
const XA_ATOM: c_ulong = 4;
const XA_CARDINAL: c_ulong = 6;
const SHAPE_SET: c_int = 0;
const SHAPE_INPUT: c_int = 2;

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
    // Remaining `f` function-pointer table is allocated by Xlib inside `XCreateImage`.
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

pub(crate) enum LinuxOverlayCmd {
    Show,
    SetLevel(f32),
    SetWaveform(f32, f32),
    UpdateConfig(OverlayConfig),
    Hide,
    Shutdown,
}

#[derive(Debug)]
pub(crate) struct LinuxOverlayClient {
    tx: mpsc::Sender<LinuxOverlayCmd>,
    thread: Option<JoinHandle<()>>,
}

impl LinuxOverlayClient {
    pub(crate) fn start(config: OverlayConfig) -> Option<Self> {
        std::env::var_os("DISPLAY")?;
        let (tx, rx) = mpsc::channel::<LinuxOverlayCmd>();
        let handle = thread::Builder::new()
            .name("eqho-overlay-linux".to_owned())
            .spawn(move || run_linux_overlay_thread(config, rx))
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
    *const c_void,
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

unsafe fn query_active_work_area(
    api: &X11OverlayApi,
    dpy: *mut c_void,
    screen: c_int,
    root: c_ulong,
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
                let hit = slice
                    .iter()
                    .find(|m| {
                        rx >= m.x && rx < m.x + m.width && ry >= m.y && ry < m.y + m.height
                    })
                    .or_else(|| slice.iter().find(|m| m.primary != 0))
                    .or_else(|| slice.first());
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

fn scaled_pill_size(pill_scale: f64, dpi_scale: f64) -> (u32, u32) {
    let (base_w, base_h) = pill_size(pill_scale);
    let s = if dpi_scale.is_finite() {
        dpi_scale.clamp(1.0, 2.5)
    } else {
        1.0
    };
    (
        ((f64::from(base_w)) * s).round().max(1.0) as u32,
        ((f64::from(base_h)) * s).round().max(1.0) as u32,
    )
}

unsafe fn apply_overlay_window_hints(api: &X11OverlayApi, dpy: *mut c_void, win: c_ulong) {
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

        // Empty ShapeInput region makes the floating pill completely click-through.
        if let Some(shape_rects) = api.xshape_combine_rectangles {
            shape_rects(dpy, win, SHAPE_INPUT, 0, 0, null(), 0, SHAPE_SET, 0);
        }
    }
}

fn run_linux_overlay_thread(mut config: OverlayConfig, rx: mpsc::Receiver<LinuxOverlayCmd>) {
    let Some(api) = X11OverlayApi::load() else {
        return;
    };
    // SAFETY: Dedicated X11 connection owned exclusively by this overlay thread.
    unsafe {
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
        let mut attrs: XSetWindowAttributes = std::mem::zeroed();
        attrs.background_pixel = 0;
        attrs.border_pixel = 0;
        attrs.override_redirect = 1; // Non-activating floating overlay, zero WM chrome
        attrs.colormap = colormap;

        let mut dpi_scale = query_dpi_scale(api, dpy, root);
        let (mut pill_w, mut pill_h) = scaled_pill_size(config.pill_scale, dpi_scale);
        let work = query_active_work_area(api, dpy, screen, root);
        let mut pos = calc_pill_position(&config.position, pill_w, pill_h, work);

        let win = (api.create_window)(
            dpy,
            root,
            pos.0,
            pos.1,
            pill_w,
            pill_h,
            0,
            vinfo.depth,
            INPUT_OUTPUT,
            vinfo.visual,
            CW_BACK_PIXEL | CW_BORDER_PIXEL | CW_OVERRIDE_REDIRECT | CW_COLORMAP,
            &mut attrs,
        );
        apply_overlay_window_hints(api, dpy, win);
        let gc = (api.create_gc)(dpy, win, 0, null_mut());
        (api.flush)(dpy);

        let mut animator = PillAnimator::new();
        let mut shown = false;
        let mut last_tick = Instant::now();

        loop {
            let timeout = if shown || animator.alpha_logical > 0.001 {
                Duration::from_millis(FRAME_MS)
            } else {
                Duration::from_millis(60)
            };

            let first_cmd = rx.recv_timeout(timeout).ok();
            let mut should_exit = false;

            let mut handle_cmd = |cmd: LinuxOverlayCmd| -> bool {
                match cmd {
                    LinuxOverlayCmd::Show => {
                        if !config.enabled {
                            return false;
                        }
                        animator.fade_target = 1.0;
                        if !shown {
                            dpi_scale = query_dpi_scale(api, dpy, root);
                            let w_now = query_active_work_area(api, dpy, screen, root);
                            let (nw, nh) = scaled_pill_size(config.pill_scale, dpi_scale);
                            pill_w = nw;
                            pill_h = nh;
                            pos = calc_pill_position(&config.position, pill_w, pill_h, w_now);
                            (api.move_resize_window)(dpy, win, pos.0, pos.1, pill_w, pill_h);
                            (api.map_raised)(dpy, win);
                            shown = true;
                        }
                        false
                    }
                    LinuxOverlayCmd::SetLevel(lvl) => {
                        animator.set_level(lvl);
                        false
                    }
                    LinuxOverlayCmd::SetWaveform(l, t) => {
                        animator.feed_voice(l, t);
                        false
                    }
                    LinuxOverlayCmd::UpdateConfig(cfg) => {
                        config = cfg;
                        dpi_scale = query_dpi_scale(api, dpy, root);
                        let (nw, nh) = scaled_pill_size(config.pill_scale, dpi_scale);
                        pill_w = nw;
                        pill_h = nh;
                        let w_now = query_active_work_area(api, dpy, screen, root);
                        pos = calc_pill_position(&config.position, pill_w, pill_h, w_now);
                        if shown {
                            (api.move_resize_window)(dpy, win, pos.0, pos.1, pill_w, pill_h);
                        }
                        if !config.enabled && shown {
                            (api.unmap_window)(dpy, win);
                            shown = false;
                            animator.alpha_logical = 0.0;
                            animator.fade_target = 0.0;
                        }
                        false
                    }
                    LinuxOverlayCmd::Hide => {
                        animator.fade_target = 0.0;
                        false
                    }
                    LinuxOverlayCmd::Shutdown => true,
                }
            };

            if let Some(cmd) = first_cmd
                && handle_cmd(cmd)
            {
                should_exit = true;
            }
            if !should_exit {
                while let Ok(cmd) = rx.try_recv() {
                    if handle_cmd(cmd) {
                        should_exit = true;
                        break;
                    }
                }
            }
            if should_exit {
                break;
            }

            if shown {
                let now = Instant::now();
                let dt = now.duration_since(last_tick).as_secs_f32().clamp(0.001, 0.1);
                last_tick = now;

                let alpha_logical = animator.step_fade(dt);
                if alpha_logical <= 0.001 && animator.fade_target <= 0.0 {
                    (api.unmap_window)(dpy, win);
                    (api.flush)(dpy);
                    shown = false;
                    continue;
                }

                let frame = animator.step(dt, config.show_level);
                let light = if config.dark_theme { 0.0_f32 } else { 1.0_f32 };
                let opacity = f32::from(animator.effective_pill_alpha(&config)) / 255.0;
                let mut pixels = render_premul_bgra(
                    pill_w,
                    pill_h,
                    frame.phase,
                    frame.level,
                    light,
                    opacity,
                    frame.wave,
                );

                // XCreateImage wraps our Rust Vec without copying; we free only the XImage header
                // struct via `XFree` (never `XDestroyImage`, which would `free()` Rust's buffer).
                let ximg = (api.create_image)(
                    dpy,
                    vinfo.visual,
                    vinfo.depth as c_uint,
                    Z_PIXMAP,
                    0,
                    pixels.as_mut_ptr().cast(),
                    pill_w,
                    pill_h,
                    32,
                    (pill_w * 4) as c_int,
                );
                if !ximg.is_null() {
                    (api.put_image)(dpy, win, gc, ximg, 0, 0, 0, 0, pill_w, pill_h);
                    (api.flush)(dpy);
                    (api.free)(ximg.cast());
                }
            } else {
                last_tick = Instant::now();
            }
        }

        if shown {
            (api.unmap_window)(dpy, win);
        }
        (api.free_gc)(dpy, gc);
        (api.destroy_window)(dpy, win);
        (api.free_colormap)(dpy, colormap);
        (api.close_display)(dpy);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_xft_dpi_scale_and_scales_pill() {
        let rm = "*customization:\t-color\nXft.dpi:\t192\nXft.hinting:\t1\n";
        assert_eq!(parse_xft_dpi_scale(rm), Some(2.0));
        assert_eq!(parse_xft_dpi_scale("Xft.dpi: 244\n"), Some(2.5));
        assert_eq!(parse_xft_dpi_scale("Xft.dpi: 96\n"), Some(1.0));
        assert_eq!(parse_xft_dpi_scale("Xcursor.size: 48\n"), None);

        let (w1, h1) = scaled_pill_size(0.8, 1.0);
        let (w2, h2) = scaled_pill_size(0.8, 2.0);
        assert_eq!((w1, h1), (224, 41));
        assert_eq!((w2, h2), (448, 82));
    }
}
