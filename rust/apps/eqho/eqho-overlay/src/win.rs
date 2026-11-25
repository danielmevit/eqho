//! Native Win32 layered iridescent pill, hover-reveal & pinnable under-pill action strip,
//! zero-focus-stealing searchable 99-language popup menu, and rounded translucent panel.

use std::cell::Cell;
use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr::{copy_nonoverlapping, null_mut};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;
use std::time::Instant;

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, ANTIALIASED_QUALITY, BI_RGB, BITMAPINFO, BITMAPINFOHEADER,
    BLENDFUNCTION, BeginPaint, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateCompatibleDC,
    CreateDIBSection, CreateFontW, CreateRoundRectRgn, CreateSolidBrush, DEFAULT_CHARSET,
    DEFAULT_PITCH, DIB_RGB_COLORS, DRAW_TEXT_FORMAT, DT_CALCRECT, DT_CENTER, DT_END_ELLIPSIS,
    DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, DT_WORDBREAK, DeleteDC, DeleteObject,
    DrawTextW, EndPaint, FF_SWISS, FillRect, GdiFlush, GetDC, GetDeviceCaps, GetMonitorInfoW,
    HBITMAP, HDC, HFONT, HGDIOBJ, HMONITOR, InvalidateRect, LOGPIXELSY, MONITOR_DEFAULTTOPRIMARY,
    MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint, MonitorFromWindow,
    OUT_DEFAULT_PRECIS, PAINTSTRUCT, ReleaseDC, SelectObject, SetBkMode, SetTextColor,
    SetWindowRgn, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetKeyState, GetKeyboardState, TME_LEAVE, TRACKMOUSEEVENT, ToUnicode,
    TrackMouseEvent, VK_BACK, VK_CAPITAL, VK_CONTROL, VK_DOWN, VK_ESCAPE, VK_LBUTTON, VK_LWIN,
    VK_MENU, VK_RBUTTON, VK_RETURN, VK_RWIN, VK_SHIFT, VK_UP,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW,
    GetClientRect, GetCursorPos, GetForegroundWindow, GetMessageW, GetSystemMetrics, HHOOK, HMENU,
    HWND_TOPMOST, IDC_ARROW, IDC_HAND, IDC_IBEAM, KBDLLHOOKSTRUCT, KillTimer, LWA_ALPHA,
    LoadCursorW, MA_NOACTIVATE, MSG, PostMessageW, PostQuitMessage, RegisterClassExW, SM_CXSCREEN,
    SM_CYSCREEN, SPI_GETWORKAREA, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SWP_SHOWWINDOW, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SetCursor,
    SetLayeredWindowAttributes, SetTimer, SetWindowPos, SetWindowsHookExW, ShowWindow,
    SystemParametersInfoW, TranslateMessage, ULW_ALPHA, UnhookWindowsHookEx, UnregisterClassW,
    UpdateLayeredWindow, WH_KEYBOARD_LL, WM_APP, WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN,
    WM_LBUTTONDOWN, WM_MOUSEACTIVATE, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL,
    WM_PAINT, WM_SETCURSOR, WM_SYSKEYDOWN, WM_TIMER, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{PCSTR, PCWSTR};

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

#[repr(C)]
struct AccentPolicy {
    accent_state: u32,
    accent_flags: u32,
    gradient_color: u32,
    animation_id: u32,
}

#[repr(C)]
struct WindowCompositionAttribData {
    attrib: u32,
    pv_data: *mut c_void,
    cb_data: usize,
}

const WCA_ACCENT_POLICY: u32 = 19;
const ACCENT_ENABLE_BLURBEHIND: u32 = 3;

/// Enable subtle Windows DWM blur-behind (`ACCENT_ENABLE_BLURBEHIND`) dynamically via `user32.dll`.
/// Gracefully no-ops if unavailable on older or headless environments.
fn apply_subtle_blur_behind(hwnd: HWND) {
    let user32_name: Vec<u16> = "user32.dll\0".encode_utf16().collect();
    // SAFETY: Resolving `SetWindowCompositionAttribute` from already-loaded `user32.dll`.
    let Ok(hmod) = (unsafe { GetModuleHandleW(PCWSTR(user32_name.as_ptr())) }) else {
        return;
    };
    let proc_name = b"SetWindowCompositionAttribute\0";
    // SAFETY: Looking up documented Win10/11 accent policy entrypoint on `user32.dll`.
    let Some(proc_ptr) = (unsafe { GetProcAddress(hmod, PCSTR(proc_name.as_ptr())) }) else {
        return;
    };
    type SetWcaFn = unsafe extern "system" fn(HWND, *mut WindowCompositionAttribData) -> i32;
    // SAFETY: Function pointer signature matches `SetWindowCompositionAttribute`.
    let set_wca: SetWcaFn = unsafe { std::mem::transmute(proc_ptr) };
    let mut policy = AccentPolicy {
        accent_state: ACCENT_ENABLE_BLURBEHIND,
        accent_flags: 0,
        gradient_color: 0,
        animation_id: 0,
    };
    let mut data = WindowCompositionAttribData {
        attrib: WCA_ACCENT_POLICY,
        pv_data: (&mut policy as *mut AccentPolicy).cast::<c_void>(),
        cb_data: std::mem::size_of::<AccentPolicy>(),
    };
    // SAFETY: `hwnd` and `data` are valid for the duration of the call.
    unsafe {
        let _ = set_wca(hwnd, &mut data);
    }
}

/// Clip DWM blur-behind cleanly to a rounded rectangle so no square blur halo bleeds past corners.
fn apply_rounded_region(hwnd: HWND, width: u32, height: u32, radius_px: i32) {
    let w = width as i32;
    let h = height as i32;
    if w <= 0 || h <= 0 {
        return;
    }
    let diam = (radius_px * 2).max(2);
    // SAFETY: `SetWindowRgn` takes ownership of the HRGN on success; delete it on failure.
    unsafe {
        let rgn = CreateRoundRectRgn(0, 0, w + 1, h + 1, diam, diam);
        if !rgn.is_invalid() && SetWindowRgn(hwnd, Some(rgn), true) == 0 {
            let _ = DeleteObject(rgn.into());
        }
    }
}

const WM_APP_CMD: u32 = WM_APP + 1;
const WM_APP_POPUP_VKEY: u32 = WM_APP + 2;
const WM_APP_POPUP_CHAR: u32 = WM_APP + 3;
const WM_MOUSELEAVE: u32 = 0x02A3;
const TIMER_ID: usize = 1;

thread_local! {
    /// Handle of the currently open language popup on this overlay thread (0 when closed).
    /// Read by the re-entrancy-safe `WH_KEYBOARD_LL` hook without touching `OverlayState`.
    static ACTIVE_POPUP_HWND: Cell<isize> = const { Cell::new(0) };
}

pub(crate) enum OverlayCmd {
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
    ShowRecovery(String),
    HideRecovery,
    Hide,
    Shutdown,
}

#[derive(Debug)]
pub(crate) struct WinOverlayClient {
    tx: mpsc::Sender<OverlayCmd>,
    pill_hwnd_raw: isize,
    thread: Option<JoinHandle<()>>,
}

impl WinOverlayClient {
    pub(crate) fn start(
        config: OverlayConfig,
        on_action: Option<OverlayActionCallback>,
        flag_provider: Option<FlagBitmapProvider>,
    ) -> Option<Self> {
        let (tx, rx) = mpsc::channel::<OverlayCmd>();
        let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<isize, String>>(1);

        let handle = std::thread::Builder::new()
            .name("eqho-overlay".to_owned())
            .spawn(move || run_overlay_thread(config, on_action, flag_provider, rx, ready_tx))
            .ok()?;

        match ready_rx.recv() {
            Ok(Ok(hwnd_raw)) => Some(Self {
                tx,
                pill_hwnd_raw: hwnd_raw,
                thread: Some(handle),
            }),
            _ => {
                let _ = handle.join();
                None
            }
        }
    }

    pub(crate) fn send(&self, cmd: OverlayCmd) {
        if self.tx.send(cmd).is_ok() {
            let hwnd = HWND(self.pill_hwnd_raw as *mut c_void);
            // SAFETY: `PostMessageW` is thread-safe and `pill_hwnd_raw` stays valid until `Shutdown`.
            unsafe {
                let _ = PostMessageW(Some(hwnd), WM_APP_CMD, WPARAM(0), LPARAM(0));
            }
        }
    }

    pub(crate) fn shutdown(&mut self) {
        if let Some(handle) = self.thread.take() {
            self.send(OverlayCmd::Shutdown);
            let _ = handle.join();
        }
    }
}

impl Drop for WinOverlayClient {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Reusable 32-bit top-down BGRA DIB backing a layered window or grayscale text mask.
struct LayeredDib {
    screen_dc: HDC,
    mem_dc: HDC,
    dib: Option<HBITMAP>,
    old_obj: Option<HGDIOBJ>,
    bits: *mut c_void,
    width: u32,
    height: u32,
    frame_buf: Vec<u8>,
}

impl LayeredDib {
    fn new() -> Self {
        // SAFETY: `GetDC(None)` acquires the screen DC; `CreateCompatibleDC` creates a memory DC.
        let screen_dc = unsafe { GetDC(None) };
        // SAFETY: `screen_dc` is valid for the lifetime of `LayeredDib`.
        let mem_dc = unsafe { CreateCompatibleDC(Some(screen_dc)) };
        Self {
            screen_dc,
            mem_dc,
            dib: None,
            old_obj: None,
            bits: null_mut(),
            width: 0,
            height: 0,
            frame_buf: Vec::new(),
        }
    }

    fn ensure_size(&mut self, w: u32, h: u32) -> bool {
        let w = w.max(1);
        let h = h.max(1);
        if self.dib.is_some() && self.width == w && self.height == h {
            return !self.bits.is_null();
        }

        self.release_dib();

        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w as i32,
                biHeight: -(h as i32), // top-down DIB
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };

        let mut bits: *mut c_void = null_mut();
        // SAFETY: `CreateDIBSection` allocates a 32-bit BGRA top-down DIB buffer in `bits`.
        let Ok(dib) = (unsafe {
            CreateDIBSection(Some(self.mem_dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0)
        }) else {
            return false;
        };
        // SAFETY: Selecting `dib` into `self.mem_dc`.
        let old = unsafe { SelectObject(self.mem_dc, dib.into()) };
        self.dib = Some(dib);
        self.old_obj = Some(old);
        self.bits = bits;
        self.width = w;
        self.height = h;
        self.frame_buf.resize((w as usize) * (h as usize) * 4, 0);
        !self.bits.is_null()
    }

    fn ensure_at_least(&mut self, w: u32, h: u32) -> bool {
        let target_w = self.width.max(w.max(1));
        let target_h = self.height.max(h.max(1));
        self.ensure_size(target_w, target_h)
    }

