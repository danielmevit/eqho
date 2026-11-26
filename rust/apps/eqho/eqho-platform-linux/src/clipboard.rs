//! Wayland (`wl-copy`/`wl-paste`), X11 CLI (`xclip`/`xsel`), and built-in native `libX11.so.6`
//! clipboard service (works out of the box on stock Ubuntu Wayland via Mutter's XWayland clipboard
//! bridge and on X11 with zero external CLI tools installed).

use std::ffi::{c_int, c_uchar, c_ulong};
use std::io::Write;
use std::process::{Command, Stdio};
use std::ptr::{null, null_mut};
use std::sync::{Mutex, OnceLock, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use eqho_platform::{ClipboardService, PlatformError};

use crate::portal_input;
use crate::pure::is_wayland_session;
use crate::x11_ffi::{
    CURRENT_TIME, PROP_MODE_REPLACE, SELECTION_CLEAR, SELECTION_NOTIFY, SELECTION_REQUEST,
    X11Api, XA_ATOM, XA_STRING, XEvent, XSelectionEvent,
};

static PENDING_WAYLAND_PASTE: OnceLock<Mutex<Option<(String, Instant)>>> = OnceLock::new();

fn pending_wayland_paste_slot() -> &'static Mutex<Option<(String, Instant)>> {
    PENDING_WAYLAND_PASTE.get_or_init(|| Mutex::new(None))
}

fn store_pending_wayland_paste(text: Option<&str>) {
    if let Ok(mut guard) = pending_wayland_paste_slot().lock() {
        *guard = text.map(|t| (t.to_owned(), Instant::now()));
    }
}

/// Consume the most recent private dictation text written on Wayland (valid for 5 s) so
/// `LinuxInputInjector::send_paste_shortcut` can deliver it through
/// `org.freedesktop.portal.Clipboard` (`Ctrl+V`) or direct `RemoteDesktop.NotifyKeyboardKeysym`
/// typing without relying on Mutter's blocked unfocused XWayland clipboard bridge.
pub(crate) fn take_pending_wayland_paste() -> Option<String> {
    let mut guard = pending_wayland_paste_slot().lock().ok()?;
    let (text, ts) = guard.take()?;
    if ts.elapsed() <= Duration::from_secs(5) {
        Some(text)
    } else {
        None
    }
}

struct ClipboardSetCmd {
    payload: Vec<u8>,
    ack: mpsc::SyncSender<Result<(), PlatformError>>,
}

struct X11ClipboardOwner {
    tx: mpsc::Sender<ClipboardSetCmd>,
    current_payload: &'static Mutex<Option<Vec<u8>>>,
}

static X11_OWNER: OnceLock<Option<X11ClipboardOwner>> = OnceLock::new();
static X11_CURRENT_PAYLOAD: OnceLock<Mutex<Option<Vec<u8>>>> = OnceLock::new();

fn current_payload_slot() -> &'static Mutex<Option<Vec<u8>>> {
    X11_CURRENT_PAYLOAD.get_or_init(|| Mutex::new(None))
}

fn ensure_x11_owner() -> Option<&'static X11ClipboardOwner> {
    X11_OWNER
        .get_or_init(|| {
            let api = X11Api::load()?;
            let (tx, rx) = mpsc::channel::<ClipboardSetCmd>();
            let payload_slot = current_payload_slot();
            thread::Builder::new()
                .name("eqho-x11-clipboard".to_owned())
                .spawn(move || run_x11_clipboard_owner(api, rx, payload_slot))
                .ok()?;
            Some(X11ClipboardOwner {
                tx,
                current_payload: payload_slot,
            })
        })
        .as_ref()
}

