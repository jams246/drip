use std::io;

use cap_fs_ext::{DirExt, MetadataExt};
use cap_std::fs::Dir;
use drip_core::{SourcePath, protocol::EntryKind};

use crate::{
    StoreError, StoreResult, database, upload_files, upload_files::FileSnapshot, upload_paths,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DirectorySnapshot {
    pub(crate) device: u64,
    pub(crate) inode: u64,
    pub(crate) modified: drip_core::Timestamp,
    pub(crate) change_seconds: i64,
    pub(crate) change_nanos: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EntrySnapshot {
    File(FileSnapshot),
    Directory(DirectorySnapshot),
}

impl EntrySnapshot {
    pub(crate) const fn kind(&self) -> EntryKind {
        match self {
            Self::File(_) => EntryKind::File,
            Self::Directory(_) => EntryKind::Directory,
        }
    }
}

pub(crate) fn read_entry(root: &Dir, source: &SourcePath) -> StoreResult<Option<EntrySnapshot>> {
    let Some((parent, name)) = open_parent(root, source)? else {
        return Ok(None);
    };
    let metadata = match parent.symlink_metadata(&name) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(database::io_error(error)),
    };
    if metadata.is_dir() {
        let directory = parent
            .open_dir_nofollow(&name)
            .map_err(database::io_error)?;
        Ok(Some(EntrySnapshot::Directory(snapshot_directory(
            &directory,
        )?)))
    } else if metadata.is_file() {
        let file = upload_paths::open_file(&parent, &name)?.ok_or(StoreError::Conflict)?;
        Ok(Some(EntrySnapshot::File(upload_files::hash_file(file)?)))
    } else {
        Err(StoreError::Conflict)
    }
}

pub(crate) fn check_entry(
    root: &Dir,
    source: &SourcePath,
    expected: Option<&EntrySnapshot>,
) -> StoreResult<()> {
    match expected {
        None => check_absent(root, source),
        Some(EntrySnapshot::File(snapshot)) => {
            upload_files::check_snapshot(root, source, Some(snapshot))
        }
        Some(EntrySnapshot::Directory(snapshot)) => check_directory(root, source, *snapshot),
    }
}

pub(crate) fn open_directory(root: &Dir, source: &SourcePath) -> StoreResult<Option<Dir>> {
    let Some(mut directory) = open_drive(root, source.drive())? else {
        return Ok(None);
    };
    for component in source.components() {
        match directory.open_dir_nofollow(component) {
            Ok(child) => directory = child,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(StoreError::Conflict),
        }
    }
    Ok(Some(directory))
}

pub(crate) fn check_directory(
    root: &Dir,
    source: &SourcePath,
    expected: DirectorySnapshot,
) -> StoreResult<()> {
    let Some((parent, name)) = open_parent(root, source)? else {
        return Err(StoreError::Conflict);
    };
    let metadata = parent.symlink_metadata(&name).map_err(database::io_error)?;
    if !metadata.is_dir() {
        return Err(StoreError::Conflict);
    }
    let directory = parent
        .open_dir_nofollow(&name)
        .map_err(database::io_error)?;
    if snapshot_directory(&directory)? == expected {
        Ok(())
    } else {
        Err(StoreError::Conflict)
    }
}

pub(crate) fn snapshot_directory(directory: &Dir) -> StoreResult<DirectorySnapshot> {
    let metadata = directory
        .symlink_metadata(".")
        .map_err(database::io_error)?;
    if !metadata.is_dir() {
        return Err(StoreError::Conflict);
    }
    let (change_seconds, change_nanos) = upload_files::change_time(&metadata);
    Ok(DirectorySnapshot {
        device: metadata.dev(),
        inode: metadata.ino(),
        modified: crate::upload_times::metadata_timestamp(&metadata)?,
        change_seconds,
        change_nanos,
    })
}

pub(crate) fn check_directory_after_exchange(
    parent: &Dir,
    name: &str,
    expected: DirectorySnapshot,
) -> StoreResult<()> {
    let directory = parent.open_dir_nofollow(name).map_err(database::io_error)?;
    let current = snapshot_directory(&directory)?;
    if current.device != expected.device
        || current.inode != expected.inode
        || current.modified != expected.modified
    {
        return Err(StoreError::Conflict);
    }
    Ok(())
}

pub(crate) fn check_absent(root: &Dir, source: &SourcePath) -> StoreResult<()> {
    let Some((parent, name)) = open_parent(root, source)? else {
        return Ok(());
    };
    match parent.symlink_metadata(&name) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Ok(_) => Err(StoreError::Conflict),
        Err(error) => Err(database::io_error(error)),
    }
}

pub(crate) fn open_parent(root: &Dir, source: &SourcePath) -> StoreResult<Option<(Dir, String)>> {
    if source.components().is_empty() {
        return Ok(Some((
            root.try_clone().map_err(database::io_error)?,
            source.drive().to_string(),
        )));
    }
    upload_paths::open_parent(root, source, false)
}

pub(crate) fn create_parent(root: &Dir, source: &SourcePath) -> StoreResult<(Dir, String)> {
    if source.components().is_empty() {
        return Ok((
            root.try_clone().map_err(database::io_error)?,
            source.drive().to_string(),
        ));
    }
    upload_paths::create_parent(root, source)
}

fn open_drive(root: &Dir, drive: char) -> StoreResult<Option<Dir>> {
    let name = drive.to_string();
    match root.open_dir_nofollow(&name) {
        Ok(directory) => Ok(Some(directory)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(StoreError::Conflict),
    }
}
