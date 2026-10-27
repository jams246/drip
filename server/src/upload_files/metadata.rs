use cap_fs_ext::MetadataExt;
use cap_std::fs::{Dir, File};
use drip_core::SourcePath;

use crate::{
    StoreError, StoreResult, database, upload_paths::open_parent, upload_times::metadata_timestamp,
};

use super::{FileSnapshot, open_read};

pub(super) fn metadata_matches(file: &File, expected: &FileSnapshot) -> StoreResult<bool> {
    let metadata = file.metadata().map_err(database::io_error)?;
    Ok(metadata.is_file() && same_metadata(&snapshot_metadata(&metadata)?, expected))
}

pub(super) fn snapshot_metadata(metadata: &cap_std::fs::Metadata) -> StoreResult<FileSnapshot> {
    let (change_seconds, change_nanos) = change_time(metadata);
    Ok(FileSnapshot {
        size: metadata.len(),
        digest: drip_core::Blake3Digest::new([0; 32]),
        last_write: metadata_timestamp(metadata)?,
        change_seconds,
        change_nanos,
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

pub(crate) fn check_file_identity(
    directory: &Dir,
    name: &str,
    expected: &FileSnapshot,
) -> StoreResult<()> {
    let file = open_read(directory, name)?;
    let metadata = file.metadata().map_err(database::io_error)?;
    let actual = snapshot_metadata(&metadata)?;
    if same_identity(&actual, expected) {
        Ok(())
    } else {
        Err(StoreError::Conflict)
    }
}

pub(crate) fn snapshot_published_file(
    root: &Dir,
    source: &SourcePath,
    expected: &FileSnapshot,
) -> StoreResult<FileSnapshot> {
    let (parent, name) = open_parent(root, source, false)?.ok_or(StoreError::Conflict)?;
    let file = crate::upload_paths::open_file(&parent, &name)?.ok_or(StoreError::Conflict)?;
    let metadata = file.metadata().map_err(database::io_error)?;
    let mut snapshot = snapshot_metadata(&metadata)?;
    if !same_identity(&snapshot, expected) {
        return Err(StoreError::Conflict);
    }
    snapshot.digest = expected.digest;
    Ok(snapshot)
}

pub(crate) fn change_time(metadata: &cap_std::fs::Metadata) -> (i64, i64) {
    #[cfg(unix)]
    {
        (
            cap_std::fs::MetadataExt::ctime(metadata),
            cap_std::fs::MetadataExt::ctime_nsec(metadata),
        )
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        (0, 0)
    }
}

pub(super) fn same_metadata(left: &FileSnapshot, right: &FileSnapshot) -> bool {
    same_identity(left, right)
        && left.change_seconds == right.change_seconds
        && left.change_nanos == right.change_nanos
}

fn same_identity(left: &FileSnapshot, right: &FileSnapshot) -> bool {
    left.size == right.size
        && left.last_write == right.last_write
        && left.device == right.device
        && left.inode == right.inode
}
