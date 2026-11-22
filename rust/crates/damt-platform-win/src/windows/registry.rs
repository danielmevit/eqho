//! Small, shared wrappers around the per-user registry reads and writes used by the platform services.

use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, WIN32_ERROR};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_ENUMERATE_SUB_KEYS, KEY_QUERY_VALUE, KEY_WRITE, REG_DWORD,
    REG_SAM_FLAGS, REG_SZ, REG_VALUE_TYPE, RegCloseKey, RegCreateKeyW, RegDeleteValueW,
    RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
use windows::core::HSTRING;

use crate::RegistryValue;

/// `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// `HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize`.
pub const PERSONALIZE_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";

/// An owned registry key handle that closes itself when dropped.
#[derive(Debug)]
pub struct RegKey(HKEY);

impl RegKey {
    fn handle(&self) -> HKEY {
        self.0
    }
}

impl Drop for RegKey {
    fn drop(&mut self) {
        // SAFETY: the handle came from a successful open/create and is closed exactly once.
        let _ = unsafe { RegCloseKey(self.0) };
    }
}

/// Opens a key under `HKEY_CURRENT_USER`; `None` when it does not exist or cannot be opened.
pub fn open_key(path: &str, access: REG_SAM_FLAGS) -> Option<RegKey> {
    let path = HSTRING::from(path);
    let mut key = HKEY::default();

    // SAFETY: the subkey is a valid null-terminated UTF-16 string and `key` is a valid out-parameter.
    let status = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, &path, None, access, &mut key) };
    status.is_ok().then_some(RegKey(key))
}

/// Creates or opens a subkey under `HKEY_CURRENT_USER` for writing; `None` on failure.
pub fn create_key(path: &str) -> Option<RegKey> {
    let path = HSTRING::from(path);
    let mut key = HKEY::default();

    // SAFETY: the subkey is a valid null-terminated UTF-16 string and `key` is a valid out-parameter.
    let status = unsafe { RegCreateKeyW(HKEY_CURRENT_USER, &path, &mut key) };
    status.is_ok().then_some(RegKey(key))
}

/// Creates or opens the Run key for writing; `None` on failure.
pub fn create_run_key() -> Option<RegKey> {
    create_key(RUN_KEY)
}

/// Reads a `REG_SZ` value, trimming the trailing NUL. `None` when missing or a different type.
pub fn read_string(key: &RegKey, name: &str) -> Option<String> {
    let name = HSTRING::from(name);
    let mut value_type = REG_VALUE_TYPE::default();
    let mut size = 0u32;

    // SAFETY: `name` is valid; the size query passes no data buffer.
    let status = unsafe {
        RegQueryValueExW(
            key.handle(),
            &name,
            None,
            Some(&mut value_type),
            None,
            Some(&mut size),
        )
    };
    if status.is_err() || value_type != REG_SZ || size == 0 {
        return None;
    }

    let mut buffer = vec![0u8; size as usize];
    // SAFETY: `buffer` is `size` bytes long and `size` is passed as its capacity.
    let status = unsafe {
        RegQueryValueExW(
            key.handle(),
            &name,
            None,
            Some(&mut value_type),
            Some(buffer.as_mut_ptr()),
            Some(&mut size),
        )
    };
    if status.is_err() {
        return None;
    }

    let units: Vec<u16> = buffer[..size as usize]
        .chunks_exact(2)
        .map(|pair| u16::from_ne_bytes([pair[0], pair[1]]))
        .collect();
    Some(
        String::from_utf16_lossy(&units)
            .trim_end_matches('\0')
            .to_owned(),
    )
}

/// Reads a DWORD value, or reports that the value exists with another type.
pub fn read_registry_value(key: &RegKey, name: &str) -> Option<RegistryValue> {
    let name = HSTRING::from(name);
    let mut value_type = REG_VALUE_TYPE::default();
    let mut data = [0u8; 4];
    let mut size = data.len() as u32;

    // SAFETY: `data` has room for exactly one DWORD and `size` describes it.
    let status = unsafe {
        RegQueryValueExW(
            key.handle(),
            &name,
            None,
            Some(&mut value_type),
            Some(data.as_mut_ptr()),
            Some(&mut size),
        )
    };
    if status.is_err() {
        return None;
    }

    if value_type == REG_DWORD && size == data.len() as u32 {
        Some(RegistryValue::Dword(u32::from_ne_bytes(data)))
    } else {
        Some(RegistryValue::Other)
    }
}

