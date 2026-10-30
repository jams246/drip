#![expect(
    unsafe_code,
    reason = "Windows ACL APIs require audited pointer boundaries"
)]

use std::{fs::File, io, os::windows::io::AsRawHandle, path::Path, ptr};

use windows::Win32::{
    Foundation::{HANDLE, HLOCAL, LocalFree},
    Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        GetSecurityInfo, SDDL_REVISION_1, SE_FILE_OBJECT, SetSecurityInfo,
    },
    Security::{
        ACL, DACL_SECURITY_INFORMATION, GetSecurityDescriptorControl, GetSecurityDescriptorDacl,
        OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
        PSID, SE_DACL_PROTECTED,
    },
};
use windows::core::{BOOL, PCWSTR, PWSTR};

pub(crate) fn secure_file(path: &Path) -> io::Result<()> {
    crate::private_file::open(path, false, false).map(drop)
}

pub(crate) fn secure_handle(file: &File) -> io::Result<()> {
    validate_owner(file)?;
    let sddl: Vec<u16> = "D:P(A;;FA;;;OW)(A;;FA;;;SY)(A;;FA;;;BA)\0"
        .encode_utf16()
        .collect();
    let mut descriptor = SecurityDescriptor(PSECURITY_DESCRIPTOR(ptr::null_mut()));
    // SAFETY: the SDDL string is NUL terminated and the output pointer is writable.
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(sddl.as_ptr()),
            SDDL_REVISION_1,
            &mut descriptor.0,
            None,
        )
    }
    .map_err(io::Error::other)?;

    let mut present = BOOL(0);
    let mut defaulted = BOOL(0);
    let mut dacl: *mut ACL = ptr::null_mut();
    // SAFETY: descriptor is a valid Windows security descriptor returned above.
    unsafe { GetSecurityDescriptorDacl(descriptor.0, &mut present, &mut dacl, &mut defaulted) }
        .map_err(io::Error::other)?;
    if !present.as_bool() || dacl.is_null() {
        return Err(io::Error::other(
            "Windows returned an empty credentials DACL",
        ));
    }
    if has_private_dacl(file, dacl)? {
        return Ok(());
    }
    let security_info = DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION;
    // SAFETY: the file handle and `dacl` stay live; DACL grants owner, SYSTEM, and administrators.
    let code = unsafe {
        SetSecurityInfo(
            HANDLE(file.as_raw_handle()),
            SE_FILE_OBJECT,
            security_info,
            None,
            None,
            Some(dacl.cast_const()),
            None,
        )
    };
    if code.0 != 0 {
        return Err(io::Error::from_raw_os_error(code.0 as i32));
    }
    Ok(())
}

fn has_private_dacl(file: &File, expected: *const ACL) -> io::Result<bool> {
    let mut descriptor = SecurityDescriptor(PSECURITY_DESCRIPTOR::default());
    let mut actual = ptr::null_mut();
    // SAFETY: the live file handle and writable outputs remain valid during the call.
    unsafe {
        GetSecurityInfo(
            HANDLE(file.as_raw_handle()),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(&mut actual),
            None,
            Some(&mut descriptor.0),
        )
    }
    .ok()
    .map_err(io::Error::other)?;
    let mut control = 0;
    let mut revision = 0;
    // SAFETY: Windows returned a valid descriptor owned by this scope.
    unsafe { GetSecurityDescriptorControl(descriptor.0, &mut control, &mut revision) }
        .map_err(io::Error::other)?;
    if control & SE_DACL_PROTECTED.0 == 0 || actual.is_null() {
        return Ok(false);
    }
    // SAFETY: both ACLs came from live Windows descriptors and include their allocated size.
    Ok(unsafe {
        std::slice::from_raw_parts(actual.cast::<u8>(), usize::from((*actual).AclSize))
            == std::slice::from_raw_parts(expected.cast::<u8>(), usize::from((*expected).AclSize))
    })
}

fn validate_owner(file: &File) -> io::Result<()> {
    let mut descriptor = SecurityDescriptor(PSECURITY_DESCRIPTOR::default());
    let mut owner = PSID::default();
    // SAFETY: the live file handle is readable and both output pointers are writable.
    unsafe {
        GetSecurityInfo(
            HANDLE(file.as_raw_handle()),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION,
            Some(&mut owner),
            None,
            None,
            None,
            Some(&mut descriptor.0),
        )
    }
    .ok()
    .map_err(io::Error::other)?;
    let mut owner_text = PWSTR::null();
    // SAFETY: GetSecurityInfo returned a SID owned by the live descriptor.
    unsafe { ConvertSidToStringSidW(owner, &mut owner_text) }.map_err(io::Error::other)?;
    // SAFETY: Windows returned a NUL-terminated allocation; it is released after copying.
    let owner = unsafe {
        let owner = PCWSTR(owner_text.0).to_string();
        let _ = LocalFree(Some(HLOCAL(owner_text.0.cast())));
        owner
    }
    .map_err(io::Error::other)?;
    let current_user = crate::windows_integration::current_user_sid()?;
    if !trusted_owner(&owner, &current_user) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "private state belongs to another Windows account",
        ));
    }
    Ok(())
}

fn trusted_owner(owner: &str, current_user: &str) -> bool {
    owner == current_user || matches!(owner, "S-1-5-18" | "S-1-5-32-544")
}

struct SecurityDescriptor(PSECURITY_DESCRIPTOR);

impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        if !(self.0).0.is_null() {
            // SAFETY: conversion API allocated this descriptor, and Drop releases it once.
            unsafe {
                let _ = LocalFree(Some(HLOCAL((self.0).0)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn private_state_accepts_only_current_user_and_system_administration_owners() {
        for owner in ["S-1-5-21-1", "S-1-5-18", "S-1-5-32-544"] {
            assert!(super::trusted_owner(owner, "S-1-5-21-1"));
        }
        assert!(!super::trusted_owner("S-1-5-21-2", "S-1-5-21-1"));
    }
}
