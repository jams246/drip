#![expect(
    unsafe_code,
    reason = "Windows file identity queries use native handles"
)]

use std::{
    fs::{File, OpenOptions},
    io,
    os::windows::{fs::OpenOptionsExt, io::AsRawHandle},
    path::{Path, PathBuf},
};

use drip_core::SourcePath;
use windows::{
    Win32::{
        Foundation::HANDLE,
        Security::Cryptography::{BCRYPT_USE_SYSTEM_PREFERRED_RNG, BCryptGenRandom},
        Storage::FileSystem::{
            FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO, FILE_BASIC_INFO,
            FILE_CASE_SENSITIVE_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_FLAG_SEQUENTIAL_SCAN, FILE_ID_INFO, FILE_INFO_BY_HANDLE_CLASS,
            FILE_NAME_NORMALIZED, FILE_STANDARD_INFO, FILE_TYPE_DISK, FileAttributeTagInfo,
            FileBasicInfo, FileCaseSensitiveInfo, FileIdInfo, FileStandardInfo, GetDriveTypeW,
            GetFileInformationByHandleEx, GetFileType, GetFinalPathNameByHandleW,
        },
        System::SystemServices::FILE_CS_FLAG_CASE_SENSITIVE_DIR,
    },
    core::PCWSTR,
};

use super::error::SourceError;

const DRIVE_REMOTE: u32 = 4;

pub(super) struct HandleInfo {
    pub attributes: u32,
    pub standard: FILE_STANDARD_INFO,
    pub id: FILE_ID_INFO,
    pub basic: FILE_BASIC_INFO,
}

pub(super) fn validate_drive(drive: char) -> Result<(), SourceError> {
    let root = format!("{drive}:\\");
    let root: Vec<u16> = root.encode_utf16().chain(Some(0)).collect();
    if unsafe { GetDriveTypeW(PCWSTR(root.as_ptr())) } == DRIVE_REMOTE {
        return Err(SourceError::NetworkDrive);
    }
    Ok(())
}

pub(super) fn reject_application_data(path: &SourcePath) -> Result<(), SourceError> {
    let Some(name) = path.components().last() else {
        return Ok(());
    };
    let parent = SourcePath::new(
        path.drive(),
        path.components()[..path.components().len() - 1].to_vec(),
    )
    .map_err(|_| SourceError::InvalidPath)?;
    let executable = std::env::current_exe()?;
    let directory = executable.parent().ok_or(SourceError::InvalidPath)?;
    let directory = std::fs::canonicalize(directory)?;
    let directory = directory.to_str().ok_or(SourceError::InvalidPath)?;
    let directory = directory.strip_prefix(r"\\?\").unwrap_or(directory);
    let directory = SourcePath::parse_absolute(directory).map_err(|_| SourceError::InvalidPath)?;
    if same_path(&parent, &directory) && is_application_data_name(name) {
        return Err(SourceError::ExcludedApplicationData);
    }
    Ok(())
}

pub(super) fn is_application_data_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    matches!(
        name.as_str(),
        "drip.json"
            | "drip.credentials"
            | "drip.log"
            | "drip.log.1"
            | "drip.state"
            | "drip.settings.lock"
            | "drip.run.lock"
    ) || [
        "drip.json.",
        "drip.credentials.",
        "drip.log.",
        "drip.state.",
        "drip.settings.",
        "drip.run.",
        ".drip.",
    ]
    .iter()
    .any(|prefix| name.starts_with(prefix))
}

pub(super) fn validate_directories(path: &SourcePath) -> Result<(), SourceError> {
    let mut directory = PathBuf::from(format!("{}:\\", path.drive()));
    check_directory(&directory)?;
    for component in path
        .components()
        .iter()
        .take(path.components().len().saturating_sub(1))
    {
        directory.push(component);
        check_directory(&directory)?;
    }
    Ok(())
}

pub(super) fn open_file(path: &Path) -> Result<File, SourceError> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0 | FILE_FLAG_SEQUENTIAL_SCAN.0);
    Ok(options.open(path)?)
}