fn run_x11_clipboard_owner(
    api: &'static X11Api,
    rx: mpsc::Receiver<ClipboardSetCmd>,
    payload_slot: &'static Mutex<Option<Vec<u8>>>,
) {
    // SAFETY: Dedicated X11 connection owned exclusively by this background clipboard thread.
    unsafe {
        let dpy = (api.open_display)(null());
        if dpy.is_null() {
            while let Ok(cmd) = rx.recv() {
                let _ = cmd
                    .ack
                    .send(Err(PlatformError("XOpenDisplay failed for clipboard".to_owned())));
            }
            return;
        }
        let root = (api.default_root_window)(dpy);
        let win = (api.create_simple_window)(dpy, root, 0, 0, 1, 1, 0, 0, 0);
        let clip_atom = (api.intern_atom)(dpy, c"CLIPBOARD".as_ptr(), 0);
        let utf8_atom = (api.intern_atom)(dpy, c"UTF8_STRING".as_ptr(), 0);
        let text_utf8_atom = (api.intern_atom)(dpy, c"text/plain;charset=utf-8".as_ptr(), 0);
        let text_plain_atom = (api.intern_atom)(dpy, c"text/plain".as_ptr(), 0);
        let targets_atom = (api.intern_atom)(dpy, c"TARGETS".as_ptr(), 0);

        let mut payload: Vec<u8> = Vec::new();
        let mut owns_selection = false;
        let mut ev: XEvent = std::mem::zeroed();

        loop {
            while let Ok(cmd) = rx.try_recv() {
                payload = cmd.payload;
                (api.set_selection_owner)(dpy, clip_atom, win, CURRENT_TIME);
                (api.flush)(dpy);
                let owner = (api.get_selection_owner)(dpy, clip_atom);
                if owner == win {
                    owns_selection = true;
                    if let Ok(mut guard) = payload_slot.lock() {
                        *guard = Some(payload.clone());
                    }
                    let _ = cmd.ack.send(Ok(()));
                } else {
                    owns_selection = false;
                    if let Ok(mut guard) = payload_slot.lock() {
                        *guard = None;
                    }
                    let _ = cmd.ack.send(Err(PlatformError(
                        "X11 server rejected CLIPBOARD selection ownership".to_owned(),
                    )));
                }
            }

            if !owns_selection {
                match rx.recv_timeout(Duration::from_millis(50)) {
                    Ok(cmd) => {
                        payload = cmd.payload;
                        (api.set_selection_owner)(dpy, clip_atom, win, CURRENT_TIME);
                        (api.flush)(dpy);
                        let owner = (api.get_selection_owner)(dpy, clip_atom);
                        if owner == win {
                            owns_selection = true;
                            if let Ok(mut guard) = payload_slot.lock() {
                                *guard = Some(payload.clone());
                            }
                            let _ = cmd.ack.send(Ok(()));
                        } else {
                            let _ = cmd.ack.send(Err(PlatformError(
                                "X11 server rejected CLIPBOARD selection ownership".to_owned(),
                            )));
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
                continue;
            }

            if (api.pending)(dpy) == 0 {
                thread::sleep(Duration::from_millis(4));
                continue;
            }

            (api.next_event)(dpy, &mut ev);
            if ev.r#type == SELECTION_REQUEST {
                let req = ev.xselectionrequest;
                let prop = if req.property != 0 {
                    req.property
                } else {
                    req.target
                };
                let mut notify_prop: c_ulong = 0;
                if req.selection == clip_atom {
                    if req.target == targets_atom {
                        let supported: [c_ulong; 5] = [
                            targets_atom,
                            utf8_atom,
                            text_utf8_atom,
                            text_plain_atom,
                            XA_STRING,
                        ];
                        (api.change_property)(
                            dpy,
                            req.requestor,
                            prop,
                            XA_ATOM,
                            32,
                            PROP_MODE_REPLACE,
                            supported.as_ptr().cast(),
                            supported.len() as c_int,
                        );
                        notify_prop = prop;
                    } else if req.target == utf8_atom
                        || req.target == text_utf8_atom
                        || req.target == text_plain_atom
                        || req.target == XA_STRING
                    {
                        (api.change_property)(
                            dpy,
                            req.requestor,
                            prop,
                            req.target,
                            8,
                            PROP_MODE_REPLACE,
                            payload.as_ptr(),
                            payload.len() as c_int,
                        );
                        notify_prop = prop;
                    }
                }
                let mut resp = XEvent {
                    xselection: XSelectionEvent {
                        r#type: SELECTION_NOTIFY,
                        serial: 0,
                        send_event: 1,
                        display: req.display,
                        requestor: req.requestor,
                        selection: req.selection,
                        target: req.target,
                        property: notify_prop,
                        time: req.time,
                    },
                };
                (api.send_event)(dpy, req.requestor, 0, 0, &mut resp);
                (api.flush)(dpy);
            } else if ev.r#type == SELECTION_CLEAR {
                owns_selection = false;
                if let Ok(mut guard) = payload_slot.lock() {
                    *guard = None;
                }
            }
        }

        (api.destroy_window)(dpy, win);
        (api.close_display)(dpy);
    }
}

fn x11_set_text(text: &str) -> Result<(), PlatformError> {
    let owner = ensure_x11_owner().ok_or_else(|| {
        PlatformError(
            "no Linux clipboard backend available (install wl-clipboard on Wayland or xclip/xsel on X11)"
                .to_owned(),
        )
    })?;
    let (ack_tx, ack_rx) = mpsc::sync_channel::<Result<(), PlatformError>>(1);
    owner
        .tx
        .send(ClipboardSetCmd {
            payload: text.as_bytes().to_vec(),
            ack: ack_tx,
        })
        .map_err(|_| PlatformError("X11 clipboard worker stopped".to_owned()))?;
    ack_rx
        .recv_timeout(Duration::from_secs(2))
        .map_err(|_| PlatformError("timed out setting X11 clipboard".to_owned()))?
}

fn x11_get_text() -> Option<String> {
    // If our own in-process X11 clipboard thread currently owns CLIPBOARD, return its payload
    // directly so we never block waiting on ourselves.
    if let Some(owner) = X11_OWNER.get().and_then(Option::as_ref)
        && let Ok(guard) = owner.current_payload.lock()
        && let Some(bytes) = guard.as_ref()
    {
        return Some(String::from_utf8_lossy(bytes).into_owned());
    }

    let api = X11Api::load()?;
    // SAFETY: Short-lived X11 connection requesting CLIPBOARD via `XConvertSelection`.
    unsafe {
        let dpy = (api.open_display)(null());
        if dpy.is_null() {
            return None;
        }
        let root = (api.default_root_window)(dpy);
        let clip_atom = (api.intern_atom)(dpy, c"CLIPBOARD".as_ptr(), 0);
        if (api.get_selection_owner)(dpy, clip_atom) == 0 {
            (api.close_display)(dpy);
            return None;
        }
        let win = (api.create_simple_window)(dpy, root, 0, 0, 1, 1, 0, 0, 0);
        let utf8_atom = (api.intern_atom)(dpy, c"UTF8_STRING".as_ptr(), 0);
        let prop_atom = (api.intern_atom)(dpy, c"EQHO_CLIP_PROP".as_ptr(), 0);

        let mut result: Option<String> = None;
        for target in [utf8_atom, XA_STRING] {
            (api.delete_property)(dpy, win, prop_atom);
            (api.convert_selection)(dpy, clip_atom, target, prop_atom, win, CURRENT_TIME);
            (api.flush)(dpy);

            let deadline = Instant::now() + Duration::from_millis(300);
            let mut ev: XEvent = std::mem::zeroed();
            let mut notified_prop: c_ulong = 0;
            while Instant::now() < deadline {
                if (api.pending)(dpy) == 0 {
                    thread::sleep(Duration::from_millis(5));
                    continue;
                }
                (api.next_event)(dpy, &mut ev);
                if ev.r#type == SELECTION_NOTIFY && ev.xselection.selection == clip_atom {
                    notified_prop = ev.xselection.property;
                    break;
                }
            }
            if notified_prop == 0 {
                continue;
            }

            let mut actual_type: c_ulong = 0;
            let mut actual_format: c_int = 0;
            let mut nitems: c_ulong = 0;
            let mut bytes_after: c_ulong = 0;
            let mut data: *mut c_uchar = null_mut();
            let st = (api.get_window_property)(
                dpy,
                win,
                notified_prop,
                0,
                1_048_576, // up to 4 MiB
                1,         // delete after reading
                0,         // AnyPropertyType
                &mut actual_type,
                &mut actual_format,
                &mut nitems,
                &mut bytes_after,
                &mut data,
            );
            if st == 0 && !data.is_null() {
                if actual_format == 8 {
                    let slice = std::slice::from_raw_parts(data, nitems as usize);
                    result = Some(String::from_utf8_lossy(slice).into_owned());
                }
                (api.free)(data.cast());
            }
            if result.is_some() {
                break;
            }
        }

        (api.destroy_window)(dpy, win);
        (api.close_display)(dpy);
        result
    }
}

