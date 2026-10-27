use std::io::{Read, Seek, SeekFrom, Write};

use cap_std::fs::{Dir, File};
use drip_core::{Blake3Digest, SourcePath};

use crate::{StoreError, StoreResult, upload_files::FileSnapshot, upload_paths};

pub(crate) fn baseline_file(
    directory: &Dir,
    source: &SourcePath,
    expected: Option<&FileSnapshot>,
) -> Option<File> {
    let (parent, name) = upload_paths::open_parent(directory, source, false).ok()??;
    let file = upload_paths::open_file(&parent, &name).ok()??;
    if expected
        .is_some_and(|snapshot| crate::upload_files::check_open_file(&file, snapshot).is_err())
    {
        return None;
    }
    Some(file)
}

pub(crate) fn copy_range(
    baseline: &mut File,
    stage: &mut File,
    baseline_offset: u64,
    target_offset: u64,
    length: u32,
    digest: Blake3Digest,
) -> StoreResult<bool> {
    if baseline.seek(SeekFrom::Start(baseline_offset)).is_err() {
        return Ok(false);
    }
    stage
        .seek(SeekFrom::Start(target_offset))
        .map_err(super::io_error)?;
    let mut hasher = blake3::Hasher::new();
    let mut remaining = length;
    let mut buffer = [0u8; 64 * 1024];
    while remaining > 0 {
        let limit = usize::try_from(remaining.min(buffer.len() as u32))
            .map_err(|_| StoreError::Conflict)?;
        let count = match baseline.read(&mut buffer[..limit]) {
            Ok(0) | Err(_) => return Ok(false),
            Ok(count) => count,
        };
        stage.write_all(&buffer[..count]).map_err(super::io_error)?;
        hasher.update(&buffer[..count]);
        remaining = remaining
            .checked_sub(u32::try_from(count).map_err(|_| StoreError::Conflict)?)
            .ok_or(StoreError::Conflict)?;
    }
    Ok(Blake3Digest::new(*hasher.finalize().as_bytes()) == digest)
}