pub(super) fn file_info(file: &impl AsRawHandle) -> Result<HandleInfo, SourceError> {
    let handle = HANDLE(file.as_raw_handle());
    let attributes: FILE_ATTRIBUTE_TAG_INFO = query_handle(handle, FileAttributeTagInfo)?;
    let standard: FILE_STANDARD_INFO = query_handle(handle, FileStandardInfo)?;
    let id: FILE_ID_INFO = query_handle(handle, FileIdInfo)?;
    let basic: FILE_BASIC_INFO = query_handle(handle, FileBasicInfo)?;
    Ok(HandleInfo {
        attributes: attributes.FileAttributes,
        standard,
        id,
        basic,
    })
}

pub(super) fn is_disk_file(file: &impl AsRawHandle) -> bool {
    unsafe { GetFileType(HANDLE(file.as_raw_handle())) == FILE_TYPE_DISK }
}

fn check_directory(path: &Path) -> Result<(), SourceError> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0);
    let directory = options.open(path)?;
    let info = file_info(&directory)?;
    if !info.standard.Directory {
        return Err(SourceError::InvalidPath);
    }
    if info.attributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err(SourceError::ReparsePoint(path.to_owned()));
    }
    let case_info: FILE_CASE_SENSITIVE_INFO = query(&directory, FileCaseSensitiveInfo)?;
    if case_info.Flags & FILE_CS_FLAG_CASE_SENSITIVE_DIR != 0 {
        return Err(SourceError::CaseSensitiveDirectory(path.to_owned()));
    }
    Ok(())
}

fn query<T: Default>(
    file: &impl AsRawHandle,
    class: FILE_INFO_BY_HANDLE_CLASS,
) -> Result<T, SourceError> {
    query_handle(HANDLE(file.as_raw_handle()), class)
}

fn query_handle<T: Default>(
    handle: HANDLE,
    class: FILE_INFO_BY_HANDLE_CLASS,
) -> Result<T, SourceError> {
    let mut info = T::default();
    unsafe {
        GetFileInformationByHandleEx(
            handle,
            class,
            (&mut info as *mut T).cast(),
            u32::try_from(std::mem::size_of::<T>()).map_err(|_| SourceError::PathResolution)?,
        )?;
    }
    Ok(info)
}

pub(super) fn validate_final_path(
    file: &impl AsRawHandle,
    expected: &SourcePath,
) -> Result<(), SourceError> {
    let final_path = final_path(file)?;
    if !same_path(&final_path, expected) {
        return Err(SourceError::PathResolution);
    }
    Ok(())
}

pub(super) fn final_path(file: &impl AsRawHandle) -> Result<SourcePath, SourceError> {
    let mut final_path = vec![0u16; 32768];
    let length = unsafe {
        GetFinalPathNameByHandleW(
            HANDLE(file.as_raw_handle()),
            &mut final_path,
            FILE_NAME_NORMALIZED,
        )
    };
    if length == 0 || length as usize >= final_path.len() {
        return Err(SourceError::PathResolution);
    }
    final_path.truncate(length as usize);
    let final_path = String::from_utf16(&final_path).map_err(|_| SourceError::PathResolution)?;
    let final_path = final_path.strip_prefix("\\\\?\\").unwrap_or(&final_path);
    SourcePath::parse_absolute(final_path).map_err(|_| SourceError::PathResolution)
}

fn same_strings(left: &str, right: &str) -> bool {
    let left = left.replace('/', "\\").encode_utf16().collect::<Vec<_>>();
    let right = right.replace('/', "\\").encode_utf16().collect::<Vec<_>>();
    if left.len() > i32::MAX as usize || right.len() > i32::MAX as usize {
        return false;
    }
    unsafe {
        windows::Win32::Globalization::CompareStringOrdinal(&left, &right, true)
            == windows::Win32::Globalization::CSTR_EQUAL
    }
}

pub(crate) fn same_path(left: &SourcePath, right: &SourcePath) -> bool {
    same_strings(&left.to_windows_path(), &right.to_windows_path())
}

pub(crate) fn random_id() -> Result<String, SourceError> {
    let mut bytes = [0; 16];
    let status = unsafe { BCryptGenRandom(None, &mut bytes, BCRYPT_USE_SYSTEM_PREFERRED_RNG) };
    if status.0 < 0 {
        return Err(SourceError::Io(io::Error::other(format!(
            "Windows random generator failed with status {:#x}",
            status.0
        ))));
    }
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}