    fn present(&mut self, hwnd: HWND, rect: OverlayRect, pixels: &[u8], alpha: u8) {
        if !self.ensure_size(rect.width, rect.height) || self.bits.is_null() {
            return;
        }
        let copy_len = pixels.len().min(self.frame_buf.len());
        // SAFETY: `self.bits` is a valid `w * h * 4` buffer owned by `self.dib`.
        unsafe {
            copy_nonoverlapping(pixels.as_ptr(), self.bits as *mut u8, copy_len);
        }

        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: alpha,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let dst_pt = POINT {
            x: rect.x,
            y: rect.y,
        };
        let src_pt = POINT { x: 0, y: 0 };
        let size = SIZE {
            cx: rect.width as i32,
            cy: rect.height as i32,
        };

        // SAFETY: `hwnd`, `screen_dc`, `mem_dc`, and `blend` are all valid.
        unsafe {
            let _ = UpdateLayeredWindow(
                hwnd,
                Some(self.screen_dc),
                Some(&dst_pt),
                Some(&size),
                Some(self.mem_dc),
                Some(&src_pt),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
        }
    }

    /// Render antialiased Windows Segoe UI text onto `canvas` inside `rect` without corrupting
    /// the premultiplied alpha channel (draws white grayscale glyphs onto a black scratch DIB,
    /// flushes GDI, and composites glyph coverage onto `canvas`).
    fn draw_text_into_canvas(
        &mut self,
        canvas: &mut PremulCanvas,
        text: &str,
        rect: LocalRect,
        font: HFONT,
        color: Rgba,
        align: DRAW_TEXT_FORMAT,
    ) {
        if text.is_empty() || rect.width == 0 || rect.height == 0 {
            return;
        }
        let tw = rect.width;
        let th = rect.height;
        if !self.ensure_at_least(tw, th) || self.bits.is_null() {
            return;
        }

        let stride = (self.width as usize) * 4;
        // Clear the target sub-region to pure black before GDI text output.
        for row in 0..(th as usize) {
            // SAFETY: `row < th <= self.height` and `tw <= self.width`, so the row slice is in bounds.
            unsafe {
                let dst = (self.bits as *mut u8).add(row * stride);
                std::ptr::write_bytes(dst, 0, (tw as usize) * 4);
            }
        }

        let mut wide: Vec<u16> = text.encode_utf16().collect();
        let mut draw_rect = RECT {
            left: 0,
            top: 0,
            right: tw as i32,
            bottom: th as i32,
        };

        // SAFETY: Selecting `font`, drawing grayscale antialiased white text on black, restoring font,
        // and flushing GDI before reading `self.bits`.
        unsafe {
            let old = SelectObject(self.mem_dc, font.into());
            SetBkMode(self.mem_dc, TRANSPARENT);
            SetTextColor(self.mem_dc, COLORREF(0x00FF_FFFF));
            DrawTextW(
                self.mem_dc,
                &mut wide,
                &mut draw_rect,
                align | DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX | DT_END_ELLIPSIS,
            );
            let _ = SelectObject(self.mem_dc, old);
            let _ = GdiFlush();
        }

        for dy in 0..(th as i32) {
            let row_off = (dy as usize) * stride;
            for dx in 0..(tw as i32) {
                let px_off = row_off + (dx as usize) * 4;
                // SAFETY: `dx < tw <= self.width` and `dy < th <= self.height`.
                let cov_u8 = unsafe {
                    let p = (self.bits as *const u8).add(px_off);
                    let b = *p;
                    let g = *p.add(1);
                    let r = *p.add(2);
                    b.max(g).max(r)
                };
                if cov_u8 > 0 {
                    canvas.blend_pixel(rect.x + dx, rect.y + dy, color, f32::from(cov_u8) / 255.0);
                }
            }
        }
    }

    fn release_dib(&mut self) {
        if let Some(old) = self.old_obj.take() {
            // SAFETY: Restoring previous GDI object before deleting `dib`.
            unsafe {
                let _ = SelectObject(self.mem_dc, old);
            }
        }
        if let Some(dib) = self.dib.take() {
            // SAFETY: Deleting our owned `HBITMAP`.
            unsafe {
                let _ = DeleteObject(dib.into());
            }
        }
        self.bits = null_mut();
        self.width = 0;
        self.height = 0;
    }
}

impl Drop for LayeredDib {
    fn drop(&mut self) {
        self.release_dib();
        // SAFETY: Releasing owned `mem_dc` and `screen_dc`.
        unsafe {
            let _ = DeleteDC(self.mem_dc);
            let _ = ReleaseDC(None, self.screen_dc);
        }
    }
}

/// Cached grayscale-antialiased Segoe UI pixel fonts for the underbar and language popup.
struct UiFonts {
    bar_regular: HFONT,
    bar_semibold: HFONT,
    chip_semibold: HFONT,
    row_regular: HFONT,
    row_semibold: HFONT,
}

impl UiFonts {
    fn create() -> Self {
        Self {
            bar_regular: create_pixel_font(12, 400),
            bar_semibold: create_pixel_font(12, 600),
            chip_semibold: create_pixel_font(11, 600),
            row_regular: create_pixel_font(13, 400),
            row_semibold: create_pixel_font(13, 600),
        }
    }
}

impl Drop for UiFonts {
    fn drop(&mut self) {
        // SAFETY: Deleting owned GDI `HFONT` handles.
        unsafe {
            let _ = DeleteObject(self.bar_regular.into());
            let _ = DeleteObject(self.bar_semibold.into());
            let _ = DeleteObject(self.chip_semibold.into());
            let _ = DeleteObject(self.row_regular.into());
            let _ = DeleteObject(self.row_semibold.into());
        }
    }
}

fn create_pixel_font(pixel_height: i32, weight: i32) -> HFONT {
    let face: Vec<u16> = "Segoe UI\0".encode_utf16().collect();
    // SAFETY: Creating a GDI logical font with grayscale `ANTIALIASED_QUALITY` for layered DIB text.
    unsafe {
        CreateFontW(
            -pixel_height.abs(),
            0,
            0,
            0,
            weight,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            ANTIALIASED_QUALITY,
            (DEFAULT_PITCH.0 | FF_SWISS.0) as u32,
            PCWSTR(face.as_ptr()),
        )
    }
}

struct OverlayState {
    rx: mpsc::Receiver<OverlayCmd>,
    config: OverlayConfig,
    on_action: Option<OverlayActionCallback>,
    flag_provider: Option<FlagBitmapProvider>,
    flag_cache: HashMap<String, Option<(u32, u32, Vec<u8>)>>,
    pill_hwnd: HWND,
    panel_hwnd: HWND,
    bar_hwnd: HWND,
    popup_hwnd: HWND,
    recovery_hwnd: HWND,
    animator: PillAnimator,
    pill_dib: LayeredDib,
    bar_dib: LayeredDib,
    popup_dib: LayeredDib,
    recovery_dib: LayeredDib,
    text_mask: LayeredDib,
    bar_canvas: PremulCanvas,
    popup_canvas: PremulCanvas,
    strip_canvas: PremulCanvas,
    recovery_canvas: PremulCanvas,
    ui_fonts: UiFonts,
    visible: bool,
    timer_running: bool,
    panel_shown: bool,
    panel_text: String,
    active_work_area: WorkArea,
    pill_rect: OverlayRect,
    last_tick: Instant,
    session_start: Instant,
    font: Option<HFONT>,
    font_size: i32,

    // Under-pill bar & language menu state
    visibility: UnderbarVisibility,
    bar_shown: bool,
    bar_layout: UnderbarLayout,
    bar_placement: UnderbarPlacement,
    hovered_bar_control: Option<UnderbarControl>,
    bar_tracking_leave: bool,
    bar_hover_blend: f32,
    bar_dirty: bool,
    last_elapsed_secs: u64,
    last_bar_alpha: u8,

    // Searchable 99-language popup state
    popup_shown: bool,
    popup_rect: OverlayRect,
    popup_layout: LangPopupLayout,
    search_query: String,
    list_scroll: usize,
    recent_scroll: usize,
    hovered_popup_hit: Option<PopupHit>,
    keyboard_selected_row: Option<usize>,
    popup_tracking_leave: bool,
    popup_dirty: bool,
    prev_mouse_down: bool,
    kb_hook: Option<HHOOK>,

    // Floating failed-paste recovery card anchored at the pill position
    recovery_shown: bool,
    recovery_rect: OverlayRect,
    recovery_layout: RecoveryCardLayout,
    hovered_recovery_btn: Option<RecoveryCardButton>,
    recovery_tracking_leave: bool,
}

impl OverlayState {
    fn get_flag(&mut self, code: &str) -> Option<(u32, u32, Vec<u8>)> {
        let key = code.trim().to_ascii_lowercase();
        if key.is_empty() {
            return None;
        }
        if let Some(cached) = self.flag_cache.get(&key) {
            return cached.clone();
        }
        let fetched = self
            .flag_provider
            .as_ref()
            .and_then(|provider| provider(&key));
        self.flag_cache.insert(key, fetched.clone());
        fetched
    }

    fn emit_action(&self, action: OverlayAction) {
        if let Some(cb) = self.on_action.as_ref().map(Arc::clone) {
            cb(action);
        }
    }

    fn ensure_font(&mut self) -> Option<HFONT> {
        if self.font.is_some() && self.font_size == self.config.font_size {
            return self.font;
        }
        if let Some(old) = self.font.take() {
            // SAFETY: Deleting previously created `HFONT`.
            unsafe {
                let _ = DeleteObject(old.into());
            }
        }
        // SAFETY: Acquiring screen DC to read `LOGPIXELSY`.
        let hdc = unsafe { GetDC(None) };
        // SAFETY: `hdc` is valid.
        let dpi = unsafe { GetDeviceCaps(Some(hdc), LOGPIXELSY) }.max(96);
        // SAFETY: Releasing screen DC.
        unsafe {
            let _ = ReleaseDC(None, hdc);
        }
        let height = -((self.config.font_size * dpi) / 72);
        let face: Vec<u16> = "Segoe UI\0".encode_utf16().collect();
        // SAFETY: Creating a GDI logical font for panel text measurement and painting.
        let font = unsafe {
            CreateFontW(
                height,
                0,
                0,
                0,
                400, // FW_NORMAL
                0,
                0,
                0,
                DEFAULT_CHARSET,
                OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                CLEARTYPE_QUALITY,
                (DEFAULT_PITCH.0 | FF_SWISS.0) as u32,
                PCWSTR(face.as_ptr()),
            )
        };
        self.font_size = self.config.font_size;
        self.font = Some(font);
        self.font
    }

    fn start_timer(&mut self) {
        if !self.timer_running {
            // SAFETY: Starting `FRAME_MS` timer on `self.pill_hwnd`.
            unsafe {
                SetTimer(Some(self.pill_hwnd), TIMER_ID, FRAME_MS as u32, None);
            }
            self.timer_running = true;
            self.last_tick = Instant::now();
        }
    }

    fn stop_timer(&mut self) {
        if self.timer_running {
            // SAFETY: Stopping timer on `self.pill_hwnd`.
            unsafe {
                let _ = KillTimer(Some(self.pill_hwnd), TIMER_ID);
            }
            self.timer_running = false;
        }
    }

    fn install_kb_hook(&mut self) {
        if self.kb_hook.is_some() {
            return;
        }
        ACTIVE_POPUP_HWND.with(|cell| cell.set(self.popup_hwnd.0 as isize));
        // SAFETY: Installing a thread-scoped low-level keyboard hook while the language popup is
        // open so the user can type to filter languages with zero focus stealing. Unhooked on close.
        if let Ok(hook) =
            unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(ll_keyboard_proc), None, 0) }
        {
            self.kb_hook = Some(hook);
        } else {
            ACTIVE_POPUP_HWND.with(|cell| cell.set(0));
        }
    }