/// Cross-session Linux clipboard supporting Wayland, X11 CLI tools, and built-in native `libX11`.
#[derive(Debug, Default, Clone, Copy)]
pub struct LinuxClipboardService;

impl LinuxClipboardService {
    fn is_wayland() -> bool {
        let wl = std::env::var("WAYLAND_DISPLAY").ok();
        let st = std::env::var("XDG_SESSION_TYPE").ok();
        is_wayland_session(wl.as_deref(), st.as_deref())
    }

    fn run_read(bin: &str, args: &[&str]) -> Option<String> {
        let out = Command::new(bin)
            .args(args)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        Some(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    fn run_write(bin: &str, args: &[&str], text: &str) -> Result<(), PlatformError> {
        // Redirecting stdout & stderr to null is critical for `xclip`/`wl-copy`, which fork
        // a background selection owner that would otherwise hold stdout open and block `.output()`.
        let mut child = Command::new(bin)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| PlatformError(format!("{bin} failed to start: {e}")))?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(text.as_bytes())
                .map_err(|e| PlatformError(format!("{bin} stdin write failed: {e}")))?;
        }
        let status = child
            .wait()
            .map_err(|e| PlatformError(format!("{bin} wait failed: {e}")))?;
        if status.success() {
            Ok(())
        } else {
            Err(PlatformError(format!("{bin} exited with {status}")))
        }
    }
}

