#![expect(
    unsafe_code,
    reason = "Synthetic registry ACL tests require raw Windows security attributes"
)]

use std::{io, ptr};

use windows::{
    Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, HLOCAL, LocalFree},
        Security::Authorization::{
            ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
        },
        Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES},
        System::Registry::{
            HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, REG_OPTION_NON_VOLATILE, RegCloseKey,
            RegCreateKeyExW, RegDeleteKeyW,
        },
    },
    core::PCWSTR,
};

pub(crate) fn remove_key(key: &str) -> io::Result<()> {
    let key = super::wide(key);
    // SAFETY: this removes only the supplied synthetic leaf test subkey under HKCU.
    let status = unsafe { RegDeleteKeyW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr())) };
    if status == ERROR_FILE_NOT_FOUND {
        Ok(())
    } else {
        super::check(status)
    }
}

pub(crate) fn create_readonly_key(key: &str) -> io::Result<()> {
    let sid = super::super::identity::current_user_sid()?;
    let sddl = super::wide(&format!("D:P(A;;KR;;;{sid})(A;;SD;;;{sid})"));
    let mut descriptor = SecurityDescriptor(PSECURITY_DESCRIPTOR(ptr::null_mut()));
    // SAFETY: `sddl` is NUL terminated and `descriptor` is writable.
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl.as_ptr()),
            SDDL_REVISION_1,
            &mut descriptor.0,
            None,
        )
    }
    .map_err(io::Error::other)?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor.0.0,
        bInheritHandle: false.into(),
    };
    let path = super::wide(key);
    let mut handle = HKEY::default();
    // SAFETY: the security attributes and path stay live; the output handle is writable.
    let status = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(path.as_ptr()),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_QUERY_VALUE,
            Some(&attributes),
            &mut handle,
            None,
        )
    };
    super::check(status)?;
    // SAFETY: this wrapper owns the key returned by RegCreateKeyExW.
    unsafe {
        let _ = RegCloseKey(handle);
    }
    Ok(())
}

struct SecurityDescriptor(PSECURITY_DESCRIPTOR);

impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        if !self.0.0.is_null() {
            // SAFETY: Windows allocated this descriptor for LocalFree.
            unsafe {
                let _ = LocalFree(Some(HLOCAL(self.0.0)));
            }
        }
    }
}
