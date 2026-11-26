//! Native macOS `NSPanel` floating iridescent pill overlay (non-activating, floats over all Spaces).
//!
//! Renders Eqho's exact pure-Rust `eqho_core::pillfx::render_premul_bgra` frames into a transparent
//! borderless `NSPanel` (`NSWindowStyleMaskNonactivatingPanel`, `NSFloatingWindowLevel`) on the
//! macOS main dispatch queue via `dispatch_async_f` so it never steals keyboard focus.

use std::ffi::{c_char, c_void};
use std::ptr::{null, null_mut};
use std::sync::mpsc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use eqho_core::pillfx::{pill_size, render_premul_bgra};

use crate::pure::{
    FRAME_MS, OverlayConfig, PillAnimator, WorkArea, calc_pill_position,
};

#[repr(C)]
#[derive(Clone, Copy)]
struct NSPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct NSSize {
    width: f64,
    height: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct NSRect {
    origin: NSPoint,
    size: NSSize,
}

const NS_WINDOW_STYLE_BORDERLESS: usize = 0;
const NS_WINDOW_STYLE_NONACTIVATING_PANEL: usize = 1 << 7;
const NS_BACKING_STORE_BUFFERED: usize = 2;
const NS_FLOATING_WINDOW_LEVEL: isize = 3;
// CanJoinAllSpaces (1 << 0) | Stationary (1 << 4) | IgnoresCycle (1 << 6) | FullScreenAuxiliary (1 << 8)
const NS_COLLECTION_BEHAVIOR: usize = (1 << 0) | (1 << 4) | (1 << 6) | (1 << 8);
// kCGImageAlphaPremultipliedFirst (2) | kCGBitmapByteOrder32Little (2 << 12) => premul BGRA8
const CG_BITMAP_INFO_PREMUL_BGRA: u32 = 2 | (2 << 12);

#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {}

#[link(name = "QuartzCore", kind = "framework")]
unsafe extern "C" {}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGMainDisplayID() -> u32;
    fn CGDisplayBounds(display: u32) -> NSRect;
    fn CGColorSpaceCreateDeviceRGB() -> *mut c_void;
    fn CGColorSpaceRelease(space: *mut c_void);
    fn CGDataProviderCreateWithData(
        info: *mut c_void,
        data: *const c_void,
        size: usize,
        release_data: Option<unsafe extern "C" fn(*mut c_void, *const c_void, usize)>,
    ) -> *mut c_void;
    fn CGDataProviderRelease(provider: *mut c_void);
    fn CGImageCreate(
        width: usize,
        height: usize,
        bits_per_component: usize,
        bits_per_pixel: usize,
        bytes_per_row: usize,
        space: *mut c_void,
        bitmap_info: u32,
        provider: *mut c_void,
        decode: *const f64,
        should_interpolate: bool,
        intent: i32,
    ) -> *mut c_void;
    fn CGImageRelease(image: *mut c_void);
}

#[link(name = "objc")]
unsafe extern "C" {
    fn objc_getClass(name: *const c_char) -> *mut c_void;
    fn sel_registerName(name: *const c_char) -> *mut c_void;
    fn objc_msgSend();
}

#[link(name = "System", kind = "framework")]
unsafe extern "C" {
    static _dispatch_main_q: c_void;
    fn dispatch_async_f(
        queue: *const c_void,
        context: *mut c_void,
        work: unsafe extern "C" fn(*mut c_void),
    );
}

pub(crate) enum MacOverlayCmd {
    Show,
    SetLevel(f32),
    SetWaveform(f32, f32),
    UpdateConfig(OverlayConfig),
    Hide,
    Shutdown,
}

#[derive(Debug)]
pub(crate) struct MacOverlayClient {
    tx: mpsc::Sender<MacOverlayCmd>,
    thread: Option<JoinHandle<()>>,
}

impl MacOverlayClient {
    pub(crate) fn start(config: OverlayConfig) -> Option<Self> {
        let (tx, rx) = mpsc::channel::<MacOverlayCmd>();
        let handle = thread::Builder::new()
            .name("eqho-overlay-macos".to_owned())
            .spawn(move || run_mac_overlay_thread(config, rx))
            .ok()?;
        Some(Self {
            tx,
            thread: Some(handle),
        })
    }

