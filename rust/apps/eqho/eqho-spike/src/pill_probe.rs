//! Pure-Rust premultiplied BGRA8 iridescent pill renderer + Win32 layered window probe.

use std::time::Instant;

use serde::Serialize;

/// Configuration for `pill-bench`.
#[derive(Debug)]
pub struct PillBenchArgs {
    pub frames: usize,
    pub width: u32,
    pub height: u32,
    pub light: bool,
    pub show_ms: u64,
}

#[derive(Debug, Serialize)]
pub struct PillBenchOutput {
    pub ok: bool,
    pub frames: usize,
    pub width: u32,
    pub height: u32,
    pub avg_ms: f64,
    pub p95_ms: f64,
    pub max_ms: f64,
    pub shown: bool,
    pub focus_stolen: bool,
}

#[inline]
fn gs(x: f32, s: f32) -> f32 {
    (-(x * x) / (2.0 * s * s)).exp()
}

#[inline]
fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[inline]
fn mix3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [mix(a[0], b[0], t), mix(a[1], b[1], t), mix(a[2], b[2], t)]
}

#[inline]
fn paint3(col: [f32; 3], target: [f32; 3], factor: [f32; 3]) -> [f32; 3] {
    [
        col[0] + (target[0] - col[0]) * factor[0],
        col[1] + (target[1] - col[1]) * factor[1],
        col[2] + (target[2] - col[2]) * factor[2],
    ]
}

