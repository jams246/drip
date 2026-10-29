use std::{
    fs::{self, File},
    os::windows::fs::MetadataExt,
    path::Path,
};

use drip_core::{SourcePath, protocol::EntryKind};
use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

use super::super::{
    error::SourceError,
    file::SourceFile,
    native::{self, file_info, same_path},
    root::{self, SourceRoot},
};
use super::{ScannedEntry, SourceScan};
use crate::settings::SavedSelection;

pub(super) fn walk_directory(
    selection: &SavedSelection,
    app_dir: &SourceRoot,
    path: &Path,
    source_path: &SourcePath,
    scan: &mut SourceScan,
) -> Result<(), SourceError> {
    let mut directories =
        root::open_directory_chain(source_path, true, &selection.volume_identity)?;
    let directory = directories.pop().ok_or(SourceError::InvalidPath)?;
    walk_open_directory(selection, app_dir, path, source_path, directory, scan)
}

fn walk_open_directory(
    selection: &SavedSelection,
    app_dir: &SourceRoot,
    path: &Path,
    source_path: &SourcePath,
    directory: File,
    scan: &mut SourceScan,
) -> Result<(), SourceError> {
    let before = directory_observation(&directory)?;
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) => {
            scan.issue(Some(source_path.clone()), error.to_string());
            return Ok(());
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                scan.issue(Some(source_path.clone()), error.to_string());
                continue;
            }
        };
        let name = match entry.file_name().into_string() {
            Ok(name) => name,
            Err(_) => {
                scan.issue(
                    Some(source_path.clone()),
                    "entry name cannot be represented losslessly",
                );
                continue;
            }
        };
        let child_source = match append(source_path, &name) {
            Ok(path) => path,
            Err(error) => {
                scan.issue(Some(source_path.clone()), error.to_string());
                continue;
            }
        };
        let child_path = entry.path();
        let metadata = match fs::symlink_metadata(&child_path) {
            Ok(metadata) => metadata,
            Err(error) => {
                scan.issue(Some(child_source), error.to_string());
                continue;
            }
        };
        let reparse = metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0;
        if same_path(source_path, &app_dir.source) && native::is_application_data_name(&name) {
            scan.protected_paths.push(child_source);
            continue;
        }
        if reparse {
            scan.issue(
                Some(child_source),
                SourceError::ReparsePoint(child_path).to_string(),
            );
            continue;
        }
        if metadata.is_dir() {
            match root::open_directory(&child_path) {
                Ok(child) => {
                    if let Err(error) = root::verify_volume(&child, &selection.volume_identity) {
                        scan.issue(Some(child_source), error.to_string());
                        continue;
                    }
                    scan.entries.push(ScannedEntry {
                        source_path: child_source.clone(),
                        local_path: child_path.clone(),
                        kind: EntryKind::Directory,
                        size: 0,
                        last_write: None,
                    });
                    if let Err(error) = walk_open_directory(
                        selection,
                        app_dir,
                        &child_path,
                        &child_source,
                        child,
                        scan,
                    ) {
                        scan.issue(Some(child_source), error.to_string());
                    }
                }
                Err(error) => scan.issue(Some(child_source), error.to_string()),
            }
        } else if metadata.is_file() {
            scan_file(selection, &child_path, &child_source, scan);
        } else {
            scan.issue(
                Some(child_source),
                "entry is not a regular file or directory",
            );
        }
    }
    if directory_observation(&directory)? != before {
        scan.issue(Some(source_path.clone()), "directory changed during scan");
    }
    Ok(())
}

fn scan_file(
    selection: &SavedSelection,
    path: &Path,
    source_path: &SourcePath,
    scan: &mut SourceScan,
) {
    match SourceFile::open(path) {
        Ok(file) if file.volume_identity() == selection.volume_identity => {
            if file.protocol_path() != source_path {
                scan.issue(
                    Some(source_path.clone()),
                    "opened file resolved to a different source path",
                );
            } else {
                scan.entries.push(ScannedEntry {
                    source_path: source_path.clone(),
                    local_path: path.to_owned(),
                    kind: EntryKind::File,
                    size: file.size(),
                    last_write: Some(file.last_write()),
                });
            }
        }
        Ok(_) => scan.issue(
            Some(source_path.clone()),
            SourceError::VolumeChanged.to_string(),
        ),
        Err(error) => scan.issue(Some(source_path.clone()), error.to_string()),
    }
}

fn directory_observation(directory: &File) -> Result<([u8; 16], i64, i64), SourceError> {
    let info = file_info(directory)?;
    Ok((
        info.id.FileId.Identifier,
        info.basic.LastWriteTime,
        info.basic.ChangeTime,
    ))
}

fn append(parent: &SourcePath, name: &str) -> Result<SourcePath, SourceError> {
    let mut components = parent.components().to_vec();
    components.push(name.to_owned());
    SourcePath::new(parent.drive(), components).map_err(|_| SourceError::InvalidPath)
}
