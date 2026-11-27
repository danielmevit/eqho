//! Real-time hardware-accelerated Apple-style frosted glass backdrop windows for Windows 10/11
//! using `Windows.UI.Composition` (`HostBackdropBrush` + Direct2D `GaussianBlur` + `Saturation`
//! vibrancy + hardware-antialiased `CompositionRoundedRectangleGeometry` clip).
//!
//! Why a companion glass window? On Windows 11, DWM ignores backdrop blur on `UpdateLayeredWindow`
//! (`ULW_ALPHA`) windows. Placing a click-through, non-activating `WS_EX_NOREDIRECTIONBITMAP`
//! Composition backdrop window strictly one Z-order step beneath each layered foreground window
//! (`panel_hwnd`, `recovery_hwnd`, `popup_hwnd`) lets DWM render a real live 60 fps GPU Gaussian
//! blur with custom rounded corners (`14–16 px`), adjustable blur radius (`0%–100%`), and
//! independent tint opacity while keeping crisp antialiased foreground text and hit-testing intact.

use std::ffi::c_void;

use windows::Foundation::{IPropertyValue, PropertyValue};
use windows::Graphics::Effects::{
    IGraphicsEffect, IGraphicsEffect_Impl, IGraphicsEffectSource, IGraphicsEffectSource_Impl,
};
use windows::System::DispatcherQueueController;
use windows::UI::Composition::Desktop::DesktopWindowTarget;
use windows::UI::Composition::{
    CompositionBackdropBrush, CompositionBrush, CompositionEffectSourceParameter,
    CompositionGeometricClip, CompositionRoundedRectangleGeometry, Compositor, SpriteVisual,
};
use windows::Win32::Foundation::{E_INVALIDARG, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Direct2D::{
    CLSID_D2D1GaussianBlur, CLSID_D2D1Saturation, D2D1_GAUSSIANBLUR_OPTIMIZATION_BALANCED,
};
use windows::Win32::Graphics::Dwm::{DWMWA_USE_HOSTBACKDROPBRUSH, DwmSetWindowAttribute};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::WinRT::Composition::ICompositorDesktopInterop;
use windows::Win32::System::WinRT::Graphics::Direct2D::{
    GRAPHICS_EFFECT_PROPERTY_MAPPING, GRAPHICS_EFFECT_PROPERTY_MAPPING_DIRECT,
    IGraphicsEffectD2D1Interop, IGraphicsEffectD2D1Interop_Impl,
};
use windows::Win32::System::WinRT::{
    CreateDispatcherQueueController, DQTAT_COM_STA, DQTYPE_THREAD_CURRENT, DispatcherQueueOptions,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, HMENU, MA_NOACTIVATE, RegisterClassExW,
    SW_HIDE, SWP_NOACTIVATE, SWP_SHOWWINDOW, SetWindowPos, ShowWindow, UnregisterClassW,
    WM_MOUSEACTIVATE, WNDCLASSEXW, WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{BOOL, GUID, HSTRING, Interface, PCWSTR, Result, implement};
use windows_numerics::Vector2;

use crate::pure::OverlayRect;

const GLASS_CLASS_NAME: &str = "EqhoGlassBackdrop";
/// Direct2D `D2D1_BORDER_MODE_HARD` (`1`) prevents dark transparent vignette bleed along window edges.
const D2D1_BORDER_MODE_HARD_U32: u32 = 1;

#[implement(IGraphicsEffect, IGraphicsEffectSource, IGraphicsEffectD2D1Interop)]
struct D2dGaussianBlurEffect {
    name: HSTRING,
    std_dev: f32,
    source: IGraphicsEffectSource,
}

impl IGraphicsEffectSource_Impl for D2dGaussianBlurEffect_Impl {}

impl IGraphicsEffect_Impl for D2dGaussianBlurEffect_Impl {
    fn Name(&self) -> Result<HSTRING> {
        Ok(self.name.clone())
    }

    fn SetName(&self, _name: &HSTRING) -> Result<()> {
        Ok(())
    }
}

impl IGraphicsEffectD2D1Interop_Impl for D2dGaussianBlurEffect_Impl {
    fn GetEffectId(&self) -> Result<GUID> {
        Ok(CLSID_D2D1GaussianBlur)
    }

    fn GetNamedPropertyMapping(
        &self,
        name: &PCWSTR,
        index: *mut u32,
        mapping: *mut GRAPHICS_EFFECT_PROPERTY_MAPPING,
    ) -> Result<()> {
        // SAFETY: `name`, `index`, and `mapping` are valid pointers supplied by Composition.
        let prop_name = unsafe { name.to_string().unwrap_or_default() };
        if prop_name.eq_ignore_ascii_case("BlurAmount") {
            unsafe {
                if !index.is_null() {
                    *index = 0;
                }
                if !mapping.is_null() {
                    *mapping = GRAPHICS_EFFECT_PROPERTY_MAPPING_DIRECT;
                }
            }
            Ok(())
        } else {
            Err(E_INVALIDARG.into())
        }
    }

    fn GetPropertyCount(&self) -> Result<u32> {
        Ok(3)
    }

    fn GetProperty(&self, index: u32) -> Result<IPropertyValue> {
        let insp = match index {
            0 => PropertyValue::CreateSingle(self.std_dev)?,
            1 => PropertyValue::CreateUInt32(D2D1_GAUSSIANBLUR_OPTIMIZATION_BALANCED.0 as u32)?,
            2 => PropertyValue::CreateUInt32(D2D1_BORDER_MODE_HARD_U32)?,
            _ => return Err(E_INVALIDARG.into()),
        };
        insp.cast()
    }

    fn GetSource(&self, index: u32) -> Result<IGraphicsEffectSource> {
        if index == 0 {
            Ok(self.source.clone())
        } else {
            Err(E_INVALIDARG.into())
        }
    }

    fn GetSourceCount(&self) -> Result<u32> {
        Ok(1)
    }
}

#[implement(IGraphicsEffect, IGraphicsEffectSource, IGraphicsEffectD2D1Interop)]
struct D2dSaturationEffect {
    name: HSTRING,
    saturation: f32,
    source: IGraphicsEffectSource,
}

impl IGraphicsEffectSource_Impl for D2dSaturationEffect_Impl {}

impl IGraphicsEffect_Impl for D2dSaturationEffect_Impl {
    fn Name(&self) -> Result<HSTRING> {
        Ok(self.name.clone())
    }

    fn SetName(&self, _name: &HSTRING) -> Result<()> {
        Ok(())
    }
}

impl IGraphicsEffectD2D1Interop_Impl for D2dSaturationEffect_Impl {
    fn GetEffectId(&self) -> Result<GUID> {
        Ok(CLSID_D2D1Saturation)
    }

    fn GetNamedPropertyMapping(
        &self,
        name: &PCWSTR,
        index: *mut u32,
        mapping: *mut GRAPHICS_EFFECT_PROPERTY_MAPPING,
    ) -> Result<()> {
        // SAFETY: `name`, `index`, and `mapping` are valid pointers supplied by Composition.
        let prop_name = unsafe { name.to_string().unwrap_or_default() };
        if prop_name.eq_ignore_ascii_case("Saturation") {
            unsafe {
                if !index.is_null() {
                    *index = 0;
                }
                if !mapping.is_null() {
                    *mapping = GRAPHICS_EFFECT_PROPERTY_MAPPING_DIRECT;
                }
            }
            Ok(())
        } else {
            Err(E_INVALIDARG.into())
        }
    }

    fn GetPropertyCount(&self) -> Result<u32> {
        Ok(1)
    }

    fn GetProperty(&self, index: u32) -> Result<IPropertyValue> {
        if index == 0 {
            PropertyValue::CreateSingle(self.saturation)?.cast()
        } else {
            Err(E_INVALIDARG.into())
        }
    }

    fn GetSource(&self, index: u32) -> Result<IGraphicsEffectSource> {
        if index == 0 {
            Ok(self.source.clone())
        } else {
            Err(E_INVALIDARG.into())
        }
    }

    fn GetSourceCount(&self) -> Result<u32> {
        Ok(1)
    }
}

unsafe extern "system" fn glass_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_MOUSEACTIVATE {
        return LRESULT(MA_NOACTIVATE as isize);
    }
    // SAFETY: Standard Win32 default window procedure fallback.
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// Shared per-thread `Windows.UI.Composition` runtime (`DispatcherQueueController` + `Compositor` +
/// `HostBackdropBrush`) owned by the Win32 overlay thread.
pub(crate) struct GlassCompositor {
    hinstance: windows::Win32::Foundation::HINSTANCE,
    com_initialized: bool,
    _dispatcher: DispatcherQueueController,
    compositor: Compositor,
    host_backdrop: CompositionBackdropBrush,
}

impl GlassCompositor {
    pub(crate) fn new(hinstance: windows::Win32::Foundation::HINSTANCE) -> Option<Self> {
        // SAFETY: Initializing STA COM on the dedicated overlay thread for WinRT Composition.
        let com_hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        let com_initialized = com_hr.is_ok();

        let opts = DispatcherQueueOptions {
            dwSize: std::mem::size_of::<DispatcherQueueOptions>() as u32,
            threadType: DQTYPE_THREAD_CURRENT,
            apartmentType: DQTAT_COM_STA,
        };
        // SAFETY: Creating a DispatcherQueue bound to the current Win32 message-loop thread.
        let dispatcher = unsafe { CreateDispatcherQueueController(opts) }.ok()?;
        let compositor = Compositor::new().ok()?;
        let host_backdrop = compositor.CreateHostBackdropBrush().ok()?;

        let class_name: Vec<u16> = format!("{GLASS_CLASS_NAME}\0").encode_utf16().collect();
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(glass_wnd_proc),
            hInstance: hinstance,
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        // SAFETY: Registering glass backdrop window class on the overlay thread.
        unsafe {
            let _ = RegisterClassExW(&wc);
        }

        Some(Self {
            hinstance,
            com_initialized,
            _dispatcher: dispatcher,
            compositor,
            host_backdrop,
        })
    }

    pub(crate) fn create_window(&self) -> Option<GlassWindow> {
        let class_name: Vec<u16> = format!("{GLASS_CLASS_NAME}\0").encode_utf16().collect();
        let ex_style = WS_EX_NOREDIRECTIONBITMAP
            | WS_EX_TRANSPARENT
            | WS_EX_NOACTIVATE
            | WS_EX_TOOLWINDOW
            | WS_EX_TOPMOST;
        // SAFETY: Creating a click-through, non-activating Win32 Composition host popup window.
        let hwnd = unsafe {
            CreateWindowExW(
                ex_style,
                PCWSTR(class_name.as_ptr()),
                PCWSTR::null(),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                Some(HMENU::default()),
                Some(self.hinstance),
                None,
            )
        }
        .ok()?;

        // Enable Win11 HostBackdropBrush sampling behind this non-redirection window (ignored on Win10).
        let use_host = BOOL(1);
        // SAFETY: Passing a valid `BOOL` to `DWMWA_USE_HOSTBACKDROPBRUSH`.
        unsafe {
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_HOSTBACKDROPBRUSH,
                (&use_host as *const BOOL).cast::<c_void>(),
                std::mem::size_of::<BOOL>() as u32,
            );
        }

        let interop: ICompositorDesktopInterop = self.compositor.cast().ok()?;
        // SAFETY: Binding our Win32 HWND to a WinRT `DesktopWindowTarget`.
        let target: DesktopWindowTarget =
            unsafe { interop.CreateDesktopWindowTarget(hwnd, true) }.ok()?;
        let visual = self.compositor.CreateSpriteVisual().ok()?;
        let rounded_geom = self.compositor.CreateRoundedRectangleGeometry().ok()?;
        let clip = self
            .compositor
            .CreateGeometricClipWithGeometry(&rounded_geom)
            .ok()?;
        visual.SetClip(&clip).ok()?;
        target.SetRoot(&visual).ok()?;

        Some(GlassWindow {
            hwnd,
            _target: target,
            visual,
            rounded_geom,
            _clip: clip,
            shown: false,
            last_blur_pct: 255,
            last_rect: OverlayRect {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
            last_radius: -1.0,
        })
    }

    fn build_glass_brush(&self, blur_pct: u8) -> Option<CompositionBrush> {
        let pct = f32::from(blur_pct.min(100)) / 100.0;
        // Smooth Apple-glass Gaussian blur curve: 1.5 px at 5% up to creamy 28 px at 100%
        // (D2D std_dev=28 ≈ ~84 px frosted glass kernel).
        let std_dev = (pct.powf(1.15) * 26.5 + 1.5).clamp(1.5, 28.0);
        // Subtle vibrancy saturation lift (1.05x -> 1.38x) so colors glow like Apple Glass.
        let saturation = 1.04 + pct * 0.34;

        let src_name = HSTRING::from("backdrop");
        let src_param = CompositionEffectSourceParameter::Create(&src_name).ok()?;
        let src_iface: IGraphicsEffectSource = src_param.cast().ok()?;

        let blur_effect: IGraphicsEffect = D2dGaussianBlurEffect {
            name: HSTRING::from("Blur"),
            std_dev,
            source: src_iface,
        }
        .into();

        // Preferred 2-stage Apple Glass graph: HostBackdrop -> GaussianBlur -> Vibrancy Saturation.
        let sat_source: IGraphicsEffectSource = blur_effect.cast().ok()?;
        let sat_effect: IGraphicsEffect = D2dSaturationEffect {
            name: HSTRING::from("Sat"),
            saturation,
            source: sat_source,
        }
        .into();

        let brush = self
            .compositor
            .CreateEffectFactory(&sat_effect)
            .and_then(|factory| factory.CreateBrush())
            .or_else(|_| {
                // Fallback: single-stage GaussianBlur without Saturation.
                self.compositor
                    .CreateEffectFactory(&blur_effect)
                    .and_then(|factory| factory.CreateBrush())
            });

        if let Ok(effect_brush) = brush
            && effect_brush
                .SetSourceParameter(&src_name, &self.host_backdrop)
                .is_ok()
            && let Ok(cb) = effect_brush.cast::<CompositionBrush>()
        {
            return Some(cb);
        }

        // Last-resort fallback: bare HostBackdropBrush.
        self.host_backdrop.cast::<CompositionBrush>().ok()
    }
}

impl Drop for GlassCompositor {
    fn drop(&mut self) {
        let class_name: Vec<u16> = format!("{GLASS_CLASS_NAME}\0").encode_utf16().collect();
        // SAFETY: Unregistering our glass window class on thread teardown.
        unsafe {
            let _ = UnregisterClassW(PCWSTR(class_name.as_ptr()), Some(self.hinstance));
        }
        if self.com_initialized {
            // SAFETY: Balancing `CoInitializeEx` on the same thread.
            unsafe {
                CoUninitialize();
            }
        }
    }
}

/// Hardware-accelerated rounded Apple-glass backdrop window paired 1-to-1 behind a layered overlay window.
pub(crate) struct GlassWindow {
    hwnd: HWND,
    _target: DesktopWindowTarget,
    visual: SpriteVisual,
    rounded_geom: CompositionRoundedRectangleGeometry,
    _clip: CompositionGeometricClip,
    shown: bool,
    last_blur_pct: u8,
    last_rect: OverlayRect,
    last_radius: f32,
}

impl GlassWindow {
    /// Synchronize blur strength, rounded geometry, fade opacity, and Z-order strictly behind
    /// `foreground_hwnd`. Hides cleanly when `blur_pct == 0` or `opacity <= 0.005`.
    pub(crate) fn sync(
        &mut self,
        comp: &GlassCompositor,
        foreground_hwnd: HWND,
        rect: OverlayRect,
        radius: f32,
        blur_pct: u8,
        opacity: f32,
    ) {
        let blur = blur_pct.min(100);
        let op = opacity.clamp(0.0, 1.0);
        if blur == 0 || op <= 0.005 || rect.width == 0 || rect.height == 0 {
            self.hide();
            return;
        }

        if self.last_blur_pct != blur
            && let Some(brush) = comp.build_glass_brush(blur)
        {
            let _ = self.visual.SetBrush(&brush);
            self.last_blur_pct = blur;
        }

        let size = Vector2 {
            X: rect.width as f32,
            Y: rect.height as f32,
        };
        let clamped_r = radius
            .min(rect.width as f32 * 0.5)
            .min(rect.height as f32 * 0.5)
            .max(0.0);
        if self.last_rect.width != rect.width
            || self.last_rect.height != rect.height
            || (self.last_radius - clamped_r).abs() > 0.1
        {
            let _ = self.visual.SetSize(size);
            let _ = self.rounded_geom.SetSize(size);
            let _ = self.rounded_geom.SetCornerRadius(Vector2 {
                X: clamped_r,
                Y: clamped_r,
            });
            self.last_radius = clamped_r;
        }
        let _ = self.visual.SetOpacity(op);

        // SAFETY: Positioning `self.hwnd` strictly one Z-order step below `foreground_hwnd`
        // without stealing focus.
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                Some(foreground_hwnd),
                rect.x,
                rect.y,
                rect.width as i32,
                rect.height as i32,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
        self.last_rect = rect;
        self.shown = true;
    }

    pub(crate) fn set_opacity(&mut self, opacity: f32) {
        if !self.shown {
            return;
        }
        let op = opacity.clamp(0.0, 1.0);
        if op <= 0.005 {
            self.hide();
        } else {
            let _ = self.visual.SetOpacity(op);
        }
    }

    pub(crate) fn hide(&mut self) {
        if self.shown {
            // SAFETY: Hiding glass backdrop without activating.
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
            self.shown = false;
        }
    }
}

impl Drop for GlassWindow {
    fn drop(&mut self) {
        // SAFETY: Destroying owned Win32 glass backdrop HWND on overlay thread exit.
        unsafe {
            let _ = DestroyWindow(self.hwnd);
        }
    }
}
