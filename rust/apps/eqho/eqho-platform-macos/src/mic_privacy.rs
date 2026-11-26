//! macOS microphone authorization probe (`AVCaptureDevice`) and System Settings deep-link.

use std::ffi::{c_char, c_void};
use std::process::{Command, Stdio};

use eqho_platform::MicPrivacyProbe;

#[link(name = "AVFoundation", kind = "framework")]
unsafe extern "C" {}

#[link(name = "Foundation", kind = "framework")]
unsafe extern "C" {}

#[link(name = "objc")]
unsafe extern "C" {
    fn objc_getClass(name: *const c_char) -> *mut c_void;
    fn sel_registerName(name: *const c_char) -> *mut c_void;
    fn objc_msgSend();
}

/// Read-only macOS microphone privacy status probe and System Settings opener.
#[derive(Debug, Default, Clone, Copy)]
pub struct MacMicPrivacyProbe;

impl MicPrivacyProbe for MacMicPrivacyProbe {
    fn mic_access_blocked(&self) -> Option<bool> {
        // SAFETY: Calling read-only `+[AVCaptureDevice authorizationStatusForMediaType:@"soun"]`.
        unsafe {
            let av_cls = objc_getClass(c"AVCaptureDevice".as_ptr());
            let ns_str_cls = objc_getClass(c"NSString".as_ptr());
            if av_cls.is_null() || ns_str_cls.is_null() {
                return None;
            }
            type MsgStr = unsafe extern "C" fn(*mut c_void, *mut c_void, *const c_char) -> *mut c_void;
            type MsgAuth = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> isize;
            let send_str: MsgStr = std::mem::transmute(objc_msgSend as *const ());
            let send_auth: MsgAuth = std::mem::transmute(objc_msgSend as *const ());

            // `AVMediaTypeAudio` constant string value is `"soun"`.
            let media_audio = send_str(
                ns_str_cls,
                sel_registerName(c"stringWithUTF8String:".as_ptr()),
                c"soun".as_ptr(),
            );
            if media_audio.is_null() {
                return None;
            }
            let status = send_auth(
                av_cls,
                sel_registerName(c"authorizationStatusForMediaType:".as_ptr()),
                media_audio,
            );
            match status {
                1 | 2 => Some(true), // Restricted | Denied
                3 => Some(false),    // Authorized
                _ => None,           // NotDetermined (0)
            }
        }
    }

    fn open_mic_privacy_settings(&self) -> bool {
        Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .is_ok()
    }
}