/// Render one iridescent pill frame into premultiplied BGRA8 (`width * height * 4` bytes).
/// Matches the visual math of `src/pillfx.py`.
pub fn render_pill_bgra(
    buf: &mut [u8],
    width: u32,
    height: u32,
    t: f32,
    level: f32,
    light: f32,
) {
    let w = width as usize;
    let h = height as usize;
    debug_assert!(buf.len() >= w * h * 4);

    let wf = width as f32;
    let hf = height as f32;
    let aspect = wf / hf;
    let lvl = level.clamp(0.0, 1.0);
    let light = light.clamp(0.0, 1.0);

    let r = 0.36 + 0.015 * (t * 1.6).sin() + 0.045 * lvl;
    let hw = (aspect * 0.5 - r - 0.05).max(0.0);

    let body = mix3([0.035, 0.060, 0.140], [0.955, 0.970, 0.995], light);
    let tint = mix3([0.080, 0.170, 0.420], [0.800, 0.890, 0.990], light);

    let amp = r * (0.19 + 0.22 * lvl);
    let th = r * (0.17 + 0.10 * lvl);
    let fr = r * (0.050 + 0.045 * lvl);
    let aw = r * 0.085;

    let ld_raw = [-0.45f32, -0.60, 0.66];
    let ld_len = (ld_raw[0] * ld_raw[0] + ld_raw[1] * ld_raw[1] + ld_raw[2] * ld_raw[2]).sqrt();
    let ld = [ld_raw[0] / ld_len, ld_raw[1] / ld_len, ld_raw[2] / ld_len];

    let aa = 1.5 / hf;
    let bw = 3.0 / hf;

    for y in 0..h {
        let uvy = (y as f32) / hf - 0.5;
        let py = uvy;
        let qy = py / r;
        let lower_t = ((qy - 0.05) / (0.6 - 0.05)).clamp(0.0, 1.0);
        let lower = lower_t * lower_t * (3.0 - 2.0 * lower_t);

        let row_offset = y * w * 4;
        for x in 0..w {
            let uvx = ((x as f32) / wf - 0.5) * aspect;
            let px = uvx - uvx.clamp(-hw, hw);
            let d = (px * px + py * py).sqrt() - r;

            let qx = px / r;
            let nz = (1.0 - (qx * qx + qy * qy)).max(0.0).sqrt();
            let nz_c = nz.max(1e-3);
            let nlen = (qx * qx + qy * qy + nz_c * nz_c).sqrt();
            let n_x = qx / nlen;
            let n_y = qy / nlen;
            let n_z = nz_c / nlen;

            let wx = uvx + 0.22 * (1.7 * uvy + t * 0.33).sin();
            let wy = uvy + 0.22 * (1.9 * uvx + t * 0.27).sin();
            let drift = 0.5 + 0.5 * (2.1 * wx + 2.6 * wy + t * 0.21).sin();
            let mut col = [
                body[0] + (tint[0] - body[0]) * (drift * 0.55),
                body[1] + (tint[1] - body[1]) * (drift * 0.55),
                body[2] + (tint[2] - body[2]) * (drift * 0.55),
            ];

            let c1 = amp * (uvx * 2.6 - t * 1.15).sin() + amp * 0.45 * (uvx * 4.9 + t * 0.70).sin();
            let c2 = amp * 0.75 * (uvx * 3.4 + t * 0.90 + 1.7).sin() - r * 0.16;

            let y1 = uvy - c1;
            let b1 = [gs(y1 - fr, th), gs(y1, th), gs(y1 + fr, th)];
            let d1 = (1.0 - light) * 0.80;
            let l1 = light * 0.78;
            col[0] += b1[0] * 0.250 * d1;
            col[1] += b1[1] * 0.550 * d1;
            col[2] += b1[2] * 1.000 * d1;
            col = paint3(col, [0.100, 0.420, 0.850], [b1[0] * l1, b1[1] * l1, b1[2] * l1]);

            let y2 = uvy - c2;
            let th2 = th * 0.55;
            let b2 = [gs(y2 + fr, th2), gs(y2, th2), gs(y2 - fr, th2)];
            let d2 = (1.0 - light) * 0.35;
            let l2 = light * 0.60;
            col[0] += b2[0] * 0.450 * d2;
            col[1] += b2[1] * 0.850 * d2;
            col[2] += b2[2] * 1.000 * d2;
            col = paint3(col, [0.150, 0.620, 0.920], [b2[0] * l2, b2[1] * l2, b2[2] * l2]);

            let ad = d + r * 0.24;
            let pulse = 0.6 + 0.4 * (t * 0.8 + uvx * 1.7).sin();
            let lp = lower * pulse;
            let arc = [gs(ad - fr, aw), gs(ad, aw), gs(ad + fr, aw)];
            let adk = lp * (1.0 - light) * 0.85;
            let alt = lp * light * 0.42;
            col[0] += arc[0] * 0.250 * adk;
            col[1] += arc[1] * 0.450 * adk;
            col[2] += arc[2] * 1.000 * adk;
            col = paint3(
                col,
                [0.250, 0.500, 0.920],
                [arc[0] * alt, arc[1] * alt, arc[2] * alt],
            );

            let fres = (1.0 - n_z.clamp(0.0, 1.0)).powf(2.3);
            let fdk = fres * (1.0 - light) * 0.65 * (0.7 + 0.5 * lvl);
            let flt = fres * light * 0.55;
            col[0] += 0.300 * fdk;
            col[1] += 0.550 * fdk;
            col[2] += 1.000 * fdk;
            col = paint3(col, [0.450, 0.620, 0.920], [flt, flt, flt]);

            let ndl = (n_x * ld[0] + n_y * ld[1] + n_z * ld[2]).max(0.0);
            let sheen = ndl.powi(64) * 0.42 + ndl.powi(6) * 0.12;
            let sh_scale = sheen * mix(1.0, 0.95, light);
            col[0] += sh_scale * 1.0;
            col[1] += sh_scale * 0.97;
            col[2] += sh_scale * 0.98;

            col = paint3(
                col,
                [0.975, 0.975, 0.990],
                [light * 0.06, light * 0.06, light * 0.06],
            );

            let mapped = [
                1.0 - (-col[0] * 1.25).exp(),
                1.0 - (-col[1] * 1.25).exp(),
                1.0 - (-col[2] * 1.25).exp(),
            ];
            let clamped = [
                col[0].clamp(0.0, 1.0),
                col[1].clamp(0.0, 1.0),
                col[2].clamp(0.0, 1.0),
            ];
            col = mix3(mapped, clamped, light);

            let stroke = ((d + bw) / aa).clamp(0.0, 1.0) * (-d / aa).clamp(0.0, 1.0);
            let st_f = stroke * light * 0.85;
            col = paint3(col, [0.995, 0.995, 1.000], [st_f, st_f, st_f]);

            let mask = (1.0 - ((d + aa) / (2.0 * aa)).clamp(0.0, 1.0)).clamp(0.0, 1.0);
            let a = (mask * 255.0).round().clamp(0.0, 255.0) as u8;
            let af = f32::from(a);
            let r_u8 = ((col[0].clamp(0.0, 1.0) * af).round() as u8).min(a);
            let g_u8 = ((col[1].clamp(0.0, 1.0) * af).round() as u8).min(a);
            let b_u8 = ((col[2].clamp(0.0, 1.0) * af).round() as u8).min(a);

            let idx = row_offset + x * 4;
            buf[idx] = b_u8;
            buf[idx + 1] = g_u8;
            buf[idx + 2] = r_u8;
            buf[idx + 3] = a;
        }
    }
}

