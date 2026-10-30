#![expect(unsafe_code, reason = "Windows file validation uses native handles")]

use std::{
    fs::{File, OpenOptions},
    io::{self, Read},
    os::windows::{
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    },
    path::Path,
};

use windows::Win32::{
    Foundation::{GENERIC_READ, GENERIC_WRITE, HANDLE},
    Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE,
        GetFileInformationByHandle, WRITE_DAC,
    },
};

/// Keeps every ancestor stable while opening or replacing private state.
pub(crate) fn parents(path: &Path) -> io::Result<Vec<File>> {
    if !path.is_absolute() {
        return Err(invalid());
    }
    let mut paths: Vec<_> = path.ancestors().skip(1).collect();
    paths.reverse();
    paths
        .into_iter()
        .map(|parent| {
            let file = OpenOptions::new()
                .read(true)
                .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0 | FILE_FLAG_BACKUP_SEMANTICS.0)
                .open(parent)?;
            let metadata = file.metadata()?;
            if !metadata.is_dir()
                || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
            {
                return Err(invalid());
            }
            Ok(file)
        })
        .collect()
}

/// Opens one regular, unaliased file and protects that exact handle before use.
pub(crate) fn open(path: &Path, write: bool, create_new: bool) -> io::Result<File> {
    let _parents = parents(path)?;
    let mut access = GENERIC_READ.0 | WRITE_DAC.0;
    if write {
        access |= GENERIC_WRITE.0;
    }
    let file = OpenOptions::new()
        .read(true)
        .write(write)
        .access_mode(access)
        .create_new(create_new)
        .share_mode(if create_new {
            0
        } else {
            FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0
        })
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(path)?;
    validate(&file)?;
    crate::credentials::acl::secure_handle(&file)?;
    Ok(file)
}

pub(crate) fn read(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let file = open(path, false, false)?;
    read_open(file, limit)
}

pub(crate) fn read_open(file: File, limit: u64) -> io::Result<Vec<u8>> {
    if file.metadata()?.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "private state exceeds its size limit",
        ));
    }
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "private state exceeds its size limit",
        ));
    }
    Ok(bytes)
}

fn validate(file: &File) -> io::Result<()> {
    let metadata = file.metadata()?;
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the file handle remains live and `info` is writable.
    unsafe { GetFileInformationByHandle(HANDLE(file.as_raw_handle()), &mut info) }
        .map_err(io::Error::other)?;
    if !metadata.is_file()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
        || info.nNumberOfLinks != 1
    {
        return Err(invalid());
    }
    Ok(())
}

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "private state path is not a regular unaliased file",
    )
}

#[cfg(test)]
pub(crate) mod tests;
