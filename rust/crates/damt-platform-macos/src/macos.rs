//! Live macOS implementations of [`damt_platform`] contracts.

use std::ffi::c_void;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use damt_platform::{
    ActiveScreenProvider, AppThemeProvider, AutostartService, MotionPreference, Notifier,
    PlatformError, PlatformServices, ScreenRect, TaskbarThemeProvider,
};

use crate::pure::{
    build_launch_agent_plist, escape_applescript_string, launch_agent_plist_path,
    parse_apple_interface_style, parse_reduce_motion,
};

#[repr(C)]
#[derive(Clone, Copy)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGSize {
    width: f64,
    height: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CGRect {
    origin: CGPoint,
    size: CGSize,
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGMainDisplayID() -> u32;
    fn CGDisplayBounds(display: u32) -> CGRect;
    fn CGEventCreate(source: *const c_void) -> *mut c_void;
    fn CGEventGetLocation(event: *const c_void) -> CGPoint;
    fn CGGetDisplaysWithPoint(
        point: CGPoint,
        max_displays: u32,
        displays: *mut u32,
        matching_display_count: *mut u32,
    ) -> i32;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(cf: *const c_void);
}

/// macOS LaunchAgent autostart manager (`~/Library/LaunchAgents/<label>.plist`).
#[derive(Debug, Clone)]
pub struct MacAutostart {
    label: String,
}

impl MacAutostart {
    #[must_use]
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
        }
    }

    fn plist_path(&self) -> Option<PathBuf> {
        let home = std::env::var("HOME").ok();
        launch_agent_plist_path(home.as_deref(), &self.label)
    }
}

impl AutostartService for MacAutostart {
    fn is_supported(&self) -> bool {
        self.plist_path().is_some()
    }

    fn is_enabled(&self) -> bool {
        self.plist_path().is_some_and(|p| p.is_file())
    }

    fn set_enabled(&self, enabled: bool) -> Result<(), PlatformError> {
        let Some(path) = self.plist_path() else {
            return Err(PlatformError("HOME not set for LaunchAgents".to_owned()));
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
        let xml = build_launch_agent_plist(&self.label, &exe.to_string_lossy());
        fs::write(&path, xml).map_err(|e| PlatformError(e.to_string()))
    }
}

/// Probes macOS Dark/Light mode via `AppleInterfaceStyle`.
#[derive(Debug, Default, Clone, Copy)]
pub struct MacTheme;

impl MacTheme {
    fn probe_is_light() -> Option<bool> {
        let out = Command::new("defaults")
            .args(["read", "-g", "AppleInterfaceStyle"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        parse_apple_interface_style(&String::from_utf8_lossy(&out.stdout), out.status.success())
    }
}

impl AppThemeProvider for MacTheme {
    fn is_supported(&self) -> bool {
        true
    }

    fn is_light_app_theme(&self) -> Option<bool> {
        Self::probe_is_light()
    }
}

impl TaskbarThemeProvider for MacTheme {
    fn is_supported(&self) -> bool {
        true
    }

    fn is_light_taskbar(&self) -> Option<bool> {
        Self::probe_is_light()
    }
}

/// Probes macOS accessibility `reduceMotion` preference.
#[derive(Debug, Default, Clone, Copy)]
pub struct MacMotion;

impl MotionPreference for MacMotion {
    fn is_supported(&self) -> bool {
        true
    }

    fn prefers_reduced_motion(&self) -> bool {
        Command::new("defaults")
            .args(["read", "com.apple.universalaccess", "reduceMotion"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()
            .filter(|o| o.status.success())
            .is_some_and(|o| parse_reduce_motion(&String::from_utf8_lossy(&o.stdout)))
    }
}

/// CoreGraphics active display bounds (display under cursor, falling back to main display).
#[derive(Debug, Default, Clone, Copy)]
pub struct MacActiveScreen;

impl ActiveScreenProvider for MacActiveScreen {
    fn is_supported(&self) -> bool {
        true
    }

    fn active_screen_bounds(&self) -> Option<ScreenRect> {
        // SAFETY: CoreGraphics display and pointer queries.
        unsafe {
            let mut display_id = CGMainDisplayID();
            let ev = CGEventCreate(std::ptr::null());
            if !ev.is_null() {
                let pt = CGEventGetLocation(ev);
                CFRelease(ev.cast());
                let mut hit: u32 = 0;
                let mut count: u32 = 0;
                if CGGetDisplaysWithPoint(pt, 1, &mut hit, &mut count) == 0 && count >= 1 {
                    display_id = hit;
                }
            }
            let r = CGDisplayBounds(display_id);
            if r.size.width > 0.0 && r.size.height > 0.0 {
                Some(ScreenRect {
                    x: r.origin.x.round() as i32,
                    y: r.origin.y.round() as i32,
                    width: r.size.width.round() as i32,
                    height: r.size.height.round() as i32,
                })
            } else {
                None
            }
        }
    }
}

/// Non-blocking macOS notification sender.
#[derive(Debug, Default, Clone, Copy)]
pub struct MacNotifier;

impl Notifier for MacNotifier {
    fn is_supported(&self) -> bool {
        true
    }

    fn notify(&self, title: &str, body: &str) -> Result<(), PlatformError> {
        let t = escape_applescript_string(title);
        let b = escape_applescript_string(body);
        let script = format!("display notification \"{b}\" with title \"{t}\"");
        let _ = Command::new("osascript")
            .args(["-e", &script])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        Ok(())
    }
}

/// Combined macOS `PlatformServices` implementation.
#[derive(Debug)]
pub struct MacPlatform {
    autostart: MacAutostart,
    screen: MacActiveScreen,
    motion: MacMotion,
    theme: MacTheme,
    notifier: Option<MacNotifier>,
}

impl MacPlatform {
    #[must_use]
    pub fn new(bundle_id: impl Into<String>) -> Self {
        Self {
            autostart: MacAutostart::new(bundle_id),
            screen: MacActiveScreen,
            motion: MacMotion,
            theme: MacTheme,
            notifier: None,
        }
    }

    #[must_use]
    pub fn with_notifier(mut self, notifier: MacNotifier) -> Self {
        self.notifier = Some(notifier);
        self
    }
}

impl PlatformServices for MacPlatform {
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
