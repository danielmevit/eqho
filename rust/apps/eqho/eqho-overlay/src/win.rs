//! Native Win32 layered iridescent pill and rounded translucent panel.

use std::ffi::c_void;
use std::ptr::{copy_nonoverlapping, null_mut};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Instant;

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Dwm::{DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, BeginPaint,
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateCompatibleDC, CreateDIBSection, CreateFontW,
    CreateSolidBrush, DEFAULT_CHARSET, DEFAULT_PITCH, DIB_RGB_COLORS, DT_CALCRECT, DT_LEFT,
    DT_NOPREFIX, DT_WORDBREAK, DeleteDC, DeleteObject, DrawTextW, EndPaint, FF_SWISS, FillRect,
    GetDC, GetDeviceCaps, HBITMAP, HDC, HFONT, HGDIOBJ, InvalidateRect, LOGPIXELSY,
    OUT_DEFAULT_PRECIS, PAINTSTRUCT, ReleaseDC, SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetClientRect, GetMessageW,
    GetSystemMetrics, HMENU, HWND_TOPMOST, KillTimer, LWA_ALPHA, MSG, PostMessageW,
    PostQuitMessage, RegisterClassExW, SM_CXSCREEN, SM_CYSCREEN, SPI_GETWORKAREA, SW_HIDE,
    SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_SHOWWINDOW, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    SetLayeredWindowAttributes, SetTimer, SetWindowPos, ShowWindow, SystemParametersInfoW,
    TranslateMessage, ULW_ALPHA, UnregisterClassW, UpdateLayeredWindow, WM_APP, WM_DESTROY,
    WM_ERASEBKGND, WM_PAINT, WM_TIMER, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::PCWSTR;

use crate::pure::{
    self, FRAME_MS, OverlayConfig, OverlayRect, PANEL_MAX_W, PANEL_PADDING_X, PANEL_PADDING_Y,
    PillAnimator, WorkArea,
};

const WM_APP_CMD: u32 = WM_APP + 1;
const TIMER_ID: usize = 1;

pub(crate) enum OverlayCmd {
    Show(String),
    UpdateText(String),
    SetLevel(f32),
    SetWaveform(f32, f32),
    UpdateConfig(OverlayConfig),
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
    pub(crate) fn start(config: OverlayConfig) -> Option<Self> {
        let (tx, rx) = mpsc::channel::<OverlayCmd>();
        let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<isize, String>>(1);

        let handle = std::thread::Builder::new()
            .name("eqho-overlay".to_owned())
            .spawn(move || run_overlay_thread(config, rx, ready_tx))
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

struct PillDib {
    screen_dc: HDC,
    mem_dc: HDC,
    dib: Option<HBITMAP>,
    old_obj: Option<HGDIOBJ>,
    bits: *mut c_void,
    width: u32,
    height: u32,
    frame_buf: Vec<u8>,
}

impl PillDib {
    fn new() -> Self {
        // SAFETY: `GetDC(None)` acquires the screen DC; `CreateCompatibleDC` creates a memory DC.
        let screen_dc = unsafe { GetDC(None) };
        // SAFETY: `screen_dc` is valid for the lifetime of `PillDib`.
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
        let Ok(dib) =
            (unsafe { CreateDIBSection(Some(self.mem_dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) })
        else {
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

impl Drop for PillDib {
    fn drop(&mut self) {
        self.release_dib();
        // SAFETY: Releasing owned `mem_dc` and `screen_dc`.
        unsafe {
            let _ = DeleteDC(self.mem_dc);
            let _ = ReleaseDC(None, self.screen_dc);
        }
    }
}

struct OverlayState {
    rx: mpsc::Receiver<OverlayCmd>,
    config: OverlayConfig,
    pill_hwnd: HWND,
    panel_hwnd: HWND,
    animator: PillAnimator,
    dib: PillDib,
    visible: bool,
    timer_running: bool,
    panel_shown: bool,
    panel_text: String,
    pill_rect: OverlayRect,
    last_tick: Instant,
    font: Option<HFONT>,
    font_size: i32,
}

impl OverlayState {
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

    fn handle_commands(&mut self) -> bool {
        let mut should_quit = false;
        while let Ok(cmd) = self.rx.try_recv() {
            match cmd {
                OverlayCmd::Show(text) => self.cmd_show(text),
                OverlayCmd::UpdateText(text) => self.cmd_update_text(text),
                OverlayCmd::SetLevel(level) => self.animator.set_level(level),
                OverlayCmd::SetWaveform(loud, tone) => self.animator.feed_voice(loud, tone),
                OverlayCmd::UpdateConfig(cfg) => self.cmd_update_config(cfg),
                OverlayCmd::Hide => self.cmd_hide(),
                OverlayCmd::Shutdown => {
                    should_quit = true;
                    break;
                }
            }
        }
        should_quit
    }

    fn cmd_show(&mut self, text: String) {
        if !self.config.enabled {
            return;
        }
        self.update_pill_geometry();
        if !self.visible {
            self.visible = true;
            self.animator.reset();
            self.animator.fade_target = 1.0;
            self.last_tick = Instant::now();
            self.render_pill_frame(0.0);
            // SAFETY: `SW_SHOWNOACTIVATE` displays the pill without stealing focus.
            unsafe {
                let _ = ShowWindow(self.pill_hwnd, SW_SHOWNOACTIVATE);
            }
            self.start_timer();
        } else {
            self.animator.fade_target = 1.0;
            self.animator.alpha_logical = 1.0;
            self.render_pill_frame(0.0);
        }
        self.cmd_update_text(text);
    }

    fn cmd_update_text(&mut self, text: String) {
        if !self.visible {
            return;
        }
        if !pure::should_show_panel(&text) {
            if self.panel_shown {
                // SAFETY: Hiding panel window without activating.
                unsafe {
                    let _ = ShowWindow(self.panel_hwnd, SW_HIDE);
                }
                self.panel_shown = false;
            }
            self.panel_text.clear();
            return;
        }

        let tailed = pure::tail_text(text.trim());
        self.panel_text = tailed;
        self.layout_and_show_panel();
    }

    fn cmd_update_config(&mut self, cfg: OverlayConfig) {
        let was_enabled = self.config.enabled;
        self.config = cfg;
        if !self.config.enabled && was_enabled && self.visible {
            self.cmd_hide();
            return;
        }
        if self.visible {
            self.update_pill_geometry();
            self.render_pill_frame(0.0);
            if self.panel_shown {
                self.layout_and_show_panel();
            }
        }
    }

    fn cmd_hide(&mut self) {
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
        let area = query_work_area();
        let (px, py) = pure::calc_pill_position(&self.config.position, pw, ph, area);
        self.pill_rect = OverlayRect {
            x: px,
            y: py,
            width: pw,
            height: ph,
        };
        let _ = self.dib.ensure_size(pw, ph);
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
        let area = query_work_area();
        let rect = pure::calc_panel_rect(
            &self.config.position,
            self.pill_rect,
            panel_w,
            panel_h,
            area,
        );

        let alpha = self.animator.effective_panel_alpha(&self.config);
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
        let dt = (now - self.last_tick).as_secs_f32();
        self.last_tick = now;

        self.render_pill_frame(dt);

        if !self.visible && self.animator.alpha_logical <= 0.0 {
            self.stop_timer();
            // SAFETY: Hiding both overlay windows once fade-out completes.
            unsafe {
                let _ = ShowWindow(self.pill_hwnd, SW_HIDE);
                let _ = ShowWindow(self.panel_hwnd, SW_HIDE);
            }
            self.panel_shown = false;
            self.panel_text.clear();
        }
    }

    fn render_pill_frame(&mut self, dt_s: f32) {
        let frame = self.animator.step(dt_s, self.config.show_level);
        let _ = self.animator.step_fade(dt_s);

        let w = self.pill_rect.width;
        let h = self.pill_rect.height;
        if !self.dib.ensure_size(w, h) {
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
        let copy_len = rendered.len().min(self.dib.frame_buf.len());
        self.dib.frame_buf[..copy_len].copy_from_slice(&rendered[..copy_len]);

        if !self.dib.bits.is_null() {
            // SAFETY: `self.dib.bits` is valid for `w * h * 4` bytes from `CreateDIBSection`.
            unsafe {
                copy_nonoverlapping(
                    self.dib.frame_buf.as_ptr(),
                    self.dib.bits as *mut u8,
                    copy_len,
                );
            }
        }

        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: self.animator.effective_pill_alpha(&self.config),
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let dst_pt = POINT {
            x: self.pill_rect.x,
            y: self.pill_rect.y,
        };
        let src_pt = POINT { x: 0, y: 0 };
        let size = SIZE {
            cx: w as i32,
            cy: h as i32,
        };

        // SAFETY: `pill_hwnd`, `screen_dc`, `mem_dc`, and `blend` are all valid.
        unsafe {
            let _ = UpdateLayeredWindow(
                self.pill_hwnd,
                Some(self.dib.screen_dc),
                Some(&dst_pt),
                Some(&size),
                Some(self.dib.mem_dc),
                Some(&src_pt),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
        }

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
        if let Some(font) = self.font.take() {
            // SAFETY: Deleting owned GDI font.
            unsafe {
                let _ = DeleteObject(font.into());
            }
        }
    }
}

fn query_work_area() -> WorkArea {
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

    let wc_pill = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(pill_wnd_proc),
        hInstance: hinstance.into(),
        lpszClassName: PCWSTR(pill_class.as_ptr()),
        ..Default::default()
    };
    let wc_panel = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(panel_wnd_proc),
        hInstance: hinstance.into(),
        lpszClassName: PCWSTR(panel_class.as_ptr()),
        ..Default::default()
    };

    // SAFETY: Registering window classes for pill and panel.
    let pill_atom = unsafe { RegisterClassExW(&wc_pill) };
    // SAFETY: Registering panel window class.
    let panel_atom = unsafe { RegisterClassExW(&wc_panel) };
    if pill_atom == 0 || panel_atom == 0 {
        let _ = ready_tx.send(Err("RegisterClassExW failed".to_owned()));
        return;
    }

    let (pw, ph) = eqho_core::pillfx::pill_size(config.pill_scale);
    let area = query_work_area();
    let (px, py) = pure::calc_pill_position(&config.position, pw, ph, area);

    let pill_ex =
        WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TRANSPARENT;
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
        // SAFETY: Unregistering classes on failure.
        unsafe {
            let _ = UnregisterClassW(PCWSTR(pill_class.as_ptr()), Some(hinstance.into()));
            let _ = UnregisterClassW(PCWSTR(panel_class.as_ptr()), Some(hinstance.into()));
        }
        let _ = ready_tx.send(Err("CreateWindowExW pill failed".to_owned()));
        return;
    };

    let panel_ex = WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE;
    // SAFETY: Creating borderless non-activating layered panel window.
    let Ok(panel_hwnd) = (unsafe {
        CreateWindowExW(
            panel_ex,
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
        // SAFETY: Cleaning up pill window and classes on failure.
        unsafe {
            let _ = DestroyWindow(pill_hwnd);
            let _ = UnregisterClassW(PCWSTR(pill_class.as_ptr()), Some(hinstance.into()));
            let _ = UnregisterClassW(PCWSTR(panel_class.as_ptr()), Some(hinstance.into()));
        }
        let _ = ready_tx.send(Err("CreateWindowExW panel failed".to_owned()));
        return;
    };

    // Best-effort Windows 11 rounded corners on the panel (ignored on Windows 10).
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

    let mut state = Box::new(OverlayState {
        rx,
        config,
        pill_hwnd,
        panel_hwnd,
        animator: PillAnimator::new(),
        dib: PillDib::new(),
        visible: false,
        timer_running: false,
        panel_shown: false,
        panel_text: String::new(),
        pill_rect: OverlayRect {
            x: px,
            y: py,
            width: pw,
            height: ph,
        },
        last_tick: Instant::now(),
        font: None,
        font_size: 0,
    });
    let state_ptr = (&mut *state) as *mut OverlayState;
    set_window_state(pill_hwnd, state_ptr);
    set_window_state(panel_hwnd, state_ptr);

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
    drop(state);

    // SAFETY: Unregistering window classes after message loop exit.
    unsafe {
        let _ = UnregisterClassW(PCWSTR(pill_class.as_ptr()), Some(hinstance.into()));
        let _ = UnregisterClassW(PCWSTR(panel_class.as_ptr()), Some(hinstance.into()));
    }
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

unsafe extern "system" fn pill_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_APP_CMD => {
            let mut destroy_handles: Option<(HWND, HWND)> = None;
            // SAFETY: `hwnd` belongs to the overlay thread.
            if let Some(state) = unsafe { get_window_state(hwnd) }
                && state.handle_commands()
            {
                state.stop_timer();
                destroy_handles = Some((state.panel_hwnd, hwnd));
            }
            if let Some((panel, pill)) = destroy_handles {
                // Detach state pointers BEFORE `DestroyWindow` so re-entrant messages
                // cannot alias `OverlayState`.
                set_window_state(panel, null_mut());
                set_window_state(pill, null_mut());
                // SAFETY: Destroying owned windows and exiting message loop on shutdown.
                unsafe {
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

#[allow(dead_code)]
fn _unused_hmenu_check(_: HMENU) {}