impl ClipboardService for LinuxClipboardService {
    fn get_text(&self) -> Option<String> {
        if Self::is_wayland() {
            if let Some(t) = Self::run_read("wl-paste", &["--no-newline", "--type", "text/plain"]) {
                return Some(t);
            }
            if portal_input::is_portal_clipboard_active() {
                return portal_input::try_read_clipboard_text();
            }
        }
        Self::run_read("xclip", &["-selection", "clipboard", "-out"])
            .or_else(|| Self::run_read("xsel", &["--clipboard", "--output"]))
            .or_else(|| Self::run_read("wl-paste", &["--no-newline"]))
            .or_else(x11_get_text)
    }

    fn set_text(&self, text: &str, private: bool) -> Result<(), PlatformError> {
        if Self::is_wayland() {
            if Self::run_write("wl-copy", &["--type", "text/plain;charset=utf-8"], text).is_ok() {
                store_pending_wayland_paste(None);
                return Ok(());
            }
            // Stash private dictation text so `send_paste_shortcut` can deliver it via native
            // Wayland `Clipboard` portal + `Ctrl+V` or `RemoteDesktop.NotifyKeyboardKeysym` even
            // when Mutter 50 blocks unfocused XWayland `XSetSelectionOwner` from reaching Wayland.
            store_pending_wayland_paste(if private { Some(text) } else { None });
            let portal_ok = portal_input::try_set_clipboard_text(text);
            let x11_res = x11_set_text(text);
            if portal_ok {
                return Ok(());
            }
            return x11_res;
        }
        if Self::run_write("xclip", &["-selection", "clipboard", "-in"], text).is_ok() {
            return Ok(());
        }
        if Self::run_write("xsel", &["--clipboard", "--input"], text).is_ok() {
            return Ok(());
        }
        if Self::run_write("wl-copy", &["--type", "text/plain;charset=utf-8"], text).is_ok() {
            return Ok(());
        }
        x11_set_text(text)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn x11_clipboard_roundtrip_when_display_available() {
        if std::env::var_os("DISPLAY").is_none() || X11Api::load().is_none() {
            return;
        }
        let svc = LinuxClipboardService;
        let sample = "Eqho Linux clipboard test — UTF-8 ✓";
        svc.set_text(sample, true).unwrap();
        assert_eq!(svc.get_text().as_deref(), Some(sample));
    }
}
