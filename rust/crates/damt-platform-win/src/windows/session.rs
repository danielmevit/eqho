use std::cell::RefCell;
use std::ffi::c_void;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::thread::JoinHandle;

use damt_platform::{SessionEvent, SessionEventSource};
use windows::Win32::Foundation::{HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Power::{
    HPOWERNOTIFY, RegisterSuspendResumeNotification, UnregisterSuspendResumeNotification,
};
use windows::Win32::System::RemoteDesktop::{
    NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification, WTSUnRegisterSessionNotification,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DEVICE_NOTIFY_WINDOW_HANDLE, DefWindowProcW, DestroyWindow, DispatchMessageW,
    GetMessageW, HMENU, HWND_MESSAGE, MSG, PBT_APMRESUMEAUTOMATIC, PBT_APMSUSPEND, PostMessageW,
    RegisterClassW, TranslateMessage, UnregisterClassW, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_POWERBROADCAST, WM_QUIT, WM_WTSSESSION_CHANGE, WNDCLASSW,
};
use windows::core::{HSTRING, PCWSTR};

use crate::SessionEventMapper;

type Sink = Arc<dyn Fn(SessionEvent) + Send + Sync>;

thread_local! {
    static SINK: RefCell<Option<Sink>> = RefCell::new(None);
}

static NEXT_CLASS: AtomicUsize = AtomicUsize::new(0);

/// Session and power events from a dedicated message-only window thread (C# `WindowsSessionEventSource`).
#[derive(Debug, Default)]
pub struct WinSessionEvents {
    started: bool,
    window: Option<isize>,
    thread: Option<JoinHandle<()>>,
}

impl WinSessionEvents {
    /// Creates an idle source; no thread starts until [`SessionEventSource::start`].
    pub fn new() -> Self {
        Self::default()
    }
}

impl SessionEventSource for WinSessionEvents {
    fn is_supported(&self) -> bool {
        true
    }

    fn start(&mut self, sink: Box<dyn Fn(SessionEvent) + Send + Sync>) {
        if self.started {
            return;
        }

        self.started = true;
        let sink: Sink = Arc::from(sink);
        let (ready_tx, ready_rx) = mpsc::channel::<Option<isize>>();

        let spawned = std::thread::Builder::new()
            .name("damt-session-events".to_owned())
            .spawn(move || run_message_thread(sink, &ready_tx));

        match spawned {
            Ok(thread) => {
                self.thread = Some(thread);
                self.window = ready_rx.recv().ok().flatten();
            }
            Err(_) => {
                self.started = false;
            }
        }
    }
}

impl Drop for WinSessionEvents {
    fn drop(&mut self) {
        if let Some(window) = self.window {
            let window = HWND(window as *mut c_void);
            // SAFETY: `window` belongs to the session thread and stays valid until it handles WM_QUIT.
            unsafe {
                let _ = PostMessageW(Some(window), WM_QUIT, WPARAM(0), LPARAM(0));
            }
        }

        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The hidden message window and the notifications registered against it.
struct MessageWindow {
    window: HWND,
    class_name: HSTRING,
    instance: HINSTANCE,
    session_registered: bool,
    suspend_resume: Option<HPOWERNOTIFY>,
}

impl MessageWindow {
    fn create() -> Option<Self> {
        // SAFETY: a null module name asks for this process's module handle.
        let module = unsafe { GetModuleHandleW(None::<&PCWSTR>) }.ok()?;
        let instance = HINSTANCE(module.0);

        let id = NEXT_CLASS.fetch_add(1, Ordering::Relaxed);
        let class_name = HSTRING::from(format!("damt-platform-win-session-{id}").as_str());
        let window_class = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance,
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };

        // SAFETY: the class structure is fully initialized and outlives the registration.
        if unsafe { RegisterClassW(&window_class) } == 0 {
            return None;
        }

        // SAFETY: the class is registered, and HWND_MESSAGE asks for a message-only window.
        let window = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                &class_name,
                None::<&PCWSTR>,
                WINDOW_STYLE(0),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None::<HMENU>,
                Some(instance),
                None,
            )
        };

        let window = match window {
            Ok(window) => window,
            Err(_) => {
                // SAFETY: the class was registered just above and is no longer needed.
                let _ = unsafe { UnregisterClassW(&class_name, Some(instance)) };
                return None;
            }
        };

        Some(Self {
            window,
            class_name,
            instance,
            session_registered: false,
            suspend_resume: None,
        })
    }

    fn register_session_notification(&mut self) -> bool {
        // SAFETY: `self.window` is a valid window owned by this thread.
        let registered =
            unsafe { WTSRegisterSessionNotification(self.window, NOTIFY_FOR_THIS_SESSION) }.is_ok();
        self.session_registered = registered;
        registered
    }

    fn register_suspend_resume_notification(&mut self) -> bool {
        // SAFETY: `self.window` is a valid window owned by this thread.
        let registered = unsafe {
            RegisterSuspendResumeNotification(HANDLE(self.window.0), DEVICE_NOTIFY_WINDOW_HANDLE)
        };

        match registered {
            Ok(handle) => {
                self.suspend_resume = Some(handle);
                true
            }
            Err(_) => false,
        }
    }

    fn pump_messages(&self) {
        let mut message = MSG::default();
        loop {
            // SAFETY: `message` is valid storage for the retrieved message.
            let result = unsafe { GetMessageW(&mut message, None, 0, 0) };
            if result.0 <= 0 {
                break;
            }

            // SAFETY: `message` was filled in by GetMessageW.
            unsafe {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }
}

impl Drop for MessageWindow {
    fn drop(&mut self) {
        // SAFETY: every handle was registered on this thread and is released exactly once here.
        unsafe {
            if let Some(handle) = self.suspend_resume.take() {
                let _ = UnregisterSuspendResumeNotification(handle);
            }
            if self.session_registered {
                let _ = WTSUnRegisterSessionNotification(self.window);
            }
            let _ = DestroyWindow(self.window);
            let _ = UnregisterClassW(&self.class_name, Some(self.instance));
        }
    }
}

fn run_message_thread(sink: Sink, ready: &Sender<Option<isize>>) {
    SINK.with(|slot| {
        *slot.borrow_mut() = Some(sink);
    });

    let Some(mut window) = MessageWindow::create() else {
        let _ = ready.send(None);
        return;
    };

    if !window.register_session_notification() {
        let _ = ready.send(None);
        return;
    }

    // Without the power registration the source still reports lock/unlock; suspend/resume is optional.
    let _ = window.register_suspend_resume_notification();

    let _ = ready.send(Some(window.window.0 as isize));

    window.pump_messages();

    // `window` drops here and releases both notifications, the window and the class.
}

fn emit(event: SessionEvent) {
    let sink = SINK.with(|slot| slot.borrow().clone());
    if let Some(sink) = sink {
        sink(event);
    }
}

unsafe extern "system" fn wndproc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_WTSSESSION_CHANGE => {
            if let Some(event) = SessionEventMapper::from_session_switch(wparam.0 as i32) {
                emit(event);
            }
            LRESULT(0)
        }
        WM_POWERBROADCAST => {
            let mode = match wparam.0 as u32 {
                PBT_APMRESUMEAUTOMATIC => 1,
                PBT_APMSUSPEND => 3,
                _ => {
                    // SAFETY: forward everything we do not own to the default procedure.
                    return unsafe { DefWindowProcW(window, message, wparam, lparam) };
                }
            };

            if let Some(event) = SessionEventMapper::from_power_mode(mode) {
                emit(event);
            }
            LRESULT(0)
        }
        _ => {
            // SAFETY: forward everything we do not own to the default procedure.
            unsafe { DefWindowProcW(window, message, wparam, lparam) }
        }
    }
}