pub fn run_pill_bench(args: &PillBenchArgs) -> Result<PillBenchOutput, String> {
    let frames = args.frames.max(1);
    let width = args.width.max(16);
    let height = args.height.max(16);
    let light = if args.light { 1.0 } else { 0.0 };

    let mut buf = vec![0u8; (width as usize) * (height as usize) * 4];
    let mut times_ms = Vec::with_capacity(frames);

    for i in 0..frames {
        let t = i as f32 * (1.0 / 60.0);
        let level = 0.35 + 0.25 * (t * 2.5).sin();
        let start = Instant::now();
        render_pill_bgra(&mut buf, width, height, t, level, light);
        times_ms.push(start.elapsed().as_secs_f64() * 1000.0);
    }

    times_ms.sort_by(|a, b| a.total_cmp(b));
    let sum: f64 = times_ms.iter().sum();
    let avg_ms = sum / frames as f64;
    let p95_idx = ((frames as f64 * 0.95).ceil() as usize)
        .saturating_sub(1)
        .min(frames - 1);
    let p95_ms = times_ms[p95_idx];
    let max_ms = times_ms[frames - 1];

    let (shown, focus_stolen) = if args.show_ms > 0 {
        show_layered_pill(width, height, light, args.show_ms)?
    } else {
        (false, false)
    };

    Ok(PillBenchOutput {
        ok: true,
        frames,
        width,
        height,
        avg_ms,
        p95_ms,
        max_ms,
        shown,
        focus_stolen,
    })
}

#[cfg(windows)]
fn show_layered_pill(
    width: u32,
    height: u32,
    light: f32,
    show_ms: u64,
) -> Result<(bool, bool), String> {
    use std::ffi::c_void;
    use std::ptr::{copy_nonoverlapping, null_mut};
    use std::time::Duration;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
        CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC,
        ReleaseDC, SelectObject,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetForegroundWindow,
        GetSystemMetrics, MSG, PM_REMOVE, PeekMessageW, RegisterClassExW, SM_CXSCREEN,
        SM_CYSCREEN, SW_SHOWNOACTIVATE, ShowWindow, TranslateMessage, ULW_ALPHA,
        UnregisterClassW, UpdateLayeredWindow, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
        WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
    };
    use windows::core::PCWSTR;

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        // SAFETY: Standard Win32 default window procedure forwarding.
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    let class_name: Vec<u16> = "EqhoSpikePill\0".encode_utf16().collect();
    // SAFETY: `GetModuleHandleW(None)` returns the current executable's HMODULE.
    let hinstance = unsafe { GetModuleHandleW(None) }.map_err(|e| e.to_string())?;

    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(wnd_proc),
        hInstance: hinstance.into(),
        lpszClassName: PCWSTR(class_name.as_ptr()),
        ..Default::default()
    };

    // SAFETY: `wc` is valid for the duration of `RegisterClassExW`.
    let atom = unsafe { RegisterClassExW(&wc) };
    if atom == 0 {
        return Err("RegisterClassExW failed".to_string());
    }

    // SAFETY: `GetForegroundWindow` and `GetSystemMetrics` take no pointers.
    let fg_before = unsafe { GetForegroundWindow() };
    // SAFETY: See above.
    let screen_w = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    // SAFETY: See above.
    let screen_h = unsafe { GetSystemMetrics(SM_CYSCREEN) };
    let pos_x = (screen_w - width as i32) / 2;
    let pos_y = (screen_h - height as i32).saturating_sub(64);

    let ex_style =
        WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST | WS_EX_TRANSPARENT;

    // SAFETY: Creating a borderless layered popup window with a registered class name.
    let hwnd = unsafe {
        CreateWindowExW(
            ex_style,
            PCWSTR(class_name.as_ptr()),
            PCWSTR(class_name.as_ptr()),
            WS_POPUP,
            pos_x,
            pos_y,
            width as i32,
            height as i32,
            None,
            None,
            Some(hinstance.into()),
            None,
        )
    }
    .map_err(|e| {
        // SAFETY: Unregister class on failure.
        unsafe {
            let _ = UnregisterClassW(PCWSTR(class_name.as_ptr()), Some(hinstance.into()));
        }
        e.to_string()
    })?;

    // SAFETY: `ShowWindow` with `SW_SHOWNOACTIVATE` does not activate or steal focus.
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }

    // SAFETY: `GetDC(None)` acquires the screen DC, released below.
    let screen_dc = unsafe { GetDC(None) };
    // SAFETY: Creating a compatible memory DC for `UpdateLayeredWindow`.
    let mem_dc = unsafe { CreateCompatibleDC(Some(screen_dc)) };

    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width as i32,
            biHeight: -(height as i32), // top-down DIB
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut dib_bits: *mut c_void = null_mut();
    // SAFETY: `CreateDIBSection` allocates a 32-bit BGRA top-down DIB buffer in `dib_bits`.
    let dib = unsafe { CreateDIBSection(Some(mem_dc), &bmi, DIB_RGB_COLORS, &mut dib_bits, None, 0) }
        .map_err(|e| e.to_string())?;
    // SAFETY: Selecting `dib` into `mem_dc`.
    let old_obj = unsafe { SelectObject(mem_dc, dib.into()) };

    let mut frame_buf = vec![0u8; (width as usize) * (height as usize) * 4];
    let blend = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: 255,
        AlphaFormat: AC_SRC_ALPHA as u8,
    };
    let dst_pt = POINT { x: pos_x, y: pos_y };
    let src_pt = POINT { x: 0, y: 0 };
    let size = SIZE {
        cx: width as i32,
        cy: height as i32,
    };

    let mut focus_stolen = false;
    let start = Instant::now();
    let total = Duration::from_millis(show_ms);

    while start.elapsed() < total {
        let t = start.elapsed().as_secs_f32();
        let level = 0.35 + 0.25 * (t * 2.5).sin();
        render_pill_bgra(&mut frame_buf, width, height, t, level, light);

        if !dib_bits.is_null() {
            // SAFETY: `dib_bits` is valid for `width * height * 4` bytes.
            unsafe {
                copy_nonoverlapping(frame_buf.as_ptr(), dib_bits as *mut u8, frame_buf.len());
            }
        }

        // SAFETY: `hwnd`, `screen_dc`, `mem_dc`, and `blend` are all valid.
        unsafe {
            let _ = UpdateLayeredWindow(
                hwnd,
                Some(screen_dc),
                Some(&dst_pt),
                Some(&size),
                Some(mem_dc),
                Some(&src_pt),
                Default::default(),
                Some(&blend),
                ULW_ALPHA,
            );
        }

        let mut msg = MSG::default();
        // SAFETY: Pumping pending messages on the current thread.
        unsafe {
            while PeekMessageW(&mut msg, Some(hwnd), 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            if GetForegroundWindow() != fg_before {
                focus_stolen = true;
            }
        }

        std::thread::sleep(Duration::from_millis(16));
    }

    // SAFETY: Cleaning up GDI objects, window, and window class in reverse creation order.
    unsafe {
        let _ = SelectObject(mem_dc, old_obj);
        let _ = DeleteObject(dib.into());
        let _ = DeleteDC(mem_dc);
        let _ = ReleaseDC(None, screen_dc);
        let _ = DestroyWindow(hwnd);
        if GetForegroundWindow() != fg_before {
            focus_stolen = true;
        }
        let _ = UnregisterClassW(PCWSTR(class_name.as_ptr()), Some(hinstance.into()));
    }

    Ok((true, focus_stolen))
}