/// Writes a `REG_SZ` value.
pub fn write_string(key: &RegKey, name: &str, value: &str) -> Result<(), WIN32_ERROR> {
    let name = HSTRING::from(name);
    let units: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes: Vec<u8> = units.iter().flat_map(|unit| unit.to_ne_bytes()).collect();

    // SAFETY: `name` is valid and `bytes` is a byte view of a NUL-terminated UTF-16 string.
    let status = unsafe { RegSetValueExW(key.handle(), &name, None, REG_SZ, Some(&bytes)) };
    if status.is_ok() { Ok(()) } else { Err(status) }
}

/// Deletes a value, treating "already missing" as success.
pub fn delete_value(key: &RegKey, name: &str) -> Result<(), WIN32_ERROR> {
    let name = HSTRING::from(name);

    // SAFETY: `name` is a valid null-terminated UTF-16 string.
    let status = unsafe { RegDeleteValueW(key.handle(), &name) };
    if status == ERROR_FILE_NOT_FOUND || status.is_ok() {
        Ok(())
    } else {
        Err(status)
    }
}

/// Access mask used by [`open_key`] when deleting the Run value.
pub const WRITE_ACCESS: REG_SAM_FLAGS = KEY_WRITE;

/// Access mask used by [`open_key`] when reading the Run value.
pub const READ_ACCESS: REG_SAM_FLAGS = KEY_QUERY_VALUE;

/// Access mask for listing subkeys and reading their values. Never includes write access.
///
/// `BitOr` for `REG_SAM_FLAGS` is not const, so the bits are combined directly:
/// `KEY_ENUMERATE_SUB_KEYS` (8) and `KEY_QUERY_VALUE` (1).
pub const ENUM_READ_ACCESS: REG_SAM_FLAGS =
    REG_SAM_FLAGS(KEY_ENUMERATE_SUB_KEYS.0 | KEY_QUERY_VALUE.0);

/// Lists subkey names under `key`. Stops at the first error or after 4096 names.
pub fn enum_subkey_names(key: &RegKey) -> Vec<String> {
    use windows::Win32::Foundation::ERROR_NO_MORE_ITEMS;
    use windows::Win32::System::Registry::RegEnumKeyExW;
    use windows::core::PWSTR;

    let mut names = Vec::new();
    for index in 0..4096u32 {
        let mut buffer = [0u16; 256];
        let mut len = buffer.len() as u32;
        // SAFETY: `buffer` is a writable UTF-16 buffer and `len` is its capacity in characters,
        // including space for a trailing NUL. The call only reads this key.
        let status = unsafe {
            RegEnumKeyExW(
                key.handle(),
                index,
                Some(PWSTR(buffer.as_mut_ptr())),
                &mut len,
                None,
                None,
                None,
                None,
            )
        };
        if status == ERROR_NO_MORE_ITEMS {
            break;
        }
        if status.is_err() {
            break;
        }
        let count = usize::try_from(len).unwrap_or(0).min(buffer.len());
        names.push(String::from_utf16_lossy(&buffer[..count]));
    }
    names
}

/// Lists value names under `key`. Stops at the first error or after 256 names.
pub fn enum_value_names(key: &RegKey) -> Vec<String> {
    use windows::Win32::Foundation::ERROR_NO_MORE_ITEMS;
    use windows::Win32::System::Registry::RegEnumValueW;
    use windows::core::PWSTR;

    let mut names = Vec::new();
    for index in 0..256u32 {
        let mut buffer = [0u16; 256];
        let mut len = buffer.len() as u32;
        // SAFETY: `buffer` is a writable UTF-16 buffer and `len` is its capacity in characters.
        // No data buffer is requested; the call only reads value names.
        let status = unsafe {
            RegEnumValueW(
                key.handle(),
                index,
                Some(PWSTR(buffer.as_mut_ptr())),
                &mut len,
                None,
                None,
                None,
                None,
            )
        };
        if status == ERROR_NO_MORE_ITEMS {
            break;
        }
        if status.is_err() {
            break;
        }
        let count = usize::try_from(len).unwrap_or(0).min(buffer.len());
        names.push(String::from_utf16_lossy(&buffer[..count]));
    }
    names
}
