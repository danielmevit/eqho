//! Live Linux implementations of [`damt_platform`] contracts.

use std::ffi::{c_char, c_int, c_uint, c_ulong, c_void};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use damt_platform::{
    ActiveScreenProvider, AppThemeProvider, AutostartService, MotionPreference, Notifier,
    PlatformError, PlatformServices, ScreenRect, TaskbarThemeProvider,
};
use libloading::Library;

use crate::pure::{
    autostart_file_path, build_autostart_desktop_entry, parse_enable_animations,
    parse_portal_color_scheme, resolve_autostart_exec_path,
};

/// XDG Autostart `.desktop` manager (`~/.config/autostart/<id>.desktop`).
#[derive(Debug, Clone)]
pub struct LinuxAutostart {
    app_name: String,
    desktop_id: String,
}

impl LinuxAutostart {
    #[must_use]
    pub fn new(app_name: impl Into<String>) -> Self {
        let name = app_name.into();
        let id = name.to_ascii_lowercase().replace(' ', "-");
        Self {
            app_name: name,
            desktop_id: id,
        }
    }

    fn entry_path(&self) -> Option<PathBuf> {
        let xdg = std::env::var("XDG_CONFIG_HOME").ok();
        let home = std::env::var("HOME").ok();
        autostart_file_path(xdg.as_deref(), home.as_deref(), &self.desktop_id)
    }
}

impl AutostartService for LinuxAutostart {
    fn is_supported(&self) -> bool {
        self.entry_path().is_some()
    }

    fn is_enabled(&self) -> bool {
        self.entry_path().is_some_and(|p| p.is_file())
    }

    fn set_enabled(&self, enabled: bool) -> Result<(), PlatformError> {
        let Some(path) = self.entry_path() else {
            return Err(PlatformError(
                "cannot resolve XDG autostart directory (HOME not set)".to_owned(),
            ));
        };
        if !enabled {
            if path.exists() {
                fs::remove_file(&path).map_err(|e| PlatformError(e.to_string()))?;
            }
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| PlatformError(e.to_string()))?;
        }
        let exe = std::env::current_exe().map_err(|e| PlatformError(e.to_string()))?;
        let appimage = std::env::var("APPIMAGE").ok();
        let target = resolve_autostart_exec_path(appimage.as_deref(), &exe);
        let body = build_autostart_desktop_entry(
            &self.app_name,
            &target.to_string_lossy(),
            &self.desktop_id,
        );
        fs::write(&path, body).map_err(|e| PlatformError(e.to_string()))
    }
}

/// Probes Linux desktop light/dark theme via XDG Desktop Portal, GNOME `gsettings`, or `GTK_THEME`.
#[derive(Debug, Default, Clone, Copy)]
pub struct LinuxTheme;