#[cfg(not(windows))]
fn show_layered_pill(
    _width: u32,
    _height: u32,
    _light: f32,
    _show_ms: u64,
) -> Result<(bool, bool), String> {
    Ok((false, false))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pill_bgra_invariants_dark_and_light() {
        let w = 280u32;
        let h = 51u32;
        let mut buf = vec![0u8; (w * h * 4) as usize];

        for light in [0.0f32, 1.0f32] {
            render_pill_bgra(&mut buf, w, h, 0.42, 0.5, light);

            // Corner pixels must be completely transparent.
            let corners = [
                (0usize, 0usize),
                ((w - 1) as usize, 0),
                (0, (h - 1) as usize),
                ((w - 1) as usize, (h - 1) as usize),
            ];
            for (cx, cy) in corners {
                let idx = (cy * w as usize + cx) * 4;
                assert_eq!(buf[idx + 3], 0, "corner ({cx},{cy}) alpha must be 0");
                assert_eq!(&buf[idx..idx + 4], &[0, 0, 0, 0]);
            }

            // Center pixel must be opaque.
            let center_idx = ((h as usize / 2) * w as usize + (w as usize / 2)) * 4;
            assert_eq!(buf[center_idx + 3], 255, "center pixel must be opaque");

            // Premultiplied BGRA invariant for every pixel: b <= a && g <= a && r <= a.
            for px in buf.chunks_exact(4) {
                let (b, g, r, a) = (px[0], px[1], px[2], px[3]);
                assert!(
                    b <= a && g <= a && r <= a,
                    "premultiplied invariant violated: b={b} g={g} r={r} a={a}"
                );
            }
        }
    }
}
