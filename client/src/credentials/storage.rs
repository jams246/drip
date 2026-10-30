#![expect(
    unsafe_code,
    reason = "Windows atomic file replacement uses native APIs"
)]

use std::{
    fs,
    io::{self, Write},
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicU64, Ordering},
};

use windows::{
    Win32::Storage::FileSystem::{MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW},
    core::PCWSTR,
};

use super::{CredentialError, path_error};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(super) fn replace_with(
    path: &Path,
    ciphertext: &[u8],
    mut checkpoint: impl FnMut(Stage),
) -> Result<(), CredentialError> {
    let _parents = crate::private_file::parents(path)
        .map_err(|source| path_error("inspect directory", path, source))?;
    let temporary = write_temporary(path, ciphertext)?;
    match crate::private_file::open(path, false, false) {
        Ok(file) => drop(file),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(source) => return Err(path_error("inspect", path, source)),
    }
    checkpoint(Stage::BeforeReplace);
    move_replace(&temporary.path, path)?;
    checkpoint(Stage::AfterReplace);
    Ok(())
}

fn write_temporary(
    destination: &Path,
    ciphertext: &[u8],
) -> Result<TemporaryFile, CredentialError> {
    let parent = destination
        .parent()
        .ok_or_else(|| CredentialError::InvalidTarget(destination.to_owned()))?;
    for _ in 0..32 {
        let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = parent.join(format!(
            ".drip.credentials.{}.{}.tmp",
            process::id(),
            counter
        ));
        match crate::private_file::open(&path, true, true) {
            Ok(mut file) => {
                let temporary = TemporaryFile { path };
                file.write_all(ciphertext)
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
    Err(CredentialError::TemporaryNameExhausted(
        destination.to_owned(),
    ))
}

fn move_replace(source: &Path, destination: &Path) -> Result<(), CredentialError> {
    let source_wide = wide_path(source).map_err(|error| path_error("encode", source, error))?;
    let destination_wide =
        wide_path(destination).map_err(|error| path_error("encode", destination, error))?;
    // SAFETY: both paths are NUL terminated, name same-directory files, and remain live for the call.
    unsafe {
        MoveFileExW(
            PCWSTR(source_wide.as_ptr()),
            PCWSTR(destination_wide.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    }
    .map_err(|source| path_error("atomically replace", destination, io::Error::other(source)))
}

fn wide_path(path: &Path) -> io::Result<Vec<u16>> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    if wide.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "credentials path contains a NUL character",
        ));
    }
    Ok(wide.into_iter().chain(Some(0)).collect())
}

struct TemporaryFile {
    path: PathBuf,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum Stage {
    BeforeReplace,
    AfterReplace,
}

impl Drop for TemporaryFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}
