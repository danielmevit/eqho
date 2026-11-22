//! Single-instance acquisition with a signal channel.
//!
//! Windows uses a named mutex `Local\<app_id>` and an auto-reset event `Local\<app_id>.signal`, exactly the
//! names the C# 0.1 app uses. For Ocy, `app_id` is `xyz.damt.ocy`, so the C# and Rust builds exclude each
//! other during the migration.
//!
//! Non-Windows targets use a lock file in the app's data directory.
//!
//! On Windows the acquiring thread owns the mutex, so drop the guard on the same thread that acquired it.

#[cfg(not(windows))]
use std::path::Path;

/// Attempt to become the single running instance.
#[derive(Debug)]
pub enum Acquire {
    /// This process is the first instance; keep the guard for as long as the app runs.
    First(SingleInstanceGuard),
    /// Another instance owns the mutex or lock file. On Windows the running instance was signalled.
    AlreadyRunning,
}

/// The single-instance API. All methods are associated functions; the type is never instantiated.
#[derive(Debug, Default, Clone, Copy)]
pub struct SingleInstance;

impl SingleInstance {
    /// Try to acquire `app_id`.
    ///
    /// On Windows, when another instance is running, its `Local\<app_id>.signal` event is set before this
    /// returns [`Acquire::AlreadyRunning`].
    pub fn acquire(app_id: &str) -> Acquire {
        match platform::try_acquire(app_id) {
            Some(inner) => Acquire::First(SingleInstanceGuard { inner }),
            None => Acquire::AlreadyRunning,
        }
    }

    /// Acquire an instance using an explicit lock-file directory.
    ///
    /// This is the Unix path used by tests and embedders; on Windows use [`acquire`](Self::acquire).
    #[cfg(not(windows))]
    pub fn acquire_in_dir(data_dir: impl AsRef<Path>, app_id: &str) -> Acquire {
        match nix::try_acquire_in_dir(data_dir.as_ref(), app_id) {
            Some(inner) => Acquire::First(SingleInstanceGuard { inner }),
            None => Acquire::AlreadyRunning,
        }
    }
}

/// The owned first-instance guard.
///
/// Dropping it stops the signal listener (if any) and releases the mutex or lock file.
#[derive(Debug)]
pub struct SingleInstanceGuard {
    inner: platform::Guard,
}

impl SingleInstanceGuard {
    /// Wait for signals from later launches on a background thread and call `callback` for each one.
    ///
    /// The callback runs on that background thread; the app is responsible for marshalling to its UI thread.
    /// Calling this more than once is a no-op. On non-Windows targets the signal channel is not available
    /// today, so this is a no-op.
    pub fn listen<F>(&mut self, callback: F)
    where
        F: Fn() + Send + 'static,
    {
        self.inner.listen(Box::new(callback));
    }
}

#[cfg(windows)]
use win as platform;

#[cfg(not(windows))]
use nix as platform;

#[cfg(windows)]
mod win {
    use std::ffi::c_void;
    use std::time::{Duration, Instant};

    use windows::Win32::Foundation::{
        CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, WAIT_OBJECT_0,
    };
    use windows::Win32::System::Threading::{
        CreateEventW, CreateMutexW, EVENT_MODIFY_STATE, INFINITE, OpenEventW, ReleaseMutex,
        SetEvent, WaitForMultipleObjects,
    };
    use windows::core::PCWSTR;

    /// The owned Windows first-instance state.
    #[derive(Debug)]
    pub(super) struct Guard {
        mutex: HANDLE,
        signal: Option<HANDLE>,
        listener: Option<Listener>,
    }

    /// A background thread waiting for the signal event, plus the event that stops it.
    #[derive(Debug)]
    struct Listener {
        thread: Option<std::thread::JoinHandle<()>>,
        stop: HANDLE,
    }

    pub(super) fn try_acquire(app_id: &str) -> Option<Guard> {
        let mutex_name = wide(&format!("Local\\{app_id}"));
        let event_name = wide(&format!("Local\\{app_id}.signal"));

        // SAFETY: both names are null-terminated, live for the duration of the calls and are only read.
        let signal = unsafe { CreateEventW(None, false, false, PCWSTR(event_name.as_ptr())) }.ok();

        // SAFETY: the name is null-terminated and lives for the call. `binitialowner = true` makes this
        // thread the owner when the mutex is created.
        let mutex = match unsafe { CreateMutexW(None, true, PCWSTR(mutex_name.as_ptr())) } {
            Ok(mutex) => mutex,
            Err(_) => {
                close(signal);
                return None;
            }
        };

        // SAFETY: GetLastError is thread-local; read it immediately after CreateMutexW.
        let already_exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;

        if already_exists {
            signal_running_instance(signal, &event_name);
            // SAFETY: the mutex handle is valid but this thread does not own it.
            unsafe {
                let _ = CloseHandle(mutex);
            }
            close(signal);
            return None;
        }

        let signal = signal.or_else(|| {
            // SAFETY: the name is null-terminated and lives for the call.
            unsafe { CreateEventW(None, false, false, PCWSTR(event_name.as_ptr())) }.ok()
        });

        Some(Guard {
            mutex,
            signal,
            listener: None,
        })
    }

