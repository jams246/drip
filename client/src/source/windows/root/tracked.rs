#![expect(
    unsafe_code,
    reason = "Opening a moved Windows root by file ID requires native handles"
)]

use std::{
    fs::File,
    os::windows::io::{AsRawHandle, FromRawHandle},
    path::PathBuf,
};

use serde::{Deserialize, Serialize};
use windows::{
    Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_INVALID_PARAMETER, ERROR_PATH_NOT_FOUND, HANDLE},
        Storage::FileSystem::{
            ExtendedFileIdType, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_ID_128, FILE_ID_DESCRIPTOR, FILE_ID_DESCRIPTOR_0, FILE_READ_ATTRIBUTES,
            FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FileIdType, OpenFileById,
        },
    },
    core::HRESULT,
};

use super::super::{SourceError, native};
use super::{SourceRoot, capture_root, open_directory, verify_volume};
use crate::settings::SavedSelection;

/// Stable native identity used to follow a selected root on its original volume.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RootIdentity {
    pub file_id: [u8; 16],
    pub created: i64,
}

pub(crate) fn resolve_root(saved: &SavedSelection) -> Result<Option<SourceRoot>, SourceError> {
    let current = capture_root(&PathBuf::from(saved.source.to_windows_path()));
    if current
        .as_ref()
        .is_ok_and(|root| saved.root_identity.is_none_or(|id| id == root.identity))
    {
        return current.map(Some);
    }
    if let Some(identity) = saved.root_identity
        && let Some(root) = locate(saved, identity)?
    {
        return Ok(Some(root));
    }
    match current {
        Ok(root) => Ok(Some(root)),
        Err(SourceError::RootUnavailable(error) | SourceError::Io(error))
            if error.kind() == std::io::ErrorKind::NotFound =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

fn locate(
    saved: &SavedSelection,
    identity: RootIdentity,
) -> Result<Option<SourceRoot>, SourceError> {
    let volume = open_directory(&PathBuf::from(format!("{}:\\", saved.source.drive())))?;
    verify_volume(&volume, &saved.volume_identity)?;
    let file = match open_identity(&volume, identity.file_id) {
        Ok(file) => file,
        Err(error)
            if [ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND]
                .iter()
                .any(|code| error.code() == HRESULT::from_win32(code.0)) =>
        {
            return Ok(None);
        }
        Err(error)
            if error.code() == HRESULT::from_win32(ERROR_INVALID_PARAMETER.0)
                && open_identity(&volume, native::file_info(&volume)?.id.FileId.Identifier)
                    .is_ok() =>
        {
            // NTFS also reports a retired file ID as invalid; probe a live ID before accepting absence.
            return Ok(None);
        }
        Err(error) => return Err(error.into()),
    };
    let source = native::final_path(&file)?;
    let root = capture_root(&PathBuf::from(source.to_windows_path()))?;
    if root.identity != identity || root.volume_identity != saved.volume_identity {
        return Err(SourceError::Replaced);
    }
    Ok(Some(root))
}

fn open_identity(volume: &File, file_id: [u8; 16]) -> Result<File, windows::core::Error> {
    let mut descriptor = FILE_ID_DESCRIPTOR {
        dwSize: std::mem::size_of::<FILE_ID_DESCRIPTOR>() as u32,
        ..Default::default()
    };
    if file_id[8..].iter().all(|byte| *byte == 0) {
        let mut short = [0; 8];
        short.copy_from_slice(&file_id[..8]);
        descriptor.Type = FileIdType;
        descriptor.Anonymous = FILE_ID_DESCRIPTOR_0 {
            FileId: i64::from_le_bytes(short),
        };
    } else {
        descriptor.Type = ExtendedFileIdType;
        descriptor.Anonymous = FILE_ID_DESCRIPTOR_0 {
            ExtendedFileId: FILE_ID_128 {
                Identifier: file_id,
            },
        };
    }
    let handle = unsafe {
        OpenFileById(
            HANDLE(volume.as_raw_handle()),
            &descriptor,
            FILE_READ_ATTRIBUTES.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
        )
    }?;
    Ok(unsafe { File::from_raw_handle(handle.0) })
}