    fn uninstall_kb_hook(&mut self) {
        ACTIVE_POPUP_HWND.with(|cell| cell.set(0));
        if let Some(hook) = self.kb_hook.take() {
            // SAFETY: Unhooking our previously installed `WH_KEYBOARD_LL` hook.
            unsafe {
                let _ = UnhookWindowsHookEx(hook);
            }
        }
    }

    fn handle_commands(&mut self) -> bool {
        let mut should_quit = false;
        while let Ok(cmd) = self.rx.try_recv() {
            match cmd {
                OverlayCmd::Show(text) => self.cmd_show(text),
                OverlayCmd::UpdateText(text) => self.cmd_update_text(text),
                OverlayCmd::SetLevel(level) => self.animator.set_level(level),
                OverlayCmd::SetWaveform(loud, tone) => self.animator.feed_voice(loud, tone),
                OverlayCmd::SetFinishing(finishing) => self.cmd_set_finishing(finishing),
                OverlayCmd::UpdateSessionMeta {
                    language,
                    delivery_mode,
                    pinned,
                    recent_languages,
                } => self.cmd_update_session_meta(language, delivery_mode, pinned, recent_languages),
                OverlayCmd::UpdateConfig(cfg) => self.cmd_update_config(cfg),
                OverlayCmd::ShowRecovery(text) => self.cmd_show_recovery(text),
                OverlayCmd::HideRecovery => self.hide_recovery_now(),
                OverlayCmd::Hide => self.cmd_hide(),
                OverlayCmd::Shutdown => {
                    should_quit = true;
                    break;
                }
            }
        }
        should_quit
    }

    fn hide_panel_now(&mut self) {
        if self.panel_shown {
            // SAFETY: Hiding panel window without activating.
            unsafe {
                let _ = ShowWindow(self.panel_hwnd, SW_HIDE);
            }
            self.panel_shown = false;
        }
        self.panel_text.clear();
    }

    fn hide_recovery_now(&mut self) {
        if self.recovery_shown {
            // SAFETY: Hiding recovery card window without activating.
            unsafe {
                let _ = ShowWindow(self.recovery_hwnd, SW_HIDE);
            }
            self.recovery_shown = false;
            self.hovered_recovery_btn = None;
            self.recovery_tracking_leave = false;
        }
    }

