mod identity;

use std::{
    ffi::OsStr,
    fs::{File, OpenOptions},
    os::windows::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
    },
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
    Ok(())
}

/// Rechecks root identity and confirms a descendant is absent without following links.
pub fn confirm_absent(
    selection: &SavedSelection,
    target: &SourcePath,
    exe_dir: &Path,
) -> Result<bool, SourceError> {
    let root = capture_saved_root(selection)?;
    if root.kind != selection.root_kind {
        return Err(SourceError::RootKindChanged);
    }
    if !strict_ancestor(&selection.source, target) {
        return Ok(false);
    }
    let app_dir = capture_app_directory(exe_dir)?;
    if is_protected_application_path(target, &app_dir) {
        return Ok(false);
    }
    let mut path = PathBuf::from(selection.source.to_windows_path());
    let mut open_directories =
        open_directory_chain(&selection.source, true, &selection.volume_identity)?;
    let suffix = &target.components()[selection.source.components().len()..];
    for (index, component) in suffix.iter().enumerate() {
        let exact_name = OsStr::new(component);
        let exact = std::fs::read_dir(&path)?.try_fold(false, |found, entry| {
            let entry = entry?;
            Ok::<_, std::io::Error>(
                found || entry.file_name().encode_wide().eq(exact_name.encode_wide()),
            )
        })?;
        if !exact {
            validate_same_root_kind(selection)?;
            return Ok(true);
        }
        path.push(component);
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                validate_same_root_kind(selection)?;
                return Ok(true);
            }
            Err(source) => return Err(SourceError::Io(source)),
        };
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
            return Err(SourceError::ReparsePoint(path));
        }
        if index + 1 < suffix.len() {
            if !metadata.is_dir() {
                return Ok(false);
            }
            let directory = open_directory(&path)?;
            verify_volume(&directory, &selection.volume_identity)?;
            open_directories.push(directory);
        }
    }
    validate_same_root_kind(selection)?;
    Ok(false)
}

fn capture_saved_root(selection: &SavedSelection) -> Result<SourceRoot, SourceError> {
    let current = capture_root(&PathBuf::from(selection.source.to_windows_path()))?;
    if !same_path(&current.source, &selection.source) {
        return Err(SourceError::Replaced);
    }
    if current.volume_identity != selection.volume_identity {
        return Err(SourceError::VolumeChanged);
    }
    Ok(current)
}

fn validate_same_root_kind(selection: &SavedSelection) -> Result<(), SourceError> {
    if capture_saved_root(selection)?.kind == selection.root_kind {
        Ok(())
    } else {
        Err(SourceError::RootKindChanged)
    }
}

fn is_protected_application_path(target: &SourcePath, exe_dir: &SourceRoot) -> bool {
    let Some(name) = target.components().last() else {
        return false;
    };
    let Some(parent) = SourcePath::new(
        target.drive(),
        target.components()[..target.components().len() - 1].to_vec(),
    )
    .ok() else {
        return false;
    };
    same_path(&parent, &exe_dir.source) && native::is_application_data_name(name)
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

fn strict_ancestor(parent: &SourcePath, child: &SourcePath) -> bool {
    if parent.drive() != child.drive() || parent.components().len() >= child.components().len() {
        return false;
    }
    SourcePath::new(
        child.drive(),
        child.components()[..parent.components().len()].to_vec(),
    )
    .is_ok_and(|prefix| same_path(parent, &prefix))
}
