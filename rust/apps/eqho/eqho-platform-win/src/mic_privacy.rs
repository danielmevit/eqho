//! Read-only Windows microphone privacy probe and `ms-settings:` launcher.

use eqho_platform::MicPrivacyProbe;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, REG_SZ, REG_VALUE_TYPE, RegCloseKey,
    RegOpenKeyExW, RegQueryValueExW,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{HSTRING, w};

use crate::pure::{classify_mic_consent, from_utf16_nul};

const MIC_CONSENT_KEY: &str =
    r"SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone";
const MIC_CONSENT_NON_PACKAGED_KEY: &str =
    r"SOFTWARE\Microsoft\Windows\CurrentVersion\CapabilityAccessManager\ConsentStore\microphone\NonPackaged";

struct RegKey(HKEY);

impl Drop for RegKey {
    fn drop(&mut self) {
        // SAFETY: `self.0` was obtained from a successful `RegOpenKeyExW` and is closed once.
        let _ = unsafe { RegCloseKey(self.0) };
    }
}

fn read_consent_value(root: HKEY, subkey: &str) -> Option<String> {
    let subkey = HSTRING::from(subkey);
    let mut handle = HKEY::default();
    // SAFETY: `subkey` is a valid NUL-terminated UTF-16 string and `handle` is a valid out-pointer.
    let status = unsafe { RegOpenKeyExW(root, &subkey, None, KEY_READ, &mut handle) };
    if status.is_err() {
        return None;
    }
    let key = RegKey(handle);

    let value_name = w!("Value");
    let mut value_type = REG_VALUE_TYPE::default();
    let mut size = 0u32;
    // SAFETY: Size-query call passes `None` for the data buffer.
    let status = unsafe {
        RegQueryValueExW(
            key.0,
            value_name,
            None,
            Some(&mut value_type),
            None,
            Some(&mut size),
        )
    };
    if status.is_err() || value_type != REG_SZ || size == 0 {
        return None;
    }

    let mut buf = vec![0u8; size as usize];
    // SAFETY: `buf` has length `size` bytes, matching the `size` out/in parameter.
    let status = unsafe {
        RegQueryValueExW(
            key.0,
            value_name,
            None,
            Some(&mut value_type),
            Some(buf.as_mut_ptr()),
            Some(&mut size),
        )
    };
    if status.is_err() {
        return None;
    }

    let units: Vec<u16> = buf[..size as usize]
        .chunks_exact(2)
        .map(|pair| u16::from_ne_bytes([pair[0], pair[1]]))
        .collect();
    Some(from_utf16_nul(&units))
}

/// Read-only Windows microphone privacy probe.
#[derive(Debug, Default, Clone, Copy)]
pub struct WinMicPrivacyProbe;

impl MicPrivacyProbe for WinMicPrivacyProbe {
    fn mic_access_blocked(&self) -> Option<bool> {
        let user = read_consent_value(HKEY_CURRENT_USER, MIC_CONSENT_KEY);
        let non_packaged = read_consent_value(HKEY_CURRENT_USER, MIC_CONSENT_NON_PACKAGED_KEY);
        let machine = read_consent_value(HKEY_LOCAL_MACHINE, MIC_CONSENT_KEY);
        classify_mic_consent(
            user.as_deref(),
            non_packaged.as_deref(),
            machine.as_deref(),
        )
    }

    fn open_mic_privacy_settings(&self) -> bool {
        // SAFETY: `ShellExecuteW` receives static NUL-terminated UTF-16 literals for `open` and
        // `ms-settings:privacy-microphone`. Return values > 32 indicate success.
        let hinst = unsafe {
            ShellExecuteW(
                None,
                w!("open"),
                w!("ms-settings:privacy-microphone"),
                None,
                None,
                SW_SHOWNORMAL,
            )
        };
        (hinst.0 as isize) > 32
    }
}