    pub(crate) fn send(&self, cmd: MacOverlayCmd) {
        let _ = self.tx.send(cmd);
    }

    pub(crate) fn shutdown(&mut self) {
        if let Some(handle) = self.thread.take() {
            let _ = self.tx.send(MacOverlayCmd::Shutdown);
            let _ = handle.join();
        }
    }
}

impl Drop for MacOverlayClient {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct MainThreadFrame {
    panel_slot: Arc<std::sync::atomic::AtomicUsize>,
    x: f64,
    y: f64,
    w: u32,
    h: u32,
    pixels: Vec<u8>,
    visible: bool,
    destroy: bool,
    in_flight: Arc<AtomicBool>,
}

unsafe extern "C" fn release_vec_data(info: *mut c_void, _data: *const c_void, _size: usize) {
    if !info.is_null() {
        // SAFETY: Reclaiming the `Box<Vec<u8>>` passed to `CGDataProviderCreateWithData`.
        unsafe {
            drop(Box::from_raw(info as *mut Vec<u8>));
        }
    }
}

unsafe extern "C" fn apply_frame_on_main_thread(ctx: *mut c_void) {
    if ctx.is_null() {
        return;
    }
    // SAFETY: Reclaiming boxed frame payload dispatched from the animator thread.
    let frame = unsafe { Box::from_raw(ctx as *mut MainThreadFrame) };
    let in_flight = Arc::clone(&frame.in_flight);

    // SAFETY: All AppKit / CALayer mutations run strictly on the macOS main queue (`_dispatch_main_q`).
    unsafe {
        type Msg0 = unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void;
        type MsgVoid = unsafe extern "C" fn(*mut c_void, *mut c_void);
        type MsgSetBool = unsafe extern "C" fn(*mut c_void, *mut c_void, u8);
        type MsgSetIsize = unsafe extern "C" fn(*mut c_void, *mut c_void, isize);
        type MsgSetUsize = unsafe extern "C" fn(*mut c_void, *mut c_void, usize);
        type MsgSetPtr = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void);
        type MsgInitPanel =
            unsafe extern "C" fn(*mut c_void, *mut c_void, NSRect, usize, usize, u8) -> *mut c_void;
        type MsgSetFrame = unsafe extern "C" fn(*mut c_void, *mut c_void, NSRect, u8);

        let send0: Msg0 = std::mem::transmute(objc_msgSend as *const ());
        let send_void: MsgVoid = std::mem::transmute(objc_msgSend as *const ());
        let send_bool: MsgSetBool = std::mem::transmute(objc_msgSend as *const ());
        let send_isize: MsgSetIsize = std::mem::transmute(objc_msgSend as *const ());
        let send_usize: MsgSetUsize = std::mem::transmute(objc_msgSend as *const ());
        let send_ptr: MsgSetPtr = std::mem::transmute(objc_msgSend as *const ());
        let send_init: MsgInitPanel = std::mem::transmute(objc_msgSend as *const ());
        let send_frame: MsgSetFrame = std::mem::transmute(objc_msgSend as *const ());

        let mut panel = frame.panel_slot.load(Ordering::Acquire) as *mut c_void;

        if frame.destroy {
            if !panel.is_null() {
                send_ptr(panel, sel_registerName(c"orderOut:".as_ptr()), null_mut());
                send_void(panel, sel_registerName(c"close".as_ptr()));
                frame.panel_slot.store(0, Ordering::Release);
            }
            in_flight.store(false, Ordering::Release);
            return;
        }

        if !frame.visible {
            if !panel.is_null() {
                send_ptr(panel, sel_registerName(c"orderOut:".as_ptr()), null_mut());
            }
            in_flight.store(false, Ordering::Release);
            return;
        }

        // Convert top-down screen y into Cocoa bottom-up coordinates on main display.
        let disp = CGDisplayBounds(CGMainDisplayID());
        let cocoa_y = disp.size.height - frame.y - f64::from(frame.h);
        let rect = NSRect {
            origin: NSPoint {
                x: frame.x,
                y: cocoa_y,
            },
            size: NSSize {
                width: f64::from(frame.w),
                height: f64::from(frame.h),
            },
        };

        if panel.is_null() {
            let panel_cls = objc_getClass(c"NSPanel".as_ptr());
            let color_cls = objc_getClass(c"NSColor".as_ptr());
            if !panel_cls.is_null() && !color_cls.is_null() {
                let alloc = send0(panel_cls, sel_registerName(c"alloc".as_ptr()));
                panel = send_init(
                    alloc,
                    sel_registerName(c"initWithContentRect:styleMask:backing:defer:".as_ptr()),
                    rect,
                    NS_WINDOW_STYLE_BORDERLESS | NS_WINDOW_STYLE_NONACTIVATING_PANEL,
                    NS_BACKING_STORE_BUFFERED,
                    0,
                );
                if !panel.is_null() {
                    let clear = send0(color_cls, sel_registerName(c"clearColor".as_ptr()));
                    send_bool(panel, sel_registerName(c"setOpaque:".as_ptr()), 0);
                    send_ptr(panel, sel_registerName(c"setBackgroundColor:".as_ptr()), clear);
                    send_bool(panel, sel_registerName(c"setHasShadow:".as_ptr()), 0);
                    send_bool(panel, sel_registerName(c"setIgnoresMouseEvents:".as_ptr()), 1);
                    send_isize(
                        panel,
                        sel_registerName(c"setLevel:".as_ptr()),
                        NS_FLOATING_WINDOW_LEVEL,
                    );
                    send_usize(
                        panel,
                        sel_registerName(c"setCollectionBehavior:".as_ptr()),
                        NS_COLLECTION_BEHAVIOR,
                    );
                    let content = send0(panel, sel_registerName(c"contentView".as_ptr()));
                    if !content.is_null() {
                        send_bool(content, sel_registerName(c"setWantsLayer:".as_ptr()), 1);
                    }
                    frame.panel_slot.store(panel as usize, Ordering::Release);
                }
            }
        }

        if !panel.is_null() {
            send_frame(panel, sel_registerName(c"setFrame:display:".as_ptr()), rect, 0);
            let content = send0(panel, sel_registerName(c"contentView".as_ptr()));
            if !content.is_null() {
                let layer = send0(content, sel_registerName(c"layer".as_ptr()));
                if !layer.is_null() && !frame.pixels.is_empty() {
                    let boxed = Box::new(frame.pixels);
                    let ptr = boxed.as_ptr();
                    let len = boxed.len();
                    let raw_box = Box::into_raw(boxed);
                    let provider = CGDataProviderCreateWithData(
                        raw_box.cast(),
                        ptr.cast(),
                        len,
                        Some(release_vec_data),
                    );
                    if !provider.is_null() {
                        let cs = CGColorSpaceCreateDeviceRGB();
                        let img = CGImageCreate(
                            frame.w as usize,
                            frame.h as usize,
                            8,
                            32,
                            (frame.w as usize) * 4,
                            cs,
                            CG_BITMAP_INFO_PREMUL_BGRA,
                            provider,
                            null(),
                            false,
                            0,
                        );
                        if !img.is_null() {
                            send_ptr(layer, sel_registerName(c"setContents:".as_ptr()), img);
                            CGImageRelease(img);
                        }
                        CGColorSpaceRelease(cs);
                        CGDataProviderRelease(provider);
                    } else {
                        drop(Box::from_raw(raw_box));
                    }
                }
            }
            send_void(panel, sel_registerName(c"orderFrontRegardless".as_ptr()));
        }
    }