    impl Guard {
        pub(super) fn listen(&mut self, callback: Box<dyn Fn() + Send>) {
            if self.listener.is_some() {
                return;
            }

            let Some(signal) = self.signal else {
                return;
            };

            // SAFETY: a null security attribute and null name create an anonymous manual-reset event.
            let stop = match unsafe { CreateEventW(None, true, false, PCWSTR::null()) } {
                Ok(stop) => stop,
                Err(_) => return,
            };

            let signal_raw = signal.0 as isize;
            let stop_raw = stop.0 as isize;

            let thread = std::thread::Builder::new()
                .name(String::from("damt-single-instance"))
                .spawn(move || {
                    let signal = HANDLE(signal_raw as *mut c_void);
                    let stop = HANDLE(stop_raw as *mut c_void);

                    loop {
                        // SAFETY: both events stay valid until Guard::drop sets `stop` and joins this
                        // thread.
                        let result =
                            unsafe { WaitForMultipleObjects(&[signal, stop], false, INFINITE) };

                        if result == WAIT_OBJECT_0 {
                            callback();
                        } else {
                            break;
                        }
                    }
                });

            match thread {
                Ok(thread) => {
                    self.listener = Some(Listener {
                        thread: Some(thread),
                        stop,
                    });
                }
                Err(_) => close(Some(stop)),
            }
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            if let Some(mut listener) = self.listener.take() {
                // SAFETY: `stop` is an event owned by this listener.
                unsafe {
                    let _ = SetEvent(listener.stop);
                }
                if let Some(thread) = listener.thread.take() {
                    let _ = thread.join();
                }
                // SAFETY: the listener thread has joined and no longer uses the event.
                unsafe {
                    let _ = CloseHandle(listener.stop);
                }
            }

            close(self.signal.take());

            // SAFETY: both handles are valid and owned by this guard. ReleaseMutex is best effort.
            unsafe {
                let _ = ReleaseMutex(self.mutex);
                let _ = CloseHandle(self.mutex);
            }
        }
    }

    /// Signal the running instance, either through the handle we already opened or by retrying `OpenEventW`
    /// for up to one second (the C# `TryOpenExisting` loop).
    fn signal_running_instance(event: Option<HANDLE>, name: &[u16]) {
        if let Some(event) = event {
            // SAFETY: `event` is a valid event handle opened or created above.
            unsafe {
                let _ = SetEvent(event);
            }
            return;
        }

        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            // SAFETY: `name` is null-terminated and lives for the call.
            let opened = unsafe { OpenEventW(EVENT_MODIFY_STATE, false, PCWSTR(name.as_ptr())) };

            if let Ok(event) = opened {
                // SAFETY: `event` is a valid event handle opened above.
                unsafe {
                    let _ = SetEvent(event);
                    let _ = CloseHandle(event);
                }
                return;
            }

            if Instant::now() >= deadline {
                return;
            }

            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn close(handle: Option<HANDLE>) {
        if let Some(handle) = handle {
            // SAFETY: the handle is owned by the caller and closed exactly once.
            unsafe {
                let _ = CloseHandle(handle);
            }
        }
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }
}

#[cfg(not(windows))]
mod nix {
    use std::fs::{File, OpenOptions};
    use std::path::Path;

    /// The owned Unix first-instance state; dropping the file releases the advisory lock.
    #[derive(Debug)]
    pub(super) struct Guard {
        _lock_file: File,
    }

    impl Guard {
        pub(super) fn listen(&mut self, _callback: Box<dyn Fn() + Send>) {
            // The Unix signal channel is not implemented yet; single-instance exclusion still works.
        }
    }

    pub(super) fn try_acquire(app_id: &str) -> Option<Guard> {
        let app_name = app_name_from_id(app_id);
        let data_dir = crate::AppDataPaths::new(app_name).data_dir();
        try_acquire_in_dir(&data_dir, app_id)
    }

    pub(super) fn try_acquire_in_dir(data_dir: &Path, app_id: &str) -> Option<Guard> {
        std::fs::create_dir_all(data_dir).ok()?;

        let path = data_dir.join(lock_file_name(app_id));
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .ok()?;

        if file.try_lock().is_ok() {
            Some(Guard { _lock_file: file })
        } else {
            None
        }
    }

    /// The last dot-separated app id component, matching the C# Ocy data directory.
    fn app_name_from_id(app_id: &str) -> &str {
        app_id
            .rsplit('.')
            .next()
            .filter(|component| !component.is_empty())
            .unwrap_or(app_id)
    }

    fn lock_file_name(app_id: &str) -> String {
        format!(".{}.lock", app_name_from_id(app_id).to_lowercase())
    }
}
