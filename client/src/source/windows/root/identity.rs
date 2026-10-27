#![expect(
    unsafe_code,
    reason = "Windows source identity requires native handles"
)]

use std::{fs::File, os::windows::io::AsRawHandle, path::Path};

use drip_core::SourcePath;
use windows::{
    Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{
            FILE_CASE_SENSITIVE_INFO, FileCaseSensitiveInfo, GetFileInformationByHandleEx,
            GetVolumeNameForVolumeMountPointW, GetVolumePathNameW,
        },
        System::SystemServices::FILE_CS_FLAG_CASE_SENSITIVE_DIR,
    },
    core::PCWSTR,
};

use super::super::{
    error::SourceError,
    native::{self, HandleInfo, file_info},
};

pub(super) fn check_case_sensitive(file: &File, path: &Path) -> Result<(), SourceError> {
    let mut info = FILE_CASE_SENSITIVE_INFO::default();
    unsafe {
        GetFileInformationByHandleEx(
            HANDLE(file.as_raw_handle()),
            FileCaseSensitiveInfo,
            (&mut info as *mut FILE_CASE_SENSITIVE_INFO).cast(),
            std::mem::size_of::<FILE_CASE_SENSITIVE_INFO>() as u32,
        )?;
    }
    if info.Flags & FILE_CS_FLAG_CASE_SENSITIVE_DIR != 0 {
        return Err(SourceError::CaseSensitiveDirectory(path.to_owned()));
    }
    Ok(())
}

pub(super) fn volume_identity(
    source: &SourcePath,
    info: &HandleInfo,
) -> Result<String, SourceError> {
    let path = wide(&source.to_windows_path());
    let mut mount = vec![0u16; 32768];
    unsafe { GetVolumePathNameW(PCWSTR(path.as_ptr()), &mut mount) }?;
    if !mount.contains(&0) {
        return Err(SourceError::PathResolution);
    }
    let mut name = vec![0u16; 32768];
    unsafe { GetVolumeNameForVolumeMountPointW(PCWSTR(mount.as_ptr()), &mut name) }?;
    let name_len = name
        .iter()
        .position(|unit| *unit == 0)
        .ok_or(SourceError::PathResolution)?;
    let guid = String::from_utf16(&name[..name_len]).map_err(|_| SourceError::PathResolution)?;
    Ok(format!(
        "{}:{:016x}",
        guid.to_ascii_lowercase(),
        info.id.VolumeSerialNumber
    ))
}

pub(crate) fn file_volume_identity(file: &File) -> Result<String, SourceError> {
    let info = file_info(file)?;
    volume_identity(&native::final_path(file)?, &info)
}

fn wide(path: &str) -> Vec<u16> {
    path.encode_utf16().chain(Some(0)).collect()
}