    fn cmd_show_recovery(&mut self, _text: String) {
        if !self.config.enabled {
            return;
        }
        self.update_pill_geometry();
        self.recovery_layout = RecoveryCardLayout::compute();
        self.recovery_rect = underbar::calc_recovery_card_rect(
            self.pill_rect,
            self.recovery_layout.width,
            self.recovery_layout.height,
            self.active_work_area,
        );
        self.hovered_recovery_btn = None;
        self.recovery_shown = true;
        apply_rounded_region(
            self.recovery_hwnd,
            self.recovery_rect.width,
            self.recovery_rect.height,
            RECOVERY_CARD_RADIUS as i32,
        );
        self.render_recovery_frame();
        // SAFETY: Surfacing recovery action card without stealing keyboard focus.
        unsafe {
            let _ = ShowWindow(self.recovery_hwnd, SW_SHOWNOACTIVATE);
            let _ = SetWindowPos(
                self.recovery_hwnd,
                Some(HWND_TOPMOST),
                self.recovery_rect.x,
                self.recovery_rect.y,
                self.recovery_rect.width as i32,
                self.recovery_rect.height as i32,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
    }

    fn render_recovery_frame(&mut self) {
        let layout = self.recovery_layout;
        let theme = UnderbarTheme::for_theme(self.config.dark_theme);
        let dark = self.config.dark_theme;

        // Subtle neutral-warm translucent notification surface (no heavy brown fill, no left rail).
        let card_bg = if dark {
            Rgba::new(0x1B, 0x1A, 0x16, 224)
        } else {
            Rgba::new(0xFB, 0xF9, 0xF3, 232)
        };
        let card_border = if dark {
            Rgba::new(0x3D, 0x3A, 0x30, 230)
        } else {
            Rgba::new(0xD8, 0xD2, 0xC0, 235)
        };
        let dot_yellow = if dark {
            Rgba::rgb(0xD2, 0x99, 0x22)
        } else {
            Rgba::rgb(0x9A, 0x67, 0x00)
        };

        self.recovery_canvas.reset(layout.width, layout.height);
        self.recovery_canvas.fill_rounded_rect(
            LocalRect::new(0, 0, layout.width, layout.height),
            RECOVERY_CARD_RADIUS,
            card_bg,
            Some(card_border),
        );

        // Yellow notification dot (7x7 circle)
        self.recovery_canvas.fill_rounded_rect(
            LocalRect::new(layout.dot_cx - 3, layout.dot_cy - 3, 7, 7),
            3.5,
            dot_yellow,
            None,
        );

        // Top prompt
        self.text_mask.draw_text_into_canvas(
            &mut self.recovery_canvas,
            "Couldn't paste automatically",
            layout.title_rect,
            self.ui_fonts.bar_regular,
            theme.text_primary,
            DT_LEFT,
        );

        // 1. Show text button (outline/subtle fill)
        let show_bg = if self.hovered_recovery_btn == Some(RecoveryCardButton::ShowText) {
            theme.chip_hover_bg
        } else {
            theme.chip_bg
        };
        self.recovery_canvas.fill_rounded_rect(
            layout.show_text,
            6.0,
            show_bg,
            Some(theme.bar_border),
        );
        self.text_mask.draw_text_into_canvas(
            &mut self.recovery_canvas,
            "Show text",
            layout.show_text,
            self.ui_fonts.bar_regular,
            theme.text_primary,
            DT_CENTER,
        );

        // 2. Copy button (Eqho blue accent)
        let copy_bg = if self.hovered_recovery_btn == Some(RecoveryCardButton::Copy) {
            theme.done_hover_bg
        } else {
            theme.done_bg
        };
        self.recovery_canvas
            .fill_rounded_rect(layout.copy, 6.0, copy_bg, None);
        self.text_mask.draw_text_into_canvas(
            &mut self.recovery_canvas,
            "Copy",
            layout.copy,
            self.ui_fonts.bar_regular,
            theme.done_fg,
            DT_CENTER,
        );

        // 3. Discard button (muted outline)
        let disc_bg = if self.hovered_recovery_btn == Some(RecoveryCardButton::Discard) {
            theme.chip_hover_bg
        } else {
            theme.chip_bg
        };
        self.recovery_canvas.fill_rounded_rect(
            layout.discard,
            6.0,
            disc_bg,
            Some(theme.bar_border),
        );
        self.text_mask.draw_text_into_canvas(
            &mut self.recovery_canvas,
            "Discard",
            layout.discard,
            self.ui_fonts.bar_regular,
            theme.text_muted,
            DT_CENTER,
        );

        // Keep slightly translucent (~90% max) so subtle blur stays visible behind card.
        self.recovery_dib.present(
            self.recovery_hwnd,
            self.recovery_rect,
            &self.recovery_canvas.pixels,
            232,
        );
    }

    fn on_recovery_mouse_move(&mut self, lx: i32, ly: i32) {
        if !self.recovery_tracking_leave {
            let mut tme = TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: self.recovery_hwnd,
                dwHoverTime: 0,
            };
            // SAFETY: Requesting `WM_MOUSELEAVE` on `self.recovery_hwnd`.
            unsafe {
                let _ = TrackMouseEvent(&mut tme);
            }
            self.recovery_tracking_leave = true;
        }
        let next = self.recovery_layout.hit_test(lx, ly);
        if next != self.hovered_recovery_btn {
            self.hovered_recovery_btn = next;
            self.render_recovery_frame();
        }
    }

    fn on_recovery_mouse_leave(&mut self) {
        self.recovery_tracking_leave = false;
        if self.hovered_recovery_btn.is_some() {
            self.hovered_recovery_btn = None;
            self.render_recovery_frame();
        }
    }

    fn on_recovery_click(&mut self, lx: i32, ly: i32) {
        let Some(btn) = self.recovery_layout.hit_test(lx, ly) else {
            return;
        };
        self.hide_recovery_now();
        match btn {
            RecoveryCardButton::ShowText => self.emit_action(OverlayAction::RecoveryShowText),
            RecoveryCardButton::Copy => self.emit_action(OverlayAction::RecoveryCopy),
            RecoveryCardButton::Discard => self.emit_action(OverlayAction::RecoveryDiscard),
        }
    }

    fn hide_bar_now(&mut self) {
        self.close_lang_popup();
        if self.bar_shown {
            // SAFETY: Hiding underbar window without activating.
            unsafe {
                let _ = ShowWindow(self.bar_hwnd, SW_HIDE);
            }
            self.bar_shown = false;
            self.hovered_bar_control = None;
            self.bar_tracking_leave = false;
        }
    }

    fn close_lang_popup(&mut self) {
        self.uninstall_kb_hook();
        self.visibility.set_lang_menu_open(false);
        if self.popup_shown {
            // SAFETY: Hiding language popup window without activating.
            unsafe {
                let _ = ShowWindow(self.popup_hwnd, SW_HIDE);
            }
            self.popup_shown = false;
            self.hovered_popup_hit = None;
            self.keyboard_selected_row = None;
            self.popup_tracking_leave = false;
            self.search_query.clear();
            self.list_scroll = 0;
            self.recent_scroll = 0;
            self.bar_dirty = true;
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
        self.render_popup_frame();
        // SAFETY: Showing language popup with `SW_SHOWNOACTIVATE` + `HWND_TOPMOST` so focus never leaves target app.
        unsafe {
            let _ = ShowWindow(self.popup_hwnd, SW_SHOWNOACTIVATE);
            let _ = SetWindowPos(
                self.popup_hwnd,
                Some(HWND_TOPMOST),
                self.popup_rect.x,
                self.popup_rect.y,
                self.popup_rect.width as i32,
                self.popup_rect.height as i32,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
        self.popup_shown = true;
        self.prev_mouse_down = is_mouse_button_down();
        self.install_kb_hook();
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

    fn recompute_bar_geometry(&mut self) {
        self.bar_layout = UnderbarLayout::compute(false);
        self.bar_placement = underbar::calc_underbar_placement(
            self.pill_rect,
            self.bar_layout.width,
            self.active_work_area,
        );
        apply_rounded_region(
            self.bar_hwnd,
            self.bar_placement.bar_rect.width,
            self.bar_placement.bar_rect.height,
            UNDERBAR_RADIUS as i32,
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

    fn sync_bar_visibility(&mut self) {
        let should_show = self.visible && self.config.controls_enabled && self.visibility.visible;
        let changed = should_show != self.bar_shown;
        if should_show {
            if !self.bar_shown {
                self.bar_shown = true;
                self.bar_dirty = true;
                self.render_bar_frame();
                // SAFETY: Showing underbar without activating so target app keeps keyboard focus.
                unsafe {
                    let _ = ShowWindow(self.bar_hwnd, SW_SHOWNOACTIVATE);
                    let _ = SetWindowPos(
                        self.bar_hwnd,
                        Some(HWND_TOPMOST),
                        self.bar_placement.bar_rect.x,
                        self.bar_placement.bar_rect.y,
                        self.bar_placement.bar_rect.width as i32,
                        self.bar_placement.bar_rect.height as i32,
                        SWP_NOACTIVATE | SWP_SHOWWINDOW,
                    );
                }
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
            self.visibility
                .reset_for_session(self.config.controls_enabled, self.config.controls_pinned);
            self.render_pill_frame(0.0);
            // SAFETY: `SW_SHOWNOACTIVATE` + `HWND_TOPMOST` surfaces the pill on the active monitor
            // without stealing keyboard focus from the user's target app.
            unsafe {
                let _ = ShowWindow(self.pill_hwnd, SW_SHOWNOACTIVATE);
                let _ = SetWindowPos(
                    self.pill_hwnd,
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
                );
            }
            self.sync_bar_visibility();
            self.start_timer();
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

        let tailed = pure::tail_text(text.trim());
        self.panel_text = tailed;
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
                self.render_bar_frame();
            }
            if self.popup_shown {
                self.recompute_popup_layout();
                self.render_popup_frame();
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
                self.render_bar_frame();
            }
            if self.popup_shown {
                self.recompute_popup_layout();
                self.render_popup_frame();
            }
            if self.panel_shown {
                self.layout_and_show_panel();
            }
        }
    }

    fn cmd_hide(&mut self) {
        self.hide_panel_now();
        self.hide_bar_now();
        self.visibility.reset_for_hide();
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
        self.start_timer();
    }

    fn update_pill_geometry(&mut self) {
        let (pw, ph) = eqho_core::pillfx::pill_size(self.config.pill_scale);
        let area = query_work_area_for_mode(
            &self.config.monitor,
            self.pill_hwnd,
            self.panel_hwnd,
            self.bar_hwnd,
            self.popup_hwnd,
        );
        self.active_work_area = area;
        let (px, py) = underbar::calc_pill_position_with_controls(
            &self.config.position,
            pw,
            ph,
            area,
            self.config.controls_enabled,
        );
        self.pill_rect = OverlayRect {
            x: px,
            y: py,
            width: pw,
            height: ph,
        };
        let _ = self.pill_dib.ensure_size(pw, ph);
        self.recompute_bar_geometry();
    }

    fn layout_and_show_panel(&mut self) {
        let font = self.ensure_font();
        let mut wide: Vec<u16> = self.panel_text.encode_utf16().collect();
        let max_text_w = (PANEL_MAX_W as i32 - 2 * PANEL_PADDING_X).max(1);
        let mut calc = RECT {
            left: 0,
            top: 0,
            right: max_text_w,
            bottom: 0,
        };

        // SAFETY: Acquiring screen DC for `DrawTextW` text measurement.
        let hdc = unsafe { GetDC(None) };
        let old_font = font.map(|f| {
            // SAFETY: Selecting `font` into `hdc`.
            unsafe { SelectObject(hdc, f.into()) }
        });
        // SAFETY: `DrawTextW` with `DT_CALCRECT` computes wrapped bounding box in `calc`.
        unsafe {
            DrawTextW(
                hdc,
                &mut wide,
                &mut calc,
                DT_CALCRECT | DT_WORDBREAK | DT_LEFT | DT_NOPREFIX,
            );
        }
        if let Some(old) = old_font {
            // SAFETY: Restoring previous font.
            unsafe {
                let _ = SelectObject(hdc, old);
            }
        }
        // SAFETY: Releasing screen DC.
        unsafe {
            let _ = ReleaseDC(None, hdc);
        }

        let panel_w = ((calc.right - calc.left) + 2 * PANEL_PADDING_X)
            .max(self.pill_rect.width as i32)
            .min(PANEL_MAX_W as i32) as u32;
        let panel_h = ((calc.bottom - calc.top) + 2 * PANEL_PADDING_Y).max(1) as u32;
        let area = self.active_work_area;
        let active_bar = if self.bar_shown {
            Some(self.bar_placement.bar_rect)
        } else {
            None
        };
        let rect = underbar::calc_panel_rect_with_underbar(
            &self.config.position,
            self.pill_rect,
            active_bar,
            panel_w,
            panel_h,
            area,
        );

        let alpha = self.animator.effective_panel_alpha(&self.config);
        apply_rounded_region(self.panel_hwnd, rect.width, rect.height, 10);
        // SAFETY: Positioning, showing without activation, and updating layered alpha on `panel_hwnd`.
        unsafe {
            let _ = SetLayeredWindowAttributes(self.panel_hwnd, COLORREF(0), alpha, LWA_ALPHA);
            let _ = SetWindowPos(
                self.panel_hwnd,
                Some(HWND_TOPMOST),
                rect.x,
                rect.y,
                rect.width as i32,
                rect.height as i32,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            let _ = InvalidateRect(Some(self.panel_hwnd), None, true);
        }
        self.panel_shown = true;
    }

    fn on_timer(&mut self) {
        let now = Instant::now();
        let dt_dur = now.saturating_duration_since(self.last_tick);
        let dt = dt_dur.as_secs_f32();
        let dt_ms = dt_dur.as_millis() as u64;
        self.last_tick = now;

        self.render_pill_frame(dt);

        if self.visible && self.config.controls_enabled {
            // Poll cursor coordinates without removing `WS_EX_TRANSPARENT` from the pill window.
            let mut pt = POINT::default();
            // SAFETY: Reading current cursor coordinates in virtual screen space.
            let has_pt = unsafe { GetCursorPos(&mut pt) }.is_ok();

            // Close language popup cleanly on outside click without stealing focus.
            if self.popup_shown {
                let mouse_down = is_mouse_button_down();
                if mouse_down && !self.prev_mouse_down && has_pt {
                    let in_bar = underbar::point_in_overlay_rect(
                        pt.x,
                        pt.y,
                        self.bar_placement.bar_rect,
                    );
                    let in_popup =
                        underbar::point_in_overlay_rect(pt.x, pt.y, self.popup_rect);
                    if !in_bar && !in_popup {
                        self.close_lang_popup();
                    }
                }
                self.prev_mouse_down = mouse_down;
            }

            let in_zone = if has_pt {
                underbar::is_cursor_in_hover_zone(
                    pt.x,
                    pt.y,
                    self.pill_rect,
                    self.bar_placement.bridge_rect,
                    self.bar_shown.then_some(self.bar_placement.bar_rect),
                    self.popup_shown.then_some(self.popup_rect),
                )
            } else {
                false
            };

            self.visibility.tick(dt_ms, in_zone);
            self.sync_bar_visibility();

            if self.bar_shown {
                let interacting =
                    self.bar_tracking_leave || self.hovered_bar_control.is_some() || self.popup_shown;
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

        if !self.visible && self.animator.alpha_logical <= 0.0 {
            self.stop_timer();
            self.hide_bar_now();
            // SAFETY: Hiding overlay windows once fade-out completes.
            unsafe {
                let _ = ShowWindow(self.pill_hwnd, SW_HIDE);
                let _ = ShowWindow(self.panel_hwnd, SW_HIDE);
            }
            self.panel_shown = false;
            self.panel_text.clear();
        }
    }

    fn effective_bar_alpha(&self) -> u8 {
        let a = self.animator.alpha_logical.clamp(0.0, 1.0);
        let ratio = UNDERBAR_REST_ALPHA_RATIO
            + (UNDERBAR_HOVER_ALPHA_RATIO - UNDERBAR_REST_ALPHA_RATIO)
                * self.bar_hover_blend.clamp(0.0, 1.0);
        (a * ratio * 255.0).round().clamp(0.0, 240.0) as u8
    }

    fn render_pill_frame(&mut self, dt_s: f32) {
        let frame = self.animator.step(dt_s, self.config.show_level);
        let _ = self.animator.step_fade(dt_s);

        let w = self.pill_rect.width;
        let h = self.pill_rect.height;
        if !self.pill_dib.ensure_size(w, h) {
            return;
        }

        let light = if self.config.dark_theme { 0.0 } else { 1.0 };
        let rendered = eqho_core::pillfx::render_premul_bgra(
            w,
            h,
            frame.phase,
            frame.level,
            light,
            1.0,
            frame.wave,
        );
        self.pill_dib.present(
            self.pill_hwnd,
            self.pill_rect,
            &rendered,
            self.animator.effective_pill_alpha(&self.config),
        );

        if self.panel_shown {
            let panel_alpha = self.animator.effective_panel_alpha(&self.config);
            // SAFETY: Updating panel window alpha during fade-in/fade-out.
            unsafe {
                let _ = SetLayeredWindowAttributes(
                    self.panel_hwnd,
                    COLORREF(0),
                    panel_alpha,
                    LWA_ALPHA,
                );
            }
        }
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

        self.bar_canvas.reset(layout.width, layout.height);

        // Capsule background + crisp 1px border.
        self.bar_canvas.fill_rounded_rect(
            LocalRect::new(0, 0, layout.width, layout.height),
            UNDERBAR_RADIUS,
            theme.bar_bg,
            Some(theme.bar_border),
        );

        // 1. Cancel `✕` button.
        if hovered == Some(UnderbarControl::Cancel) {
            self.bar_canvas.fill_rounded_rect(
                layout.cancel,
                10.0,
                theme.hover_bg,
                None,
            );
        }
        let cancel_fg = if finishing {
            theme.text_muted.with_alpha(110)
        } else if hovered == Some(UnderbarControl::Cancel) {
            theme.text_primary
        } else {
            theme.text_muted
        };
        self.bar_canvas.draw_icon_x(
            (layout.cancel.x as f32) + (layout.cancel.width as f32) * 0.5,
            (layout.cancel.y as f32) + (layout.cancel.height as f32) * 0.5,
            3.4,
            1.35,
            cancel_fg,
        );

        // 2. Vertical 1px divider hairline.
        self.bar_canvas
            .fill_rect(LocalRect::new(layout.divider_x, 6, 1, 14), theme.divider);

        if finishing {
            // Keep capsule uncluttered with centered `"Writing…"` status while finalizing.
            let msg_rect = LocalRect::new(
                layout.timer.x,
                layout.timer.y,
                (layout.done.x - layout.timer.x - 4).max(1) as u32,
                layout.timer.height,
            );
            self.text_mask.draw_text_into_canvas(
                &mut self.bar_canvas,
                FINISHING_LABEL,
                msg_rect,
                self.ui_fonts.bar_regular,
                theme.text_primary,
                DT_CENTER,
            );
        } else {
            // 3. Live elapsed dictation timer (`M:SS`).
            self.text_mask.draw_text_into_canvas(
                &mut self.bar_canvas,
                &elapsed_text,
                layout.timer,
                self.ui_fonts.bar_regular,
                theme.text_muted,
                DT_CENTER,
            );

            // 4. Compact regular-weight language chip (`EN ▾`, no flag on the bar itself).
            let lang_active = self.popup_shown || hovered == Some(UnderbarControl::Language);
            if lang_active {
                self.bar_canvas
                    .fill_rounded_rect(layout.language, 9.0, theme.hover_bg, None);
            }
            let chevron_cx = (layout.language.right() - 7) as f32;
            let code_w = (layout.language.width.saturating_sub(14)).max(1);
            let code_rect = LocalRect::new(
                layout.language.x + 3,
                layout.language.y,
                code_w,
                layout.language.height,
            );
            self.text_mask.draw_text_into_canvas(
                &mut self.bar_canvas,
                &lang_code,
                code_rect,
                self.ui_fonts.bar_regular,
                if self.popup_shown {
                    theme.accent_text
                } else {
                    theme.text_primary
                },
                DT_CENTER,
            );
            self.bar_canvas.draw_icon_chevron(
                chevron_cx,
                (layout.language.y as f32) + (layout.language.height as f32) * 0.5,
                self.popup_shown,
                if self.popup_shown {
                    theme.accent_text
                } else {
                    theme.text_muted
                },
            );

            // 5. Delivery mode toggle chip (`Batch` / `Live`, regular weight).
            let is_live = underbar::normalize_delivery_mode(&self.config.delivery_mode) == "live";
            if hovered == Some(UnderbarControl::DeliveryMode) {
                self.bar_canvas
                    .fill_rounded_rect(layout.delivery_mode, 9.0, theme.hover_bg, None);
            }
            self.text_mask.draw_text_into_canvas(
                &mut self.bar_canvas,
                delivery_label,
                layout.delivery_mode,
                self.ui_fonts.bar_regular,
                if is_live {
                    theme.accent_text
                } else {
                    theme.text_primary
                },
                DT_CENTER,
            );

            // 6. Pushpin icon — pinned colors ONLY the pin vector bright blue, NEVER a blue circle!
            if hovered == Some(UnderbarControl::Pin) {
                self.bar_canvas
                    .fill_rounded_rect(layout.pin, 9.0, theme.hover_bg, None);
            }
            let pin_color = if pinned {
                theme.pin_active
            } else if hovered == Some(UnderbarControl::Pin) {
                theme.text_primary
            } else {
                theme.pin_inactive
            };
            self.bar_canvas.draw_icon_pushpin(
                (layout.pin.x as f32) + (layout.pin.width as f32) * 0.5,
                (layout.pin.y as f32) + (layout.pin.height as f32) * 0.5,
                pin_color,
            );
        }

        // 7. Done `✓` button (borderless Deep Navy Blend `#1A489B` dark / `#0969DA` light).
        let done_bg = if finishing {
            theme.done_bg.with_alpha(150)
        } else if hovered == Some(UnderbarControl::Done) {
            theme.done_hover_bg
        } else {
            theme.done_bg
        };
        self.bar_canvas
            .fill_rounded_rect(layout.done, 10.0, done_bg, None);
        self.bar_canvas.draw_icon_check(
            (layout.done.x as f32) + (layout.done.width as f32) * 0.5,
            (layout.done.y as f32) + (layout.done.height as f32) * 0.5,
            1.0,
            1.55,
            theme.done_fg,
        );

        let bar_alpha = self.effective_bar_alpha();
        self.bar_dib.present(
            self.bar_hwnd,
            self.bar_placement.bar_rect,
            &self.bar_canvas.pixels,
            bar_alpha,
        );
        self.last_bar_alpha = bar_alpha;
        self.bar_dirty = false;
    }

    fn render_popup_frame(&mut self) {
        let filtered = underbar::filter_languages(&self.search_query);
        let layout = self.popup_layout.clone();
        let theme = UnderbarTheme::for_theme(self.config.dark_theme);
        let active_code = self.config.language.trim().to_ascii_lowercase();
        let has_query = !self.search_query.is_empty();

        // Pre-fetch visible row flags and recent chip flags into cache before borrowing canvases.
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

        self.popup_canvas.reset(layout.width, layout.height);

        // Outer rounded card + 1px neutral border.
        self.popup_canvas.fill_rounded_rect(
            LocalRect::new(0, 0, layout.width, layout.height),
            10.0,
            theme.popup_bg,
            Some(theme.popup_border),
        );

        // Top search input bar (accent border indicates active search typing without focus steal).
        self.popup_canvas.fill_rounded_rect(
            layout.search_rect,
            6.0,
            theme.search_bg,
            Some(if has_query {
                theme.accent_text
            } else {
                theme.search_border
            }),
        );
        self.popup_canvas.draw_icon_search(
            (layout.search_rect.x + 11) as f32,
            (layout.search_rect.y as f32) + (layout.search_rect.height as f32) * 0.5,
            theme.text_muted,
        );
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
            (self.search_query.as_str(), theme.text_primary)
        } else {
            ("Search languages…", theme.text_muted)
        };
        self.text_mask.draw_text_into_canvas(
            &mut self.popup_canvas,
            search_str,
            query_rect,
            self.ui_fonts.row_regular,
            search_color,
            DT_LEFT,
        );

        if has_query {
            if self.hovered_popup_hit == Some(PopupHit::ClearSearch) {
                self.popup_canvas
                    .fill_rounded_rect(layout.clear_rect, 6.0, theme.hover_bg, None);
            }
            self.popup_canvas.draw_icon_x(
                (layout.clear_rect.x as f32) + (layout.clear_rect.width as f32) * 0.5,
                (layout.clear_rect.y as f32) + (layout.clear_rect.height as f32) * 0.5,
                3.0,
                1.25,
                theme.text_muted,
            );
        }

        // Scrollable 99-language list rows.
        if filtered.is_empty() {
            self.text_mask.draw_text_into_canvas(
                &mut self.popup_canvas,
                "No matching languages",
                layout.list_rect,
                self.ui_fonts.row_regular,
                theme.text_muted,
                DT_CENTER,
            );
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
                    self.popup_canvas
                        .fill_rounded_rect(row_rect, 6.0, theme.row_hover_bg, None);
                }

                // Flag bitmap or crisp 2-letter code fallback.
                let name_x =
                    if let Some((fw, fh, rgba)) = row_flags.get(vis_idx).and_then(|o| o.as_ref()) {
                        let flag_rect = LocalRect::new(row_rect.x + 6, row_rect.y + 8, 18, 12);
                        self.popup_canvas
                            .blit_flag_rgba(flag_rect, *fw, *fh, rgba, theme.flag_rim);
                        flag_rect.right() + 7
                    } else {
                        let badge_rect = LocalRect::new(row_rect.x + 6, row_rect.y + 6, 22, 16);
                        self.popup_canvas.fill_rounded_rect(
                            badge_rect,
                            4.0,
                            theme.chip_bg,
                            Some(theme.bar_border),
                        );
                        let code_upper = underbar::format_language_code(entry.code);
                        self.text_mask.draw_text_into_canvas(
                            &mut self.popup_canvas,
                            &code_upper,
                            badge_rect,
                            self.ui_fonts.chip_semibold,
                            theme.text_muted,
                            DT_CENTER,
                        );
                        badge_rect.right() + 6
                    };

                let check_space = if is_active { 20 } else { 6 };
                let name_w = (row_rect.right() - check_space - name_x).max(1) as u32;
                let name_rect = LocalRect::new(name_x, row_rect.y, name_w, row_rect.height);
                self.text_mask.draw_text_into_canvas(
                    &mut self.popup_canvas,
                    entry.name,
                    name_rect,
                    if is_active {
                        self.ui_fonts.row_semibold
                    } else {
                        self.ui_fonts.row_regular
                    },
                    if is_active {
                        theme.accent_text
                    } else {
                        theme.text_primary
                    },
                    DT_LEFT,
                );

                if is_active {
                    self.popup_canvas.draw_icon_check(
                        (row_rect.right() - 10) as f32,
                        (row_rect.y as f32) + (row_rect.height as f32) * 0.5,
                        0.95,
                        1.45,
                        theme.accent_text,
                    );
                }
            }

            // Minimalist vertical scrollbar thumb when list exceeds viewport.
            if needs_scrollbar {
                let track_h = layout.list_rect.height as f32;
                let ratio = (layout.visible_rows as f32) / (filtered.len() as f32);
                let thumb_h = (track_h * ratio).max(20.0).min(track_h);
                let max_scroll = layout.max_list_scroll().max(1) as f32;
                let scroll_frac = (self.list_scroll as f32) / max_scroll;
                let thumb_y = layout.list_rect.y + ((track_h - thumb_h) * scroll_frac).round() as i32;
                let thumb_rect = LocalRect::new(
                    layout.list_rect.right() - 4,
                    thumb_y,
                    3,
                    thumb_h.round() as u32,
                );
                self.popup_canvas
                    .fill_rounded_rect(thumb_rect, 1.5, theme.scrollbar_thumb, None);
            }
        }

        // Compact bottom recent-languages chip strip (hidden if < 2 recents, NO "RECENT" label).
        if layout.recent_strip.height > 0 {
            let strip = &layout.recent_strip;
            // Crisp 1px full-width hairline divider at top of strip.
            self.popup_canvas.fill_rect(
                LocalRect::new(
                    POPUP_PADDING_X,
                    layout.recent_strip_y,
                    strip.row_width,
                    1,
                ),
                theme.divider,
            );

            // Render chips onto dedicated strip canvas so peeking chips clip cleanly and receive
            // smooth right/left edge fade hints.
            let chip_row_h = underbar::RECENT_CHIP_HEIGHT;
            let chip_row_y = layout.recent_strip_y + 6;
            self.strip_canvas.reset(strip.row_width, chip_row_h);

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
                self.strip_canvas
                    .fill_rounded_rect(chip_rect, 6.0, bg, border);

                let code_upper = underbar::format_language_code(&chip.code);
                if let Some((fw, fh, rgba)) = recent_flags.get(idx).and_then(|o| o.as_ref()) {
                    // Center `[14×10 flag] [4px] [code]` cleanly inside the chip.
                    let content_w = 14 + 4 + 20;
                    let start_x = chip_rect.x + ((chip_rect.width as i32 - content_w) / 2).max(4);
                    let flag_rect = LocalRect::new(start_x, 6, 14, 10);
                    self.strip_canvas
                        .blit_flag_rgba(flag_rect, *fw, *fh, rgba, theme.flag_rim);
                    let label_rect = LocalRect::new(
                        flag_rect.right() + 4,
                        0,
                        (chip_rect.right() - (flag_rect.right() + 4) - 2).max(1) as u32,
                        chip_rect.height,
                    );
                    self.text_mask.draw_text_into_canvas(
                        &mut self.strip_canvas,
                        &code_upper,
                        label_rect,
                        self.ui_fonts.chip_semibold,
                        if is_active {
                            theme.accent_text
                        } else {
                            theme.text_primary
                        },
                        DT_LEFT,
                    );
                } else {
                    self.text_mask.draw_text_into_canvas(
                        &mut self.strip_canvas,
                        &code_upper,
                        chip_rect,
                        self.ui_fonts.chip_semibold,
                        if is_active {
                            theme.accent_text
                        } else {
                            theme.text_primary
                        },
                        DT_CENTER,
                    );
                }
            }

            let fade_l = if strip.show_left_fade {
                RECENT_FADE_WIDTH
            } else {
                0
            };
            let fade_r = if strip.show_right_fade {
                RECENT_FADE_WIDTH
            } else {
                0
            };
            self.popup_canvas.composite_with_edge_fade(
                &self.strip_canvas,
                POPUP_PADDING_X,
                chip_row_y,
                fade_l,
                fade_r,
            );
        }

        self.popup_dib
            .present(self.popup_hwnd, self.popup_rect, &self.popup_canvas.pixels, 255);
        self.popup_dirty = false;
    }

    fn on_bar_mouse_move(&mut self, lx: i32, ly: i32) {
        if !self.bar_tracking_leave {
            let mut tme = TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: self.bar_hwnd,
                dwHoverTime: 0,
            };
            // SAFETY: Requesting `WM_MOUSELEAVE` on `self.bar_hwnd`.
            unsafe {
                let _ = TrackMouseEvent(&mut tme);
            }
            self.bar_tracking_leave = true;
        }
        let next = self.bar_layout.hit_test(lx, ly);
        if next != self.hovered_bar_control {
            self.hovered_bar_control = next;
            self.render_bar_frame();
        }
    }

    fn on_bar_mouse_leave(&mut self) {
        self.bar_tracking_leave = false;
        if self.hovered_bar_control.is_some() {
            self.hovered_bar_control = None;
            self.render_bar_frame();
        }
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

    fn on_popup_mouse_move(&mut self, lx: i32, ly: i32) {
        if !self.popup_tracking_leave {
            let mut tme = TRACKMOUSEEVENT {
                cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                dwFlags: TME_LEAVE,
                hwndTrack: self.popup_hwnd,
                dwHoverTime: 0,
            };
            // SAFETY: Requesting `WM_MOUSELEAVE` on `self.popup_hwnd`.
            unsafe {
                let _ = TrackMouseEvent(&mut tme);
            }
            self.popup_tracking_leave = true;
        }
        let next = self
            .popup_layout
            .hit_test(lx, ly, !self.search_query.is_empty(), self.list_scroll);
        if next != self.hovered_popup_hit {
            self.hovered_popup_hit = next;
            self.keyboard_selected_row = None;
            self.render_popup_frame();
        }
    }

    fn on_popup_mouse_leave(&mut self) {
        self.popup_tracking_leave = false;
        if self.hovered_popup_hit.is_some() {
            self.hovered_popup_hit = None;
            self.render_popup_frame();
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

    fn on_popup_click(&mut self, lx: i32, ly: i32) {
        let Some(hit) = self
            .popup_layout
            .hit_test(lx, ly, !self.search_query.is_empty(), self.list_scroll)
        else {
            return;
        };
        match hit {
            PopupHit::SearchInput => {
                // Search is already live via `WH_KEYBOARD_LL`; no focus change needed.
            }
            PopupHit::ClearSearch => {
                self.search_query.clear();
                self.list_scroll = 0;
                self.keyboard_selected_row = None;
                self.recompute_popup_layout();
                self.render_popup_frame();
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

    fn on_popup_wheel(&mut self, screen_x: i32, screen_y: i32, delta: i16, horizontal: bool) {
        let lx = screen_x - self.popup_rect.x;
        let ly = screen_y - self.popup_rect.y;
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
                // Horizontal tilt wheel uses positive = right, vertical wheel uses down (<0) = right.
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

    fn on_popup_vkey(&mut self, vk: u16) {
        if !self.popup_shown {
            return;
        }
        if vk == VK_ESCAPE.0 {
            self.close_lang_popup();
            self.render_bar_frame();
        } else if vk == VK_BACK.0 {
            if self.search_query.pop().is_some() {
                self.list_scroll = 0;
                self.keyboard_selected_row = None;
                self.recompute_popup_layout();
                self.render_popup_frame();
            }
        } else if vk == VK_RETURN.0 {
            let filtered = underbar::filter_languages(&self.search_query);
            let idx = self.keyboard_selected_row.unwrap_or(0);
            if let Some(entry) = filtered.get(idx) {
                self.select_language_and_close(entry.code.to_owned());
            }
        } else if vk == VK_DOWN.0 || vk == VK_UP.0 {
            let filtered = underbar::filter_languages(&self.search_query);
            if filtered.is_empty() {
                return;
            }
            let max_idx = filtered.len() - 1;
            let next = match self.keyboard_selected_row {
                None => 0,
                Some(cur) if vk == VK_DOWN.0 => (cur + 1).min(max_idx),
                Some(cur) => cur.saturating_sub(1),
            };
            self.keyboard_selected_row = Some(next);
            if next < self.list_scroll {
                self.list_scroll = next;
            } else if next >= self.list_scroll + self.popup_layout.visible_rows {
                self.list_scroll = next + 1 - self.popup_layout.visible_rows;
            }
            self.render_popup_frame();
        }
    }

    fn on_popup_char(&mut self, ch: char) {
        if !self.popup_shown || ch.is_control() {
            return;
        }
        if self.search_query.chars().count() < 32 {
            self.search_query.push(ch);
            self.list_scroll = 0;
            self.keyboard_selected_row = Some(0);
            self.recompute_popup_layout();
            self.render_popup_frame();
        }
    }

    fn paint_panel(&mut self, hwnd: HWND) {
        let mut ps = PAINTSTRUCT::default();
        // SAFETY: `BeginPaint` / `EndPaint` bracket GDI painting in `WM_PAINT`.
        let hdc = unsafe { BeginPaint(hwnd, &mut ps) };

        let mut client = RECT::default();
        // SAFETY: Reading client rect of `hwnd`.
        unsafe {
            let _ = GetClientRect(hwnd, &mut client);
        }

        // Dark: #151517 bg / #F5F6F8 fg; Light: #FFFFFF bg / #1A1C1F fg (BGR COLORREF).
        let (bg_color, fg_color) = if self.config.dark_theme {
            (COLORREF(0x0017_1515), COLORREF(0x00F8_F6F5))
        } else {
            (COLORREF(0x00FF_FFFF), COLORREF(0x001F_1C1A))
        };

        // SAFETY: Creating solid brush, filling client area, and deleting brush.
        unsafe {
            let brush = CreateSolidBrush(bg_color);
            let _ = FillRect(hdc, &client, brush);
            let _ = DeleteObject(brush.into());
        }

        if !self.panel_text.is_empty() {
            let font = self.ensure_font();
            let old_font = font.map(|f| {
                // SAFETY: Selecting font into `hdc`.
                unsafe { SelectObject(hdc, f.into()) }
            });
            let mut text_rect = RECT {
                left: client.left + PANEL_PADDING_X,
                top: client.top + PANEL_PADDING_Y,
                right: (client.right - PANEL_PADDING_X).max(client.left + PANEL_PADDING_X),
                bottom: (client.bottom - PANEL_PADDING_Y).max(client.top + PANEL_PADDING_Y),
            };
            let mut wide: Vec<u16> = self.panel_text.encode_utf16().collect();
            // SAFETY: Drawing wrapped text inside padded `text_rect`.
            unsafe {
                SetBkMode(hdc, TRANSPARENT);
                SetTextColor(hdc, fg_color);
                DrawTextW(
                    hdc,
                    &mut wide,
                    &mut text_rect,
                    DT_WORDBREAK | DT_LEFT | DT_NOPREFIX,
                );
            }
            if let Some(old) = old_font {
                // SAFETY: Restoring previous font.
                unsafe {
                    let _ = SelectObject(hdc, old);
                }
            }
        }

        // SAFETY: Completing `BeginPaint`.
        unsafe {
            let _ = EndPaint(hwnd, &ps);
        }
    }
}

impl Drop for OverlayState {
    fn drop(&mut self) {
        self.stop_timer();
        self.uninstall_kb_hook();
        if let Some(font) = self.font.take() {
            // SAFETY: Deleting owned GDI font.
            unsafe {
                let _ = DeleteObject(font.into());
            }
        }
    }
}

fn is_mouse_button_down() -> bool {
    // SAFETY: `GetAsyncKeyState` is a pure Win32 state query.
    unsafe {
        (GetAsyncKeyState(i32::from(VK_LBUTTON.0)) < 0)
            || (GetAsyncKeyState(i32::from(VK_RBUTTON.0)) < 0)
    }
}

fn monitor_work_area(hmon: HMONITOR) -> Option<WorkArea> {
    if hmon.is_invalid() {
        return None;
    }
    let mut info = MONITORINFO {
        cbSize: std::mem::size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    // SAFETY: `info.cbSize` is initialized and `hmon` is a valid monitor handle.
    if unsafe { GetMonitorInfoW(hmon, &mut info) }.as_bool() {
        let r = info.rcWork;
        if r.right > r.left && r.bottom > r.top {
            return Some(WorkArea {
                left: r.left,
                top: r.top,
                right: r.right,
                bottom: r.bottom,
            });
        }
    }
    None
}

fn query_work_area_for_mode(
    mode: &str,
    own_pill: HWND,
    own_panel: HWND,
    own_bar: HWND,
    own_popup: HWND,
) -> WorkArea {
    match mode {
        "primary" => {
            // SAFETY: Queries the primary display handle.
            let hmon = unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };
            if let Some(area) = monitor_work_area(hmon) {
                return area;
            }
        }
        "cursor" => {
            let mut pt = POINT::default();
            // SAFETY: Writes current cursor screen coordinates into `pt`.
            if unsafe { GetCursorPos(&mut pt) }.is_ok() {
                // SAFETY: Resolves the monitor nearest to the cursor position.
                let hmon = unsafe { MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST) };
                if let Some(area) = monitor_work_area(hmon) {
                    return area;
                }
            }
        }
        _ => {
            // SAFETY: Queries the current foreground window handle.
            let fg = unsafe { GetForegroundWindow() };
            if !fg.is_invalid()
                && !fg.0.is_null()
                && fg != own_pill
                && fg != own_panel
                && fg != own_bar
                && fg != own_popup
            {
                // SAFETY: Resolves the monitor hosting `fg`.
                let hmon = unsafe { MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST) };
                if let Some(area) = monitor_work_area(hmon) {
                    return area;
                }
            }
            let mut pt = POINT::default();
            // SAFETY: Fallback to cursor monitor.
            if unsafe { GetCursorPos(&mut pt) }.is_ok() {
                // SAFETY: Resolves the monitor nearest to the cursor position.
                let hmon = unsafe { MonitorFromPoint(pt, MONITOR_DEFAULTTONEAREST) };
                if let Some(area) = monitor_work_area(hmon) {
                    return area;
                }
            }
        }
    }
    query_primary_work_area()
}

fn query_primary_work_area() -> WorkArea {
    let mut rect = RECT::default();
    // SAFETY: `SPI_GETWORKAREA` writes the primary monitor work area into `rect`.
    let ok = unsafe {
        SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some((&mut rect as *mut RECT).cast::<c_void>()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    if ok.is_ok() && rect.right > rect.left && rect.bottom > rect.top {
        return WorkArea {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
        };
    }
    // SAFETY: Fallback to primary screen metrics.
    let sw = unsafe { GetSystemMetrics(SM_CXSCREEN) }.max(1);
    // SAFETY: Fallback to primary screen metrics.
    let sh = unsafe { GetSystemMetrics(SM_CYSCREEN) }.max(1);
    WorkArea {
        left: 0,
        top: 0,
        right: sw,
        bottom: sh,
    }
}

fn run_overlay_thread(
    config: OverlayConfig,
    on_action: Option<OverlayActionCallback>,
    flag_provider: Option<FlagBitmapProvider>,
    rx: mpsc::Receiver<OverlayCmd>,
    ready_tx: mpsc::SyncSender<Result<isize, String>>,
) {
    // SAFETY: `GetModuleHandleW(None)` returns the current executable's HMODULE.
    let Ok(hinstance) = (unsafe { GetModuleHandleW(None) }) else {
        let _ = ready_tx.send(Err("GetModuleHandleW failed".to_owned()));
        return;
    };

    let pill_class: Vec<u16> = "EqhoOverlayPill\0".encode_utf16().collect();
    let panel_class: Vec<u16> = "EqhoOverlayPanel\0".encode_utf16().collect();
    let bar_class: Vec<u16> = "EqhoOverlayUnderbar\0".encode_utf16().collect();
    let popup_class: Vec<u16> = "EqhoOverlayLangPopup\0".encode_utf16().collect();
    let recovery_class: Vec<u16> = "EqhoOverlayRecovery\0".encode_utf16().collect();

    type WndProcFn = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;
    let classes: [(&[u16], WndProcFn); 5] = [
        (&pill_class, pill_wnd_proc),
        (&panel_class, panel_wnd_proc),
        (&bar_class, bar_wnd_proc),
        (&popup_class, popup_wnd_proc),
        (&recovery_class, recovery_wnd_proc),
    ];

    let mut registered: Vec<&[u16]> = Vec::with_capacity(5);
    for (name, proc_fn) in classes {
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(proc_fn),
            hInstance: hinstance.into(),
            lpszClassName: PCWSTR(name.as_ptr()),
            ..Default::default()
        };
        // SAFETY: Registering Win32 window class owned by this thread.
        if unsafe { RegisterClassExW(&wc) } == 0 {
            for prev in registered {
                // SAFETY: Unregistering previously registered classes on failure.
                unsafe {
                    let _ = UnregisterClassW(PCWSTR(prev.as_ptr()), Some(hinstance.into()));
                }
            }
            let _ = ready_tx.send(Err("RegisterClassExW failed".to_owned()));
            return;
        }
        registered.push(name);
    }

    let cleanup_classes = |hinst| {
        for name in [
            &pill_class,
            &panel_class,
            &bar_class,
            &popup_class,
            &recovery_class,
        ] {
            // SAFETY: Unregistering classes created on this thread.
            unsafe {
                let _ = UnregisterClassW(PCWSTR(name.as_ptr()), Some(hinst));
            }
        }
    };

    let (pw, ph) = eqho_core::pillfx::pill_size(config.pill_scale);
    let area = query_primary_work_area();
    let (px, py) = underbar::calc_pill_position_with_controls(
        &config.position,
        pw,
        ph,
        area,
        config.controls_enabled,
    );
    let pill_rect = OverlayRect {
        x: px,
        y: py,
        width: pw,
        height: ph,
    };
    let bar_layout = UnderbarLayout::compute(false);
    let bar_placement = underbar::calc_underbar_placement(pill_rect, bar_layout.width, area);
    let popup_layout = LangPopupLayout::compute(99, &config.recent_languages, 0);
    let popup_rect = underbar::calc_lang_popup_rect(
        bar_placement.bar_rect,
        bar_layout.language,
        popup_layout.width,
        popup_layout.height,
        area,
    );
    let recovery_layout = RecoveryCardLayout::compute();
    let recovery_rect = underbar::calc_recovery_card_rect(
        pill_rect,
        recovery_layout.width,
        recovery_layout.height,
        area,
    );

    let pill_ex =
        WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT;
    let interactive_ex = WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE;

    // SAFETY: Creating borderless non-activating layered pill window.
    let Ok(pill_hwnd) = (unsafe {
        CreateWindowExW(
            pill_ex,
            PCWSTR(pill_class.as_ptr()),
            PCWSTR(pill_class.as_ptr()),
            WS_POPUP,
            px,
            py,
            pw as i32,
            ph as i32,
            None,
            None,
            Some(hinstance.into()),
            None,
        )
    }) else {
        cleanup_classes(hinstance.into());
        let _ = ready_tx.send(Err("CreateWindowExW pill failed".to_owned()));
        return;
    };

    // SAFETY: Creating borderless non-activating layered panel window.
    let Ok(panel_hwnd) = (unsafe {
        CreateWindowExW(
            interactive_ex,
            PCWSTR(panel_class.as_ptr()),
            PCWSTR(panel_class.as_ptr()),
            WS_POPUP,
            px,
            py,
            pw as i32,
            40,
            None,
            None,
            Some(hinstance.into()),
            None,
        )
    }) else {
        // SAFETY: Cleaning up created windows on failure.
        unsafe {
            let _ = DestroyWindow(pill_hwnd);
        }
        cleanup_classes(hinstance.into());
        let _ = ready_tx.send(Err("CreateWindowExW panel failed".to_owned()));
        return;
    };

    // SAFETY: Creating borderless non-activating layered underbar window.
    let Ok(bar_hwnd) = (unsafe {
        CreateWindowExW(
            interactive_ex,
            PCWSTR(bar_class.as_ptr()),
            PCWSTR(bar_class.as_ptr()),
            WS_POPUP,
            bar_placement.bar_rect.x,
            bar_placement.bar_rect.y,
            bar_placement.bar_rect.width as i32,
            bar_placement.bar_rect.height as i32,
            None,
            None,
            Some(hinstance.into()),
            None,
        )
    }) else {
        // SAFETY: Cleaning up created windows on failure.
        unsafe {
            let _ = DestroyWindow(panel_hwnd);
            let _ = DestroyWindow(pill_hwnd);
        }
        cleanup_classes(hinstance.into());
        let _ = ready_tx.send(Err("CreateWindowExW bar failed".to_owned()));
        return;
    };

    // SAFETY: Creating borderless non-activating layered language popup window.
    let Ok(popup_hwnd) = (unsafe {
        CreateWindowExW(
            interactive_ex,
            PCWSTR(popup_class.as_ptr()),
            PCWSTR(popup_class.as_ptr()),
            WS_POPUP,
            popup_rect.x,
            popup_rect.y,
            popup_rect.width as i32,
            popup_rect.height as i32,
            None,
            None,
            Some(hinstance.into()),
            None,
        )
    }) else {
        // SAFETY: Cleaning up created windows on failure.
        unsafe {
            let _ = DestroyWindow(bar_hwnd);
            let _ = DestroyWindow(panel_hwnd);
            let _ = DestroyWindow(pill_hwnd);
        }
        cleanup_classes(hinstance.into());
        let _ = ready_tx.send(Err("CreateWindowExW popup failed".to_owned()));
        return;
    };

    // SAFETY: Creating borderless non-activating layered recovery action card window.
    let Ok(recovery_hwnd) = (unsafe {
        CreateWindowExW(
            interactive_ex,
            PCWSTR(recovery_class.as_ptr()),
            PCWSTR(recovery_class.as_ptr()),
            WS_POPUP,
            recovery_rect.x,
            recovery_rect.y,
            recovery_rect.width as i32,
            recovery_rect.height as i32,
            None,
            None,
            Some(hinstance.into()),
            None,
        )
    }) else {
        // SAFETY: Cleaning up created windows on failure.
        unsafe {
            let _ = DestroyWindow(popup_hwnd);
            let _ = DestroyWindow(bar_hwnd);
            let _ = DestroyWindow(panel_hwnd);
            let _ = DestroyWindow(pill_hwnd);
        }
        cleanup_classes(hinstance.into());
        let _ = ready_tx.send(Err("CreateWindowExW recovery failed".to_owned()));
        return;
    };

    // Best-effort Windows 11 rounded corners + subtle frosted blur behind panel, underbar, and recovery card.
    let corner_pref = DWMWCP_ROUND.0;
    // SAFETY: `corner_pref` is a valid `i32` for `DWMWA_WINDOW_CORNER_PREFERENCE`.
    unsafe {
        let _ = DwmSetWindowAttribute(
            panel_hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            (&corner_pref as *const i32).cast::<c_void>(),
            std::mem::size_of::<i32>() as u32,
        );
    }
    apply_subtle_blur_behind(panel_hwnd);
    apply_subtle_blur_behind(bar_hwnd);
    apply_subtle_blur_behind(recovery_hwnd);
    apply_rounded_region(
        bar_hwnd,
        bar_placement.bar_rect.width,
        bar_placement.bar_rect.height,
        UNDERBAR_RADIUS as i32,
    );
    apply_rounded_region(
        recovery_hwnd,
        recovery_rect.width,
        recovery_rect.height,
        RECOVERY_CARD_RADIUS as i32,
    );

    let visibility = UnderbarVisibility::new(config.controls_enabled, config.controls_pinned);
    let now = Instant::now();
    let mut state = Box::new(OverlayState {
        rx,
        config,
        on_action,
        flag_provider,
        flag_cache: HashMap::new(),
        pill_hwnd,
        panel_hwnd,
        bar_hwnd,
        popup_hwnd,
        recovery_hwnd,
        animator: PillAnimator::new(),
        pill_dib: LayeredDib::new(),
        bar_dib: LayeredDib::new(),
        popup_dib: LayeredDib::new(),
        recovery_dib: LayeredDib::new(),
        text_mask: LayeredDib::new(),
        bar_canvas: PremulCanvas::new(bar_layout.width, bar_layout.height),
        popup_canvas: PremulCanvas::new(popup_layout.width, popup_layout.height),
        strip_canvas: PremulCanvas::new(popup_layout.width, underbar::RECENT_CHIP_HEIGHT),
        recovery_canvas: PremulCanvas::new(recovery_layout.width, recovery_layout.height),
        ui_fonts: UiFonts::create(),
        visible: false,
        timer_running: false,
        panel_shown: false,
        panel_text: String::new(),
        active_work_area: area,
        pill_rect,
        last_tick: now,
        session_start: now,
        font: None,
        font_size: 0,
        visibility,
        bar_shown: false,
        bar_layout,
        bar_placement,
        hovered_bar_control: None,
        bar_tracking_leave: false,
        bar_hover_blend: 0.0,
        bar_dirty: true,
        last_elapsed_secs: 0,
        last_bar_alpha: 0,
        popup_shown: false,
        popup_rect,
        popup_layout,
        search_query: String::new(),
        list_scroll: 0,
        recent_scroll: 0,
        hovered_popup_hit: None,
        keyboard_selected_row: None,
        popup_tracking_leave: false,
        popup_dirty: true,
        prev_mouse_down: false,
        kb_hook: None,
        recovery_shown: false,
        recovery_rect,
        recovery_layout,
        hovered_recovery_btn: None,
        recovery_tracking_leave: false,
    });
    let state_ptr = (&mut *state) as *mut OverlayState;
    set_window_state(pill_hwnd, state_ptr);
    set_window_state(panel_hwnd, state_ptr);
    set_window_state(bar_hwnd, state_ptr);
    set_window_state(popup_hwnd, state_ptr);
    set_window_state(recovery_hwnd, state_ptr);

    let _ = ready_tx.send(Ok(pill_hwnd.0 as isize));

    let mut msg = MSG::default();
    // SAFETY: Standard Win32 message loop on the dedicated overlay thread.
    unsafe {
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    set_window_state(pill_hwnd, null_mut());
    set_window_state(panel_hwnd, null_mut());
    set_window_state(bar_hwnd, null_mut());
    set_window_state(popup_hwnd, null_mut());
    set_window_state(recovery_hwnd, null_mut());
    drop(state);
    cleanup_classes(hinstance.into());
}

fn set_window_state(hwnd: HWND, ptr: *mut OverlayState) {
    use windows::Win32::UI::WindowsAndMessaging::{GWLP_USERDATA, SetWindowLongPtrW};
    // SAFETY: Storing thread-local `OverlayState` pointer in `GWLP_USERDATA`.
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, ptr as isize);
    }
}

unsafe fn get_window_state(hwnd: HWND) -> Option<&'static mut OverlayState> {
    use windows::Win32::UI::WindowsAndMessaging::{GWLP_USERDATA, GetWindowLongPtrW};
    // SAFETY: Reading `GWLP_USERDATA` set by `set_window_state` on the same thread.
    let raw = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) };
    if raw == 0 {
        None
    } else {
        // SAFETY: Pointer remains valid until `PostQuitMessage` exits the message loop.
        Some(unsafe { &mut *(raw as *mut OverlayState) })
    }
}

#[inline]
fn lparam_xy(lparam: LPARAM) -> (i32, i32) {
    let x = i32::from((lparam.0 & 0xFFFF) as i16);
    let y = i32::from(((lparam.0 >> 16) & 0xFFFF) as i16);
    (x, y)
}

#[inline]
fn wparam_wheel_delta(wparam: WPARAM) -> i16 {
    ((wparam.0 >> 16) & 0xFFFF) as i16
}

unsafe extern "system" fn ll_keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && (wparam.0 as u32 == WM_KEYDOWN || wparam.0 as u32 == WM_SYSKEYDOWN) {
        let popup_raw = ACTIVE_POPUP_HWND.with(Cell::get);
        if popup_raw != 0 {
            // SAFETY: `lparam` points to a valid `KBDLLHOOKSTRUCT` when `code >= 0`.
            let kb = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
            // Never intercept global modifier hotkeys (e.g. Alt+Q, Ctrl+C, Win+...).
            // SAFETY: Querying modifier key states via `GetAsyncKeyState`.
            let modifier_held = unsafe {
                GetAsyncKeyState(i32::from(VK_CONTROL.0)) < 0
                    || GetAsyncKeyState(i32::from(VK_MENU.0)) < 0
                    || GetAsyncKeyState(i32::from(VK_LWIN.0)) < 0
                    || GetAsyncKeyState(i32::from(VK_RWIN.0)) < 0
            };
            if !modifier_held {
                let vk = kb.vkCode as u16;
                let popup_hwnd = HWND(popup_raw as *mut c_void);
                if vk == VK_ESCAPE.0
                    || vk == VK_BACK.0
                    || vk == VK_RETURN.0
                    || vk == VK_UP.0
                    || vk == VK_DOWN.0
                {
                    // SAFETY: Posting key event to our thread's popup window without touching state.
                    unsafe {
                        let _ = PostMessageW(
                            Some(popup_hwnd),
                            WM_APP_POPUP_VKEY,
                            WPARAM(usize::from(vk)),
                            LPARAM(0),
                        );
                    }
                    return LRESULT(1);
                }

                let mut key_state = [0u8; 256];
                // SAFETY: Reading keyboard state plus live shift/caps state since overlay never takes focus.
                let has_state = unsafe {
                    let ok = GetKeyboardState(&mut key_state).is_ok();
                    if ok {
                        if GetAsyncKeyState(i32::from(VK_SHIFT.0)) < 0 {
                            key_state[usize::from(VK_SHIFT.0)] = 0x80;
                        }
                        if (GetKeyState(i32::from(VK_CAPITAL.0)) & 1) != 0 {
                            key_state[usize::from(VK_CAPITAL.0)] = 0x01;
                        }
                    }
                    ok
                };
                if has_state {
                    let mut utf16 = [0u16; 4];
                    // SAFETY: Translating virtual key with flag `0x4` (preserve dead-key state).
                    let written = unsafe {
                        ToUnicode(kb.vkCode, kb.scanCode, Some(&key_state), &mut utf16, 0x4)
                    };
                    if written > 0 {
                        let slice = &utf16[..(written as usize).min(utf16.len())];
                        if let Some(Ok(ch)) = char::decode_utf16(slice.iter().copied()).next()
                            && !ch.is_control()
                        {
                            // SAFETY: Posting typed character to our popup window without stealing focus.
                            unsafe {
                                let _ = PostMessageW(
                                    Some(popup_hwnd),
                                    WM_APP_POPUP_CHAR,
                                    WPARAM(ch as usize),
                                    LPARAM(0),
                                );
                            }
                            return LRESULT(1);
                        }
                    }
                }
            }
        }
    }
    // SAFETY: Passing unhandled keyboard events to the next hook in the chain.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

unsafe extern "system" fn pill_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_APP_CMD => {
            let mut destroy_handles: Option<(HWND, HWND, HWND, HWND, HWND)> = None;
            // SAFETY: `hwnd` belongs to the overlay thread.
            if let Some(state) = unsafe { get_window_state(hwnd) }
                && state.handle_commands()
            {
                state.stop_timer();
                state.uninstall_kb_hook();
                destroy_handles = Some((
                    state.recovery_hwnd,
                    state.popup_hwnd,
                    state.bar_hwnd,
                    state.panel_hwnd,
                    hwnd,
                ));
            }
            if let Some((recovery, popup, bar, panel, pill)) = destroy_handles {
                // Detach state pointers BEFORE `DestroyWindow` so re-entrant messages
                // cannot alias `OverlayState`.
                set_window_state(recovery, null_mut());
                set_window_state(popup, null_mut());
                set_window_state(bar, null_mut());
                set_window_state(panel, null_mut());
                set_window_state(pill, null_mut());
                // SAFETY: Destroying all owned overlay windows and exiting message loop on shutdown.
                unsafe {
                    let _ = DestroyWindow(recovery);
                    let _ = DestroyWindow(popup);
                    let _ = DestroyWindow(bar);
                    let _ = DestroyWindow(panel);
                    let _ = DestroyWindow(pill);
                    PostQuitMessage(0);
                }
            }
            LRESULT(0)
        }
        WM_TIMER => {
            // SAFETY: `hwnd` belongs to the overlay thread.
            if let Some(state) = unsafe { get_window_state(hwnd) } {
                state.on_timer();
            }
            LRESULT(0)
        }
        WM_DESTROY => LRESULT(0),
        // SAFETY: Forwarding unhandled messages to `DefWindowProcW`.
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

unsafe extern "system" fn panel_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            // SAFETY: `hwnd` belongs to the overlay thread.
            if let Some(state) = unsafe { get_window_state(hwnd) } {
                state.paint_panel(hwnd);
                return LRESULT(0);
            }
            // SAFETY: Fallback default processing when state is detached.
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        // SAFETY: Forwarding unhandled messages to `DefWindowProcW`.
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

unsafe extern "system" fn bar_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_SETCURSOR => {
            // SAFETY: `hwnd` belongs to the overlay thread.
            let is_pointer = unsafe { get_window_state(hwnd) }
                .is_some_and(|s| !s.visibility.finishing && s.hovered_bar_control.is_some());
            let cursor_id = if is_pointer { IDC_HAND } else { IDC_ARROW };
            // SAFETY: Loading standard system cursor.
            if let Ok(cur) = unsafe { LoadCursorW(None, cursor_id) } {
                // SAFETY: Applying loaded system cursor handle for this window.
                unsafe {
                    let _ = SetCursor(Some(cur));
                }
                return LRESULT(1);
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let (lx, ly) = lparam_xy(lparam);
            // SAFETY: `hwnd` belongs to the overlay thread.
            if let Some(state) = unsafe { get_window_state(hwnd) } {
                state.on_bar_mouse_move(lx, ly);
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            // SAFETY: `hwnd` belongs to the overlay thread.
            if let Some(state) = unsafe { get_window_state(hwnd) } {
                state.on_bar_mouse_leave();
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let (lx, ly) = lparam_xy(lparam);
            // SAFETY: `hwnd` belongs to the overlay thread.
            if let Some(state) = unsafe { get_window_state(hwnd) } {
                state.on_bar_click(lx, ly);
            }
            LRESULT(0)
        }
        // SAFETY: Forwarding unhandled messages to `DefWindowProcW`.
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

unsafe extern "system" fn popup_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_SETCURSOR => {
            // SAFETY: `hwnd` belongs to the overlay thread.
            let cursor_id = match unsafe { get_window_state(hwnd) }
                .and_then(|s| s.hovered_popup_hit.as_ref())
            {
                Some(PopupHit::SearchInput) => IDC_IBEAM,
                Some(
                    PopupHit::ClearSearch | PopupHit::LanguageRow(_) | PopupHit::RecentChip(_),
                ) => IDC_HAND,
                None => IDC_ARROW,
            };
            // SAFETY: Loading standard system cursor.
            if let Ok(cur) = unsafe { LoadCursorW(None, cursor_id) } {
                // SAFETY: Applying loaded system cursor handle for this window.
                unsafe {
                    let _ = SetCursor(Some(cur));
                }
                return LRESULT(1);
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let (lx, ly) = lparam_xy(lparam);
            // SAFETY: `hwnd` belongs to the overlay thread.
            if let Some(state) = unsafe { get_window_state(hwnd) } {
                state.on_popup_mouse_move(lx, ly);
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            // SAFETY: `hwnd` belongs to the overlay thread.
            if let Some(state) = unsafe { get_window_state(hwnd) } {
                state.on_popup_mouse_leave();
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let (lx, ly) = lparam_xy(lparam);
            // SAFETY: `hwnd` belongs to the overlay thread.
            if let Some(state) = unsafe { get_window_state(hwnd) } {
                state.on_popup_click(lx, ly);
            }
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let (sx, sy) = lparam_xy(lparam);
            let delta = wparam_wheel_delta(wparam);
            // SAFETY: `hwnd` belongs to the overlay thread.
            if let Some(state) = unsafe { get_window_state(hwnd) } {
                state.on_popup_wheel(sx, sy, delta, false);
            }
            LRESULT(0)
        }
        WM_MOUSEHWHEEL => {
            let (sx, sy) = lparam_xy(lparam);
            let delta = wparam_wheel_delta(wparam);
            // SAFETY: `hwnd` belongs to the overlay thread.
            if let Some(state) = unsafe { get_window_state(hwnd) } {
                state.on_popup_wheel(sx, sy, delta, true);
            }
            LRESULT(0)
        }
        WM_APP_POPUP_VKEY => {
            // SAFETY: `hwnd` belongs to the overlay thread.
            if let Some(state) = unsafe { get_window_state(hwnd) } {
                state.on_popup_vkey(wparam.0 as u16);
            }
            LRESULT(0)
        }
        WM_APP_POPUP_CHAR => {
            if let Some(ch) = char::from_u32(wparam.0 as u32) {
                // SAFETY: `hwnd` belongs to the overlay thread.
                if let Some(state) = unsafe { get_window_state(hwnd) } {
                    state.on_popup_char(ch);
                }
            }
            LRESULT(0)
        }
        // SAFETY: Forwarding unhandled messages to `DefWindowProcW`.
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

unsafe extern "system" fn recovery_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_SETCURSOR => {
            // SAFETY: `hwnd` belongs to the overlay thread.
            let is_pointer = unsafe { get_window_state(hwnd) }
                .is_some_and(|s| s.hovered_recovery_btn.is_some());
            let cursor_id = if is_pointer { IDC_HAND } else { IDC_ARROW };
            // SAFETY: Loading standard system cursor.
            if let Ok(cur) = unsafe { LoadCursorW(None, cursor_id) } {
                // SAFETY: Applying loaded system cursor handle for this window.
                unsafe {
                    let _ = SetCursor(Some(cur));
                }
                return LRESULT(1);
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let (lx, ly) = lparam_xy(lparam);
            // SAFETY: `hwnd` belongs to the overlay thread.
            if let Some(state) = unsafe { get_window_state(hwnd) } {
                state.on_recovery_mouse_move(lx, ly);
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            // SAFETY: `hwnd` belongs to the overlay thread.
            if let Some(state) = unsafe { get_window_state(hwnd) } {
                state.on_recovery_mouse_leave();
            }
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let (lx, ly) = lparam_xy(lparam);
            // SAFETY: `hwnd` belongs to the overlay thread.
            if let Some(state) = unsafe { get_window_state(hwnd) } {
                state.on_recovery_click(lx, ly);
            }
            LRESULT(0)
        }
        // SAFETY: Forwarding unhandled messages to `DefWindowProcW`.
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

#[allow(dead_code)]
fn _unused_hmenu_check(_: HMENU) {}
