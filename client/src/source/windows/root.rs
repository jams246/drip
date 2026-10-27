mod absence;
mod identity;

pub use absence::confirm_absent;

use std::{
    fs::{File, OpenOptions},
    os::windows::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

use drip_core::{SourcePath, protocol::EntryKind};
use windows::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_SHARE_READ, FILE_SHARE_WRITE,
};

use super::{
    error::SourceError,
    native::{self, file_info, is_disk_file, same_path},
};
use crate::settings::SavedSelection;
pub(super) use identity::file_volume_identity;
use identity::{check_case_sensitive, volume_identity};

/// Canonical source root captured when a selection is added.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceRoot {
    pub source: SourcePath,
    pub kind: EntryKind,
    pub volume_identity: String,
}

/// Captures canonical path, root kind, and native volume identity.
pub fn capture_root(path: &Path) -> Result<SourceRoot, SourceError> {
    let requested = SourcePath::try_from_path(path).map_err(|_| SourceError::InvalidPath)?;
    native::validate_drive(requested.drive())?;
    native::validate_directories(&requested)?;
    let file = open_root(path).map_err(SourceError::RootUnavailable)?;
    let info = file_info(&file)?;
    if !is_disk_file(&file) || info.attributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err(SourceError::NotRegular);
    }
    native::validate_final_path(&file, &requested)?;
    let source = native::final_path(&file)?;
    native::reject_application_data(&source)?;
    let kind = if info.standard.Directory {
        check_case_sensitive(&file, path)?;
        EntryKind::Directory
    } else {
        EntryKind::File
    };
    let volume_identity = volume_identity(&source, &info)?;
    Ok(SourceRoot {
        source,
        kind,
        volume_identity,
    })
}

pub(super) fn capture_app_directory(path: &Path) -> Result<SourceRoot, SourceError> {
    let raw = path.to_str().ok_or(SourceError::InvalidPath)?;
    let raw = raw.strip_prefix("\\\\?\\").unwrap_or(raw);
    let canonical = SourcePath::parse_absolute(raw).map_err(|_| SourceError::InvalidPath)?;
    let root = capture_root(&PathBuf::from(canonical.to_windows_path()))?;
    if root.kind != EntryKind::Directory {
        return Err(SourceError::InvalidPath);
    }
    Ok(root)
}

/// Confirms saved root still resolves to the same path, kind, and volume.
pub fn validate_root(selection: &SavedSelection) -> Result<(), SourceError> {
    let path = PathBuf::from(selection.source.to_windows_path());
    let current = capture_root(&path)?;
    if !same_path(&current.source, &selection.source) {
        return Err(SourceError::Replaced);
    }
    if current.volume_identity != selection.volume_identity {
        return Err(SourceError::VolumeChanged);
    }
    if current.source != selection.source {
        return Err(SourceError::Changed);
    }
    if current.kind != selection.root_kind {
        return Err(SourceError::RootKindChanged);
    }
    Ok(())
}

pub(super) fn open_directory(path: &Path) -> Result<File, SourceError> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0);
    let directory = options.open(path)?;
    let info = file_info(&directory)?;
    if !info.standard.Directory {
        return Err(SourceError::InvalidPath);
    }
    if info.attributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err(SourceError::ReparsePoint(path.to_owned()));
    }
    let expected = SourcePath::try_from_path(path).map_err(|_| SourceError::InvalidPath)?;
    native::validate_final_path(&directory, &expected)?;
    if native::final_path(&directory)? != expected {
        return Err(SourceError::Changed);
    }
    check_case_sensitive(&directory, path)?;
    Ok(directory)
}

pub(super) fn open_directory_chain(
    source: &SourcePath,
    include_source: bool,
    expected_volume: &str,
) -> Result<Vec<File>, SourceError> {
    let component_count = if include_source {
        source.components().len()
    } else {
        source.components().len().saturating_sub(1)
    };
    let mut path = PathBuf::from(format!("{}:\\", source.drive()));
    let mut directories = Vec::with_capacity(component_count + 1);
    let drive = open_directory(&path)?;
    verify_volume(&drive, expected_volume)?;
    directories.push(drive);
    for component in source.components().iter().take(component_count) {
        path.push(component);
        let directory = open_directory(&path)?;
        verify_volume(&directory, expected_volume)?;
        directories.push(directory);
    }
    Ok(directories)
}

pub(super) fn verify_volume(file: &File, expected: &str) -> Result<(), SourceError> {
    let info = file_info(file)?;
    if volume_identity(&native::final_path(file)?, &info)? == expected {
        Ok(())
    } else {
        Err(SourceError::VolumeChanged)
    }
}

fn open_root(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS.0 | FILE_FLAG_OPEN_REPARSE_POINT.0);
    options.open(path)
}
