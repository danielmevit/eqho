//! Native Linux X11/XWayland 32-bit ARGB composited floating iridescent pill overlay.
//!
//! Dynamically loads `libX11.so.6` via `libloading` (zero link-time X11 dependency) and creates a
//! borderless, `override_redirect`, always-on-top, non-activating 32-bit TrueColor window that
//! blits `eqho_core::pillfx::render_premul_bgra` at ~60fps without ever stealing keyboard focus.
//! Degrades silently to no-op when `$DISPLAY` is absent (headless / pure Wayland).

use std::ffi::{c_char, c_int, c_long, c_uint, c_ulong, c_void};
use std::ptr::{null, null_mut};
use std::sync::mpsc;
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

struct X11OverlayApi {
    _lib: Library,
    open_display: unsafe extern "C" fn(*const c_char) -> *mut c_void,
    close_display: unsafe extern "C" fn(*mut c_void) -> c_int,
    default_screen: unsafe extern "C" fn(*mut c_void) -> c_int,
    root_window: unsafe extern "C" fn(*mut c_void, c_int) -> c_ulong,
    display_width: unsafe extern "C" fn(*mut c_void, c_int) -> c_int,
    display_height: unsafe extern "C" fn(*mut c_void, c_int) -> c_int,
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
}

impl X11OverlayApi {
    fn load() -> Option<Self> {
        // SAFETY: Standard Xlib functions in `libX11.so.6`.
        unsafe {
            let lib = Library::new("libX11.so.6").ok()?;
            Some(Self {
                open_display: *lib.get(b"XOpenDisplay\0").ok()?,
                close_display: *lib.get(b"XCloseDisplay\0").ok()?,
                default_screen: *lib.get(b"XDefaultScreen\0").ok()?,
                root_window: *lib.get(b"XRootWindow\0").ok()?,
                display_width: *lib.get(b"XDisplayWidth\0").ok()?,
                display_height: *lib.get(b"XDisplayHeight\0").ok()?,
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
                _lib: lib,
            })
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

        let (mut pill_w, mut pill_h) = pill_size(config.pill_scale);
        let work = WorkArea {
            left: 0,
            top: 0,
            right: (api.display_width)(dpy, screen),
            bottom: (api.display_height)(dpy, screen),
        };
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
        let gc = (api.create_gc)(dpy, win, 0, null_mut());

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
                            let w_now = WorkArea {
                                left: 0,
                                top: 0,
                                right: (api.display_width)(dpy, screen),
                                bottom: (api.display_height)(dpy, screen),
                            };
                            let (nw, nh) = pill_size(config.pill_scale);
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
                        let (nw, nh) = pill_size(config.pill_scale);
                        pill_w = nw;
                        pill_h = nh;
                        let w_now = WorkArea {
                            left: 0,
                            top: 0,
                            right: (api.display_width)(dpy, screen),
                            bottom: (api.display_height)(dpy, screen),
                        };
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
