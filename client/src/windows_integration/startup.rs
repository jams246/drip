#![expect(
    unsafe_code,
    reason = "Windows registry and ACL APIs require audited pointer boundaries"
)]

use std::io;

use windows::{
    Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA},
        System::Registry::{
            HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE,
            REG_VALUE_TYPE, RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW,
            RegQueryValueExW, RegSetValueExW,
        },
    },
    core::PCWSTR,
};

#[cfg(test)]
mod test_registry;
#[cfg(test)]
pub(crate) use test_registry::{create_readonly_key, remove_key};

pub(crate) const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
pub(crate) const RUN_VALUE: &str = "Drip";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RegistryValue {
    pub kind: REG_VALUE_TYPE,
    pub data: Vec<u8>,
}

pub(crate) fn read_value(key: &str, name: &str) -> io::Result<Option<RegistryValue>> {
    let Some(key) = open_key(key, false, false)? else {
        return Ok(None);
    };
    let key = RegistryKey(key);
    let name = wide(name);
    let mut kind = REG_VALUE_TYPE(0);
    let mut size = 0;
    // SAFETY: the open key and NUL-terminated value name stay live; outputs are writable.
    let status = unsafe {
        RegQueryValueExW(
            key.0,
            PCWSTR(name.as_ptr()),
            None,
            Some(&mut kind),
            None,
            Some(&mut size),
        )
    };
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(None);
    }
    check(status)?;
    if size > 1_048_576 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "startup registry value exceeds 1 MiB",
        ));
    }
    let mut data = vec![0; size as usize];
    // SAFETY: `data` has the size reported by the first query and output size is writable.
    let status = unsafe {
        RegQueryValueExW(
            key.0,
            PCWSTR(name.as_ptr()),
            None,
            Some(&mut kind),
            Some(data.as_mut_ptr()),
            Some(&mut size),
        )
    };
    if status == ERROR_MORE_DATA {
        return Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "startup registry value changed while being read",
        ));
    }
    check(status)?;
    data.truncate(size as usize);
    Ok(Some(RegistryValue { kind, data }))
}

pub(crate) fn write_value(key: &str, name: &str, value: Option<&RegistryValue>) -> io::Result<()> {
    let Some(key) = open_key(key, value.is_some(), true)? else {
        return Ok(());
    };
    let key = RegistryKey(key);
    let name = wide(name);
    let status = match value {
        Some(value) => {
            // SAFETY: the key, value name, and byte slice stay live for the call.
            unsafe {
                RegSetValueExW(
                    key.0,
                    PCWSTR(name.as_ptr()),
                    None,
                    value.kind,
                    Some(&value.data),
                )
            }
        }
        None => {
            // SAFETY: the open key and NUL-terminated value name stay live for the call.
            unsafe { RegDeleteValueW(key.0, PCWSTR(name.as_ptr())) }
        }
    };
    if value.is_none() && status == ERROR_FILE_NOT_FOUND {
        return Ok(());
    }
    check(status)
}

fn open_key(path: &str, create: bool, write: bool) -> io::Result<Option<HKEY>> {
    let path = wide(path);
    let mut key = HKEY::default();
    let access = if write {
        KEY_QUERY_VALUE | KEY_SET_VALUE
    } else {
        KEY_QUERY_VALUE
    };
    let status = if create {
        // SAFETY: HKCU and the NUL-terminated key path are valid; `key` is writable.
        unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(path.as_ptr()),
                None,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                access,
                None,
                &mut key,
                None,
            )
        }
    } else {
        // SAFETY: HKCU and the NUL-terminated key path are valid; `key` is writable.
        unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(path.as_ptr()),
                None,
                access,
                &mut key,
            )
        }
    };
    if !create && status == ERROR_FILE_NOT_FOUND {
        Ok(None)
    } else {
        check(status)?;
        Ok(Some(key))
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(Some(0)).collect()
}

fn check(status: windows::Win32::Foundation::WIN32_ERROR) -> io::Result<()> {
    if status.0 == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(status.0 as i32))
    }
}

struct RegistryKey(HKEY);

impl Drop for RegistryKey {
    fn drop(&mut self) {
        // SAFETY: this wrapper owns the key returned by RegOpenKeyExW or RegCreateKeyExW.
        unsafe {
            let _ = RegCloseKey(self.0);
        }
    }
}
