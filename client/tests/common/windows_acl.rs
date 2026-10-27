#![expect(
    unsafe_code,
    reason = "Windows ACL test setup uses native security APIs"
)]

use std::{io, os::windows::ffi::OsStrExt, path::Path, ptr};

use windows::{
    Win32::{
        Foundation::{HLOCAL, LocalFree},
        Security::Authorization::{
            ConvertStringSecurityDescriptorToSecurityDescriptorW, GetNamedSecurityInfoW,
            SDDL_REVISION_1, SE_FILE_OBJECT, SetNamedSecurityInfoW,
        },
        Security::{
            ACL, DACL_SECURITY_INFORMATION, GetSecurityDescriptorControl,
            GetSecurityDescriptorDacl, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
            SE_DACL_PROTECTED, SECURITY_DESCRIPTOR_CONTROL, UNPROTECTED_DACL_SECURITY_INFORMATION,
        },
    },
    core::{BOOL, PCWSTR},
};

pub struct DirectoryAcl {
    path: Vec<u16>,
    descriptor: PSECURITY_DESCRIPTOR,
    dacl: *mut ACL,
    protected: bool,
    restricted: bool,
}

impl DirectoryAcl {
    pub fn deny_create_file(path: &Path) -> io::Result<Self> {
        let path = wide_path(path)?;
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        let mut dacl = ptr::null_mut();
        // SAFETY: the path is NUL terminated and output pointers are writable.
        let code = unsafe {
            GetNamedSecurityInfoW(
                PCWSTR(path.as_ptr()),
                SE_FILE_OBJECT,
                DACL_SECURITY_INFORMATION,
                None,
                None,
                Some(&mut dacl),
                None,
                &mut descriptor,
            )
        };
        if code.0 != 0 {
            return Err(io::Error::from_raw_os_error(code.0 as i32));
        }
        let mut guard = Self {
            path,
            descriptor,
            dacl,
            protected: false,
            restricted: false,
        };
        let mut control = SECURITY_DESCRIPTOR_CONTROL::default();
        let mut revision = 0;
        // SAFETY: descriptor is the valid buffer returned by GetNamedSecurityInfoW.
        unsafe { GetSecurityDescriptorControl(guard.descriptor, &mut control.0, &mut revision) }
            .map_err(io::Error::other)?;
        guard.protected = control.contains(SE_DACL_PROTECTED);
        guard.restrict()?;
        Ok(guard)
    }

    fn restrict(&mut self) -> io::Result<()> {
        let sddl: Vec<u16> = "D:P(D;;0x2;;;OW)(A;;0x001200A9;;;OW)(A;;FA;;;SY)(A;;FA;;;BA)\0"
            .encode_utf16()
            .collect();
        let mut descriptor = AllocatedDescriptor(PSECURITY_DESCRIPTOR::default());
        // SAFETY: SDDL string is NUL terminated and output pointer is writable.
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
        let mut dacl = ptr::null_mut();
        // SAFETY: descriptor is the valid security descriptor returned above.
        unsafe { GetSecurityDescriptorDacl(descriptor.0, &mut present, &mut dacl, &mut defaulted) }
            .map_err(io::Error::other)?;
        if !present.as_bool() || dacl.is_null() {
            return Err(io::Error::other("Windows returned an empty test DACL"));
        }
        let security_info = DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION;
        // SAFETY: path and DACL remain live for the call; test DACL denies owner file creation.
        let code = unsafe {
            SetNamedSecurityInfoW(
                PCWSTR(self.path.as_ptr()),
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
        self.restricted = true;
        Ok(())
    }

    pub fn restore(mut self) -> io::Result<()> {
        self.restore_inner()?;
        self.restricted = false;
        Ok(())
    }

    fn restore_inner(&self) -> io::Result<()> {
        let protection = if self.protected {
            PROTECTED_DACL_SECURITY_INFORMATION
        } else {
            UNPROTECTED_DACL_SECURITY_INFORMATION
        };
        let security_info = DACL_SECURITY_INFORMATION | protection;
        // SAFETY: path and original descriptor remain live; DACL came from that descriptor.
        let code = unsafe {
            SetNamedSecurityInfoW(
                PCWSTR(self.path.as_ptr()),
                SE_FILE_OBJECT,
                security_info,
                None,
                None,
                Some(self.dacl.cast_const()),
                None,
            )
        };
        if code.0 != 0 {
            return Err(io::Error::from_raw_os_error(code.0 as i32));
        }
        Ok(())
    }
}

struct AllocatedDescriptor(PSECURITY_DESCRIPTOR);

impl Drop for AllocatedDescriptor {
    fn drop(&mut self) {
        if !(self.0).0.is_null() {
            // SAFETY: conversion API allocated this descriptor and Drop releases it once.
            unsafe {
                let _ = LocalFree(Some(HLOCAL((self.0).0)));
            }
        }
    }
}

impl Drop for DirectoryAcl {
    fn drop(&mut self) {
        if self.restricted {
            let _ = self.restore_inner();
        }
        if !self.descriptor.0.is_null() {
            // SAFETY: GetNamedSecurityInfoW allocated this descriptor; Drop releases it once.
            unsafe {
                let _ = LocalFree(Some(HLOCAL(self.descriptor.0)));
            }
        }
    }
}

fn wide_path(path: &Path) -> io::Result<Vec<u16>> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    if wide.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "test path contains a NUL character",
        ));
    }
    Ok(wide.into_iter().chain(Some(0)).collect())
}