impl LinuxTheme {
    fn probe_is_light() -> Option<bool> {
        // 1. XDG Desktop Portal (works on GNOME, KDE Plasma, Hyprland, Sway, Cosmic).
        if let Ok(out) = Command::new("gdbus")
            .args([
                "call",
                "--session",
                "--dest",
                "org.freedesktop.portal.Desktop",
                "--object-path",
                "/org/freedesktop/portal/desktop",
                "--method",
                "org.freedesktop.portal.Settings.Read",
                "org.freedesktop.appearance",
                "color-scheme",
            ])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            && out.status.success()
            && let Some(light) = parse_portal_color_scheme(&String::from_utf8_lossy(&out.stdout))
        {
            return Some(light);
        }

        // 2. GNOME gsettings fallback.
        if let Ok(out) = Command::new("gsettings")
            .args(["get", "org.gnome.desktop.interface", "color-scheme"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            && out.status.success()
            && let Some(light) = parse_portal_color_scheme(&String::from_utf8_lossy(&out.stdout))
        {
            return Some(light);
        }

        // 3. GTK_THEME environment variable hint.
        if let Ok(gtk) = std::env::var("GTK_THEME")
            && let Some(light) = parse_portal_color_scheme(&gtk)
        {
            return Some(light);
        }

        None
    }
}

impl AppThemeProvider for LinuxTheme {
    fn is_supported(&self) -> bool {
        true
    }

    fn is_light_app_theme(&self) -> Option<bool> {
        Self::probe_is_light()
    }
}

impl TaskbarThemeProvider for LinuxTheme {
    fn is_supported(&self) -> bool {
        true
    }

    fn is_light_taskbar(&self) -> Option<bool> {
        Self::probe_is_light()
    }
}

/// Probes Linux reduced-motion preference via GNOME `enable-animations`.
#[derive(Debug, Default, Clone, Copy)]
pub struct LinuxMotion;

impl MotionPreference for LinuxMotion {
    fn is_supported(&self) -> bool {
        true
    }

    fn prefers_reduced_motion(&self) -> bool {
        if let Ok(out) = Command::new("gsettings")
            .args(["get", "org.gnome.desktop.interface", "enable-animations"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            && out.status.success()
            && let Some(enabled) = parse_enable_animations(&String::from_utf8_lossy(&out.stdout))
        {
            return !enabled;
        }
        false
    }
}

/// Non-blocking desktop notification sender (`notify-send` with `gdbus` fallback).
#[derive(Debug, Clone)]
pub struct LinuxNotifier {
    app_name: String,
    icon_name: String,
}

impl LinuxNotifier {
    #[must_use]
    pub fn new(app_name: impl Into<String>, icon_name: impl Into<String>) -> Self {
        Self {
            app_name: app_name.into(),
            icon_name: icon_name.into(),
        }
    }
}

impl Notifier for LinuxNotifier {
    fn is_supported(&self) -> bool {
        true
    }

    fn notify(&self, title: &str, body: &str) -> Result<(), PlatformError> {
        let spawned = Command::new("notify-send")
            .args([
                "--app-name",
                &self.app_name,
                "--icon",
                &self.icon_name,
                "--",
                title,
                body,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();

        if spawned.is_ok() {
            return Ok(());
        }

        // Fallback to org.freedesktop.Notifications via gdbus.
        let _ = Command::new("gdbus")
            .args([
                "call",
                "--session",
                "--dest",
                "org.freedesktop.Notifications",
                "--object-path",
                "/org/freedesktop/Notifications",
                "--method",
                "org.freedesktop.Notifications.Notify",
                &self.app_name,
                "0",
                &self.icon_name,
                title,
                body,
                "[]",
                "{}",
                "4000",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        Ok(())
    }
}

/// Active monitor bounds via dynamic `libX11.so.6` + `libXrandr.so.2` (zero link-time dependency).
#[derive(Debug, Default, Clone, Copy)]
pub struct LinuxActiveScreen;

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

type XOpenDisplayFn = unsafe extern "C" fn(*const c_char) -> *mut c_void;
type XCloseDisplayFn = unsafe extern "C" fn(*mut c_void) -> c_int;
type XDefaultScreenFn = unsafe extern "C" fn(*mut c_void) -> c_int;
type XDefaultRootWindowFn = unsafe extern "C" fn(*mut c_void) -> c_ulong;
type XDisplayWidthFn = unsafe extern "C" fn(*mut c_void, c_int) -> c_int;
type XDisplayHeightFn = unsafe extern "C" fn(*mut c_void, c_int) -> c_int;
type XQueryPointerFn = unsafe extern "C" fn(
    *mut c_void,
    c_ulong,
    *mut c_ulong,
    *mut c_ulong,
    *mut c_int,
    *mut c_int,
    *mut c_int,
    *mut c_int,
    *mut c_uint,
) -> c_int;
type XrrGetMonitorsFn =
    unsafe extern "C" fn(*mut c_void, c_ulong, c_int, *mut c_int) -> *mut XrrMonitorInfo;
type XrrFreeMonitorsFn = unsafe extern "C" fn(*mut XrrMonitorInfo);

/// Process-lifetime handle to `libX11.so.6` and optional `libXrandr.so.2`.
///
/// Keeping both libraries loaded for the process lifetime is mandatory: `XRRGetMonitors`
/// registers an Xlib extension close-display callback (`XRRCloseDisplay`) on the `Display`.
/// Dropping/`dlclose`-ing `libXrandr.so.2` before `XCloseDisplay` causes an immediate SIGSEGV.
struct X11ScreenApi {
    _x11: Library,
    _xrr: Option<Library>,
    open_display: XOpenDisplayFn,
    close_display: XCloseDisplayFn,
    default_screen: XDefaultScreenFn,
    default_root_window: XDefaultRootWindowFn,
    display_width: XDisplayWidthFn,
    display_height: XDisplayHeightFn,
    query_pointer: XQueryPointerFn,
    xrr_get_monitors: Option<XrrGetMonitorsFn>,
    xrr_free_monitors: Option<XrrFreeMonitorsFn>,
}

// SAFETY: `X11ScreenApi` holds immutable function pointers and process-lifetime library handles;
// callers open a dedicated `Display*` per call and never share a `Display*` across threads.
unsafe impl Send for X11ScreenApi {}
unsafe impl Sync for X11ScreenApi {}

static X11_SCREEN_API: OnceLock<Option<X11ScreenApi>> = OnceLock::new();

fn load_x11_screen_api() -> Option<&'static X11ScreenApi> {
    X11_SCREEN_API
        .get_or_init(|| {
            // SAFETY: Dynamic lookup of standard Xlib / XRandR entry points on Linux.
            unsafe {
                let x11 = Library::new("libX11.so.6").ok()?;
                let open_display = *x11.get::<XOpenDisplayFn>(b"XOpenDisplay\0").ok()?;
                let close_display = *x11.get::<XCloseDisplayFn>(b"XCloseDisplay\0").ok()?;
                let default_screen = *x11.get::<XDefaultScreenFn>(b"XDefaultScreen\0").ok()?;
                let default_root_window =
                    *x11.get::<XDefaultRootWindowFn>(b"XDefaultRootWindow\0").ok()?;
                let display_width = *x11.get::<XDisplayWidthFn>(b"XDisplayWidth\0").ok()?;
                let display_height = *x11.get::<XDisplayHeightFn>(b"XDisplayHeight\0").ok()?;
                let query_pointer = *x11.get::<XQueryPointerFn>(b"XQueryPointer\0").ok()?;

                let xrr = Library::new("libXrandr.so.2").ok();
                let (xrr_get_monitors, xrr_free_monitors) = match xrr.as_ref() {
                    Some(lib) => match (
                        lib.get::<XrrGetMonitorsFn>(b"XRRGetMonitors\0"),
                        lib.get::<XrrFreeMonitorsFn>(b"XRRFreeMonitors\0"),
                    ) {
                        (Ok(get_m), Ok(free_m)) => (Some(*get_m), Some(*free_m)),
                        _ => (None, None),
                    },
                    None => (None, None),
                };

                Some(X11ScreenApi {
                    _x11: x11,
                    _xrr: xrr,
                    open_display,
                    close_display,
                    default_screen,
                    default_root_window,
                    display_width,
                    display_height,
                    query_pointer,
                    xrr_get_monitors,
                    xrr_free_monitors,
                })
            }
        })
        .as_ref()
}

impl ActiveScreenProvider for LinuxActiveScreen {
    fn is_supported(&self) -> bool {
        std::env::var_os("DISPLAY").is_some()
    }

    fn active_screen_bounds(&self) -> Option<ScreenRect> {
        std::env::var_os("DISPLAY")?;
        let api = load_x11_screen_api()?;
        // SAFETY: Dedicated short-lived X11 display connection using cached Xlib/XRandR entry points.
        unsafe {
            let dpy = (api.open_display)(std::ptr::null());
            if dpy.is_null() {
                return None;
            }
            let screen = (api.default_screen)(dpy);
            let root = (api.default_root_window)(dpy);
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

            let mut chosen: Option<ScreenRect> = None;
            if let (Some(get_mons), Some(free_mons)) =
                (api.xrr_get_monitors, api.xrr_free_monitors)
            {
                let mut n: c_int = 0;
                let ptr = get_mons(dpy, root, 1, &mut n);
                if !ptr.is_null() && n > 0 {
                    let slice = std::slice::from_raw_parts(ptr, n as usize);
                    let hit = slice
                        .iter()
                        .find(|m| {
                            rx >= m.x
                                && rx < m.x + m.width
                                && ry >= m.y
                                && ry < m.y + m.height
                        })
                        .or_else(|| slice.iter().find(|m| m.primary != 0))
                        .or_else(|| slice.first());
                    if let Some(m) = hit
                        && m.width > 0
                        && m.height > 0
                    {
                        chosen = Some(ScreenRect {
                            x: m.x,
                            y: m.y,
                            width: m.width,
                            height: m.height,
                        });
                    }
                    free_mons(ptr);
                }
            }

            let fallback = ScreenRect {
                x: 0,
                y: 0,
                width: (api.display_width)(dpy, screen),
                height: (api.display_height)(dpy, screen),
            };
            (api.close_display)(dpy);
            chosen.or(if fallback.width > 0 && fallback.height > 0 {
                Some(fallback)
            } else {
                None
            })
        }
    }
}

/// Combined Linux `PlatformServices` implementation for desktop apps.
#[derive(Debug)]
pub struct LinuxPlatform {
    autostart: LinuxAutostart,
    screen: LinuxActiveScreen,
    motion: LinuxMotion,
    theme: LinuxTheme,
    notifier: Option<LinuxNotifier>,
}

impl LinuxPlatform {
    #[must_use]
    pub fn new(app_name: impl Into<String>) -> Self {
        Self {
            autostart: LinuxAutostart::new(app_name),
            screen: LinuxActiveScreen,
            motion: LinuxMotion,
            theme: LinuxTheme,
            notifier: None,
        }
    }

    #[must_use]
    pub fn with_notifier(mut self, notifier: LinuxNotifier) -> Self {
        self.notifier = Some(notifier);
        self
    }
}

impl PlatformServices for LinuxPlatform {
    fn autostart(&self) -> &dyn AutostartService {
        &self.autostart
    }

    fn active_screen(&self) -> &dyn ActiveScreenProvider {
        &self.screen
    }

    fn motion(&self) -> &dyn MotionPreference {
        &self.motion
    }

    fn taskbar_theme(&self) -> &dyn TaskbarThemeProvider {
        &self.theme
    }

    fn app_theme(&self) -> &dyn AppThemeProvider {
        &self.theme
    }

    fn notifier(&self) -> &dyn Notifier {
        match &self.notifier {
            Some(n) => n,
            None => &damt_platform::Unsupported,
        }
    }
}
