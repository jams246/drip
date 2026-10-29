use super::super::{
    error::SourceError,
    native::{self, same_path},
};
use super::{
    SourceRoot, capture_app_directory, capture_root, open_directory, open_directory_chain,
    verify_volume,
};
use crate::settings::SavedSelection;
use drip_core::SourcePath;
use std::{
    ffi::OsStr,
    os::windows::{ffi::OsStrExt, fs::MetadataExt},
    path::{Path, PathBuf},
};
use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

/// Rechecks the canonical root and confirms an inventoried spelling is absent without following links.
pub fn confirm_absent(
    selection: &SavedSelection,
    target: &SourcePath,
    exe_dir: &Path,
) -> Result<bool, SourceError> {
    let root = capture_saved_root(selection)?;
    if root.source != selection.source {
        return Err(SourceError::Changed);
    }
    if root.kind != selection.root_kind {
        return Err(SourceError::RootKindChanged);
    }
    if !selected_path(&selection.source, target) {
        return Ok(false);
    }
    let app_dir = capture_app_directory(exe_dir)?;
    if is_protected_application_path(target, &app_dir) {
        return Ok(false);
    }
    let mut open_directories =
        open_directory_chain(&selection.source, false, &selection.volume_identity)?;
    let mut path = PathBuf::from(selection.source.to_windows_path());
    let _root_handle = super::open_root(&path)?;
    verify_volume(&_root_handle, &selection.volume_identity)?;
    validate_same_root_kind(selection)?;
    let root_components = selection.source.components().len();
    if target.components()[..root_components] != *selection.source.components() {
        validate_same_root_kind(selection)?;
        return Ok(true);
    }
    if target.components().len() == root_components {
        return Ok(false);
    }
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
    let current = capture_saved_root(selection)?;
    if current.source != selection.source {
        return Err(SourceError::Changed);
    }
    if current.kind == selection.root_kind {
        Ok(())
    } else {
        Err(SourceError::RootKindChanged)
    }
}

fn is_protected_application_path(target: &SourcePath, exe_dir: &SourceRoot) -> bool {
    target.has_windows_prefix(&exe_dir.source)
        && target
            .components()
            .get(exe_dir.source.components().len())
            .is_some_and(|name| native::is_application_data_name(name))
}

fn selected_path(parent: &SourcePath, child: &SourcePath) -> bool {
    if parent.drive() != child.drive() || parent.components().len() > child.components().len() {
        return false;
    }
    SourcePath::new(
        child.drive(),
        child.components()[..parent.components().len()].to_vec(),
    )
    .is_ok_and(|prefix| same_path(parent, &prefix))
}
