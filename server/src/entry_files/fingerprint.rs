use cap_fs_ext::MetadataExt;
use cap_std::fs::Dir;

use crate::{StoreError, StoreResult, database, directory_scan::Budget};

pub(super) fn directory_membership_stamp(
    directory: &Dir,
    budget: &mut Budget,
) -> StoreResult<(i64, i64)> {
    // Windows does not reliably advance a directory timestamp after a child is created.
    let names = budget.names(directory, false)?;
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"drip-directory-membership-v1\0");
    for name in names {
        let metadata = directory
            .symlink_metadata(&name)
            .map_err(database::io_error)?;
        let kind = if metadata.is_dir() {
            1
        } else if metadata.is_file() {
            2
        } else {
            return Err(StoreError::UnsupportedEntry);
        };
        let length = u64::try_from(name.len()).map_err(|_| StoreError::Conflict)?;
        hasher.update(&length.to_le_bytes());
        hasher.update(name.as_bytes());
        hasher.update(&[kind]);
        hasher.update(&metadata.dev().to_le_bytes());
        hasher.update(&metadata.ino().to_le_bytes());
        hasher.update(&metadata.len().to_le_bytes());
        let modified = crate::upload_times::metadata_timestamp(&metadata)?;
        hasher.update(&modified.unix_seconds().to_le_bytes());
        hasher.update(&modified.nanos().to_le_bytes());
    }
    let digest = hasher.finalize();
    let mut seconds = [0; 8];
    seconds.copy_from_slice(&digest.as_bytes()[..8]);
    let mut nanos = [0; 4];
    nanos.copy_from_slice(&digest.as_bytes()[8..12]);
    Ok((
        i64::from_le_bytes(seconds),
        i64::from(u32::from_le_bytes(nanos) % 1_000_000_000),
    ))
}
