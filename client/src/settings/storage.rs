#![expect(unsafe_code, reason = "Windows atomic replacement uses native APIs")]

use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    os::windows::ffi::OsStrExt,
    os::windows::fs::MetadataExt,
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::Duration,
};

use windows::{
    Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_LOCK_VIOLATION, ERROR_SHARING_VIOLATION},
    Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        MoveFileExW,
    },
    core::PCWSTR,
};

use super::store::{SettingsError, path_error};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);
const REPLACE_RETRY_COUNT: u8 = 20;
const REPLACE_RETRY_DELAY: Duration = Duration::from_millis(50);

pub(super) fn replace_with(destination: &Path, bytes: &[u8]) -> Result<(), SettingsError> {
    let temporary = write_temporary(destination, bytes)?;
    match fs::symlink_metadata(destination) {
        Ok(metadata)
            if metadata.file_type().is_file()
                && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 == 0 =>
        {
            crate::credentials::acl::secure_file(destination)
                .map_err(|source| path_error("protect permissions on", destination, source))?;
        }
        Ok(_) => return Err(SettingsError::InvalidTarget(destination.to_owned())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(source) => return Err(path_error("inspect", destination, source)),
    }
    move_replace(&temporary.path, destination)
}

fn write_temporary(destination: &Path, bytes: &[u8]) -> Result<TemporaryFile, SettingsError> {
    let parent = destination
        .parent()
        .ok_or_else(|| SettingsError::InvalidTarget(destination.to_owned()))?;
    for _ in 0..32 {
        let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(".drip.json.{}.{}.tmp", process::id(), counter));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(mut file) => {
                let temporary = TemporaryFile { path };
                crate::credentials::acl::secure_file(&temporary.path).map_err(|source| {
                    path_error("protect permissions on", &temporary.path, source)
                })?;
                file.write_all(bytes)
                    .map_err(|source| path_error("write", &temporary.path, source))?;
                file.flush()
                    .map_err(|source| path_error("flush", &temporary.path, source))?;
                file.sync_all()
                    .map_err(|source| path_error("sync", &temporary.path, source))?;
                drop(file);
                return Ok(temporary);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(source) => return Err(path_error("create", &path, source)),
        }
    }
    Err(SettingsError::TemporaryNameExhausted(
        destination.to_owned(),
    ))
}

fn move_replace(source: &Path, destination: &Path) -> Result<(), SettingsError> {
    let source_wide = wide_path(source).map_err(|error| path_error("encode", source, error))?;
    let destination_wide =
        wide_path(destination).map_err(|error| path_error("encode", destination, error))?;
    let mut retries = 0;
    loop {
        // SAFETY: both paths are NUL terminated, name same-directory files, and remain live for the call.
        let result = unsafe {
            MoveFileExW(
                PCWSTR(source_wide.as_ptr()),
                PCWSTR(destination_wide.as_ptr()),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        match result {
            Ok(()) => return Ok(()),
            Err(error) if retries < REPLACE_RETRY_COUNT && is_transient_replace_error(&error) => {
                retries += 1;
                thread::sleep(REPLACE_RETRY_DELAY);
            }
            Err(source) => {
                return Err(path_error(
                    "atomically replace",
                    destination,
                    io::Error::other(source),
                ));
            }
        }
    }
}

// A destination handle without FILE_SHARE_DELETE can report access denied.
fn is_transient_replace_error(error: &windows::core::Error) -> bool {
    let code = error.code().0 as u32;
    if code & 0xffff_0000 != 0x8007_0000 {
        return false;
    }
    let code = code & 0xffff;
    code == ERROR_ACCESS_DENIED.0
        || code == ERROR_SHARING_VIOLATION.0
        || code == ERROR_LOCK_VIOLATION.0
}

fn wide_path(path: &Path) -> io::Result<Vec<u16>> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    if wide.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "settings path contains a NUL character",
        ));
    }
    Ok(wide.into_iter().chain(Some(0)).collect())
}

struct TemporaryFile {
    path: PathBuf,
}

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}
