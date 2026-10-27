use std::io::{self, Read};

use cap_fs_ext::{DirExt, FollowSymlinks, MetadataExt, OpenOptionsFollowExt, SystemTimeSpec};
use cap_std::fs::{Dir, File, OpenOptions};
use drip_core::{Blake3Digest, SourcePath, Timestamp};

use crate::{StoreError, StoreResult, database};
use crate::{
    upload_paths::{open_file, open_parent},
    upload_times::{metadata_timestamp, timestamp_system_time},
};

#[cfg(test)]
pub(crate) type CheckpointHook = std::sync::Arc<dyn Fn(&'static str) + Send + Sync>;

#[cfg(test)]
static CHECKPOINT_HOOK: std::sync::OnceLock<std::sync::Mutex<Option<CheckpointHook>>> =
    std::sync::OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileSnapshot {
    pub(crate) size: u64,
    pub(crate) digest: Blake3Digest,
    pub(crate) last_write: Timestamp,
    pub(crate) device: u64,
    pub(crate) inode: u64,
}

pub(crate) fn read_snapshot(root: &Dir, source: &SourcePath) -> StoreResult<Option<FileSnapshot>> {
    let Some((parent, name)) = open_parent(root, source, false)? else {
        return Ok(None);
    };
    let Some(file) = open_file(&parent, &name)? else {
        return Ok(None);
    };
    hash_file(file).map(Some)
}

pub(crate) fn check_snapshot(
    root: &Dir,
    source: &SourcePath,
    expected: Option<&FileSnapshot>,
) -> StoreResult<()> {
    let Some((parent, name)) = open_parent(root, source, false)? else {
        return if expected.is_none() {
            Ok(())
        } else {
            Err(StoreError::Conflict)
        };
    };
    let actual = open_file(&parent, &name)?;
    match (actual, expected) {
        (None, None) => Ok(()),
        (Some(file), Some(expected)) if metadata_matches(&file, expected)? => Ok(()),
        _ => Err(StoreError::Conflict),
    }
}

pub(crate) fn verify_stage(
    staging: &Dir,
    name: &str,
    expected_size: u64,
    expected_digest: Blake3Digest,
    last_write: Timestamp,
) -> StoreResult<FileSnapshot> {
    let file = open_read(staging, name)?;
    let mut snapshot = hash_file(file)?;
    if snapshot.size != expected_size || snapshot.digest != expected_digest {
        return Err(StoreError::Conflict);
    }
    staging
        .set_mtime(
            name,
            SystemTimeSpec::Absolute(cap_std::time::SystemTime::from_std(timestamp_system_time(
                last_write,
            )?)),
        )
        .map_err(map_io)?;
    let file = open_read(staging, name)?;
    file.sync_all().map_err(map_io)?;
    let metadata = file.metadata().map_err(database::io_error)?;
    if metadata.len() != expected_size
        || metadata.dev() != snapshot.device
        || metadata.ino() != snapshot.inode
    {
        return Err(StoreError::Conflict);
    }
    snapshot.last_write = metadata_timestamp(&metadata)?;
    Ok(snapshot)
}

pub(crate) fn check_stage_snapshot(
    staging: &Dir,
    name: &str,
    expected: &FileSnapshot,
) -> StoreResult<()> {
    let file = open_read(staging, name)?;
    if metadata_matches(&file, expected)? {
        Ok(())
    } else {
        Err(StoreError::Conflict)
    }
}

pub(crate) fn fsync_directory(directory: &Dir) -> StoreResult<()> {
    let mut options = OpenOptions::new();
    options.read(true);
    options.follow(FollowSymlinks::No);
    directory
        .open_with(".", &options)
        .map_err(database::io_error)?
        .sync_all()
        .map_err(database::io_error)
}

pub(crate) fn publish(
    staging: &Dir,
    stage_name: &str,
    parent: &Dir,
    filename: &str,
) -> Result<(), PublishFailure> {
    checkpoint("before_rename");
    staging
        .rename(stage_name, parent, filename)
        .map_err(|error| PublishFailure {
            error: database::io_error(error),
            renamed: false,
        })?;
    checkpoint("after_rename");
    fsync_directory(parent).map_err(|error| PublishFailure {
        error,
        renamed: true,
    })?;
    checkpoint("after_parent_fsync");
    Ok(())
}

pub(crate) struct PublishFailure {
    pub(crate) error: StoreError,
    pub(crate) renamed: bool,
}

fn open_read(directory: &Dir, name: &str) -> StoreResult<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    options.follow(FollowSymlinks::No);
    directory
        .open_with(name, &options)
        .map_err(database::io_error)
}

fn hash_file(mut file: File) -> StoreResult<FileSnapshot> {
    let before = file.metadata().map_err(database::io_error)?;
    if !before.is_file() {
        return Err(StoreError::Conflict);
    }
    let initial = snapshot_metadata(&before)?;
    let mut hasher = blake3::Hasher::new();
    let mut bytes = [0u8; 64 * 1024];
    let mut size = 0u64;
    loop {
        let count = file.read(&mut bytes).map_err(database::io_error)?;
        if count == 0 {
            break;
        }
        size = size.checked_add(count as u64).ok_or(StoreError::Conflict)?;
        hasher.update(&bytes[..count]);
    }
    let after = file.metadata().map_err(database::io_error)?;
    if size != initial.size || !same_metadata(&initial, &snapshot_metadata(&after)?) {
        return Err(StoreError::Conflict);
    }
    Ok(FileSnapshot {
        size,
        digest: Blake3Digest::new(*hasher.finalize().as_bytes()),
        ..initial
    })
}

fn metadata_matches(file: &File, expected: &FileSnapshot) -> StoreResult<bool> {
    let metadata = file.metadata().map_err(database::io_error)?;
    Ok(metadata.is_file() && same_metadata(&snapshot_metadata(&metadata)?, expected))
}

fn snapshot_metadata(metadata: &cap_std::fs::Metadata) -> StoreResult<FileSnapshot> {
    Ok(FileSnapshot {
        size: metadata.len(),
        digest: Blake3Digest::new([0; 32]),
        last_write: metadata_timestamp(metadata)?,
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

fn same_metadata(left: &FileSnapshot, right: &FileSnapshot) -> bool {
    left.size == right.size
        && left.last_write == right.last_write
        && left.device == right.device
        && left.inode == right.inode
}

fn map_io(error: io::Error) -> StoreError {
    if error.kind() == io::ErrorKind::StorageFull {
        StoreError::InsufficientStagingSpace
    } else {
        database::io_error(error)
    }
}

#[cfg(test)]
fn checkpoint(name: &'static str) {
    if let Some(hook) = CHECKPOINT_HOOK
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
    {
        hook(name);
    }
}

#[cfg(not(test))]
fn checkpoint(_: &'static str) {}

#[cfg(test)]
pub(crate) fn set_checkpoint_hook(hook: Option<CheckpointHook>) {
    *CHECKPOINT_HOOK
        .get_or_init(|| std::sync::Mutex::new(None))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = hook;
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::{checkpoint, set_checkpoint_hook};

    #[test]
    fn publication_checkpoint_hook_can_gate_test_races() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&seen);
        let test_thread = std::thread::current().id();
        set_checkpoint_hook(Some(Arc::new(move |name| {
            if std::thread::current().id() == test_thread {
                captured.lock().unwrap().push(name);
            }
        })));
        checkpoint("before_rename");
        set_checkpoint_hook(None);
        assert_eq!(*seen.lock().unwrap(), ["before_rename"]);
    }
}
