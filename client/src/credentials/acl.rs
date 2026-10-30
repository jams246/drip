#![expect(
    unsafe_code,
    reason = "Windows ACL APIs require audited pointer boundaries"
)]

use std::{fs::File, io, os::windows::io::AsRawHandle, path::Path, ptr};

use windows::Win32::{
    Foundation::{HANDLE, HLOCAL, LocalFree},
    Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1, SE_FILE_OBJECT,
        SetSecurityInfo,
    },
    Security::{
        ACL, DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl,
        PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
    },
};
use windows::core::{BOOL, PCWSTR};

pub(crate) fn secure_file(path: &Path) -> io::Result<()> {
    crate::private_file::open(path, false, false).map(drop)
}

pub(crate) fn secure_handle(file: &File) -> io::Result<()> {
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