    in_flight.store(false, Ordering::Release);
}

fn dispatch_frame(frame: MainThreadFrame) {
    let raw = Box::into_raw(Box::new(frame));
    // SAFETY: Dispatching frame update onto macOS main queue.
    unsafe {
        dispatch_async_f(
            &_dispatch_main_q as *const c_void,
            raw.cast(),
            apply_frame_on_main_thread,
        );
    }
}

fn run_mac_overlay_thread(mut config: OverlayConfig, rx: mpsc::Receiver<MacOverlayCmd>) {
    let panel_slot = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let in_flight = Arc::new(AtomicBool::new(false));
    let mut animator = PillAnimator::new();
    let mut shown = false;
    let mut last_tick = Instant::now();

    loop {
        let timeout = if shown || animator.alpha_logical > 0.001 {
            Duration::from_millis(FRAME_MS)
        } else {
            Duration::from_millis(60)
        };
        let first = rx.recv_timeout(timeout).ok();
        let mut should_exit = false;

        let mut handle = |cmd: MacOverlayCmd| -> bool {
            match cmd {
                MacOverlayCmd::Show => {
                    if config.enabled {
                        animator.fade_target = 1.0;
                        shown = true;
                    }
                    false
                }
                MacOverlayCmd::SetLevel(lvl) => {
                    animator.set_level(lvl);
                    false
                }
                MacOverlayCmd::SetWaveform(l, t) => {
                    animator.feed_voice(l, t);
                    false
                }
                MacOverlayCmd::UpdateConfig(cfg) => {
                    config = cfg;
                    if !config.enabled {
                        animator.fade_target = 0.0;
                        animator.alpha_logical = 0.0;
                        shown = false;
                        dispatch_frame(MainThreadFrame {
                            panel_slot: Arc::clone(&panel_slot),
                            x: 0.0,
                            y: 0.0,
                            w: 0,
                            h: 0,
                            pixels: Vec::new(),
                            visible: false,
                            destroy: false,
                            in_flight: Arc::clone(&in_flight),
                        });
                    }
                    false
                }
                MacOverlayCmd::Hide => {
                    animator.fade_target = 0.0;
                    false
                }
                MacOverlayCmd::Shutdown => true,
            }
        };

        if let Some(c) = first
            && handle(c)
        {
            should_exit = true;
        }
        if !should_exit {
            while let Ok(c) = rx.try_recv() {
                if handle(c) {
                    should_exit = true;
                    break;
                }
            }
        }
        if should_exit {
            dispatch_frame(MainThreadFrame {
                panel_slot: Arc::clone(&panel_slot),
                x: 0.0,
                y: 0.0,
                w: 0,
                h: 0,
                pixels: Vec::new(),
                visible: false,
                destroy: true,
                in_flight: Arc::clone(&in_flight),
            });
            break;
        }

        if shown {
            let now = Instant::now();
            let dt = now.duration_since(last_tick).as_secs_f32().clamp(0.001, 0.1);
            last_tick = now;

            let alpha_logical = animator.step_fade(dt);
            if alpha_logical <= 0.001 && animator.fade_target <= 0.0 {
                shown = false;
                dispatch_frame(MainThreadFrame {
                    panel_slot: Arc::clone(&panel_slot),
                    x: 0.0,
                    y: 0.0,
                    w: 0,
                    h: 0,
                    pixels: Vec::new(),
                    visible: false,
                    destroy: false,
                    in_flight: Arc::clone(&in_flight),
                });
                continue;
            }

            if in_flight.swap(true, Ordering::AcqRel) {
                continue;
            }
            let (pw, ph) = pill_size(config.pill_scale);
            // SAFETY: Querying main display bounds via CoreGraphics.
            let bounds = unsafe { CGDisplayBounds(CGMainDisplayID()) };
            let work = WorkArea {
                left: bounds.origin.x.round() as i32,
                top: bounds.origin.y.round() as i32,
                right: (bounds.origin.x + bounds.size.width).round() as i32,
                bottom: (bounds.origin.y + bounds.size.height).round() as i32,
            };
            let pos = calc_pill_position(&config.position, pw, ph, work);
            let frame = animator.step(dt, config.show_level);
            let light = if config.dark_theme { 0.0_f32 } else { 1.0_f32 };
            let opacity = f32::from(animator.effective_pill_alpha(&config)) / 255.0;
            let pixels = render_premul_bgra(
                pw,
                ph,
                frame.phase,
                frame.level,
                light,
                opacity,
                frame.wave,
            );
            dispatch_frame(MainThreadFrame {
                panel_slot: Arc::clone(&panel_slot),
                x: f64::from(pos.0),
                y: f64::from(pos.1),
                w: pw,
                h: ph,
                pixels,
                visible: true,
                destroy: false,
                in_flight: Arc::clone(&in_flight),
            });
        } else {
            last_tick = Instant::now();
        }
    }
}
