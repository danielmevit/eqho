//! macOS `NSPasteboard` clipboard service with concealed/transient history opt-out.

use std::ffi::{CStr, CString, c_char, c_void};

use eqho_platform::{ClipboardService, PlatformError};

#[link(name = "AppKit", kind = "framework")]
unsafe extern "C" {}

#[link(name = "objc")]
unsafe extern "C" {
    fn objc_getClass(name: *const c_char) -> *mut c_void;
    fn sel_registerName(name: *const c_char) -> *mut c_void;
    fn objc_msgSend();
}

/// Native `NSPasteboard` clipboard supporting `org.nspasteboard.TransientType` / `ConcealedType`.
#[derive(Debug, Default, Clone, Copy)]
pub struct MacClipboardService;

impl MacClipboardService {
    unsafe fn ns_string(s: &CStr) -> *mut c_void {
        // SAFETY: Creating an autoreleased `NSString` from a valid C string.
        unsafe {
            let cls = objc_getClass(c"NSString".as_ptr());
            type MsgStr = unsafe extern "C" fn(*mut c_void, *mut c_void, *const c_char) -> *mut c_void;
            let send: MsgStr =
                std::mem::transmute::<*const (), MsgStr>(objc_msgSend as *const ());
            send(cls, sel_registerName(c"stringWithUTF8String:".as_ptr()), s.as_ptr())
        }
    }
}

impl ClipboardService for MacClipboardService {
    fn get_text(&self) -> Option<String> {
        // SAFETY: Reading `NSPasteboardTypeString` from `NSPasteboard.generalPasteboard`.
        unsafe {
            let pb_cls = objc_getClass(c"NSPasteboard".as_ptr());
            if pb_cls.is_null() {
                return None;
            }
            type Msg0 = unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void;
            type Msg1 = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> *mut c_void;
            type MsgCstr = unsafe extern "C" fn(*mut c_void, *mut c_void) -> *const c_char;
            let send0: Msg0 = std::mem::transmute::<*const (), Msg0>(objc_msgSend as *const ());
            let send1: Msg1 = std::mem::transmute::<*const (), Msg1>(objc_msgSend as *const ());
            let send_cstr: MsgCstr =
                std::mem::transmute::<*const (), MsgCstr>(objc_msgSend as *const ());

            let pb = send0(pb_cls, sel_registerName(c"generalPasteboard".as_ptr()));
            if pb.is_null() {
                return None;
            }
            let type_str = Self::ns_string(c"public.utf8-plain-text");
            let ns_val = send1(pb, sel_registerName(c"stringForType:".as_ptr()), type_str);
            if ns_val.is_null() {
                return None;
            }
            let cptr = send_cstr(ns_val, sel_registerName(c"UTF8String".as_ptr()));
            if cptr.is_null() {
                return None;
            }
            Some(CStr::from_ptr(cptr).to_string_lossy().into_owned())
        }
    }

    fn set_text(&self, text: &str, private: bool) -> Result<(), PlatformError> {
        let clean = text.replace('\0', "");
        let ctext = CString::new(clean).map_err(|e| PlatformError(e.to_string()))?;

        // SAFETY: Writing UTF-8 text (and optional concealed/transient markers) to `NSPasteboard`.
        unsafe {
            let pb_cls = objc_getClass(c"NSPasteboard".as_ptr());
            if pb_cls.is_null() {
                return Err(PlatformError("NSPasteboard class unavailable".to_owned()));
            }
            type Msg0 = unsafe extern "C" fn(*mut c_void, *mut c_void) -> *mut c_void;
            type MsgClear = unsafe extern "C" fn(*mut c_void, *mut c_void) -> isize;
            type MsgSet2 =
                unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *mut c_void) -> u8;

            let send0: Msg0 = std::mem::transmute::<*const (), Msg0>(objc_msgSend as *const ());
            let send_clear: MsgClear =
                std::mem::transmute::<*const (), MsgClear>(objc_msgSend as *const ());
            let send_set2: MsgSet2 =
                std::mem::transmute::<*const (), MsgSet2>(objc_msgSend as *const ());

            let pb = send0(pb_cls, sel_registerName(c"generalPasteboard".as_ptr()));
            if pb.is_null() {
                return Err(PlatformError("generalPasteboard returned null".to_owned()));
            }
            let _ = send_clear(pb, sel_registerName(c"clearContents".as_ptr()));

            let val = Self::ns_string(&ctext);
            let utf8_type = Self::ns_string(c"public.utf8-plain-text");
            let ok = send_set2(
                pb,
                sel_registerName(c"setString:forType:".as_ptr()),
                val,
                utf8_type,
            );
            if ok == 0 {
                return Err(PlatformError("NSPasteboard setString:forType: failed".to_owned()));
            }

            if private {
                let empty = Self::ns_string(c"");
                let transient = Self::ns_string(c"org.nspasteboard.TransientType");
                let concealed = Self::ns_string(c"org.nspasteboard.ConcealedType");
                let _ = send_set2(
                    pb,
                    sel_registerName(c"setString:forType:".as_ptr()),
                    empty,
                    transient,
                );
                let _ = send_set2(
                    pb,
                    sel_registerName(c"setString:forType:".as_ptr()),
                    empty,
                    concealed,
                );
            }
        }
        Ok(())
    }
}
