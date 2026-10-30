use std::{fs::File, io, sync::Mutex};

use cap_std::fs::Dir;

use crate::{Store, StoreError, StoreResult, database};

pub(crate) struct DaemonLease {
    _files: Vec<File>,
}

impl DaemonLease {
    pub(super) fn acquire(state: &Dir, mirror: &Dir) -> StoreResult<Self> {
        let state_file = lock_file(state)?;
        let mirror_file = lock_file(mirror)?;
        let same_directory = same_directory(state, mirror)?;
        let files = if same_directory {
            vec![state_file]
        } else {
            vec![state_file, mirror_file]
        };
        for file in &files {
            file.try_lock().map_err(|error| match error {
                std::fs::TryLockError::WouldBlock => database::io_error(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "another daemon owns the server state or mirror directory",
                )),
                std::fs::TryLockError::Error(error) => database::io_error(error),
            })?;
        }
        Ok(Self { _files: files })
    }
}

fn lock_file(directory: &Dir) -> StoreResult<File> {
    super::security::private_file(directory, ".drip-daemon.lock", true)?.ok_or(StoreError::Conflict)
}

fn same_directory(left: &Dir, right: &Dir) -> StoreResult<bool> {
    use cap_fs_ext::MetadataExt;
    let left = left.dir_metadata().map_err(database::io_error)?;
    let right = right.dir_metadata().map_err(database::io_error)?;
    Ok(left.dev() == right.dev() && left.ino() == right.ino())
}

impl Store {
    /// Opens storage after acquiring exclusive daemon ownership of both roots.
    pub fn open_for_daemon(
        state_directory: impl AsRef<std::path::Path>,
        mirror_directory: impl AsRef<std::path::Path>,
    ) -> StoreResult<Self> {
        let state_directory = state_directory.as_ref();
        let mirror_directory = mirror_directory.as_ref();
        let state = super::security::root(state_directory)?;
        let mirror = super::security::root(mirror_directory)?;
        let lease = DaemonLease::acquire(&state, &mirror)?;
        let mut store = Self::open(state_directory, mirror_directory)?;
        store.daemon_lease = std::sync::Arc::new(Mutex::new(Some(lease)));
        Ok(store)
    }

    pub(crate) fn take_daemon_lease(&self) -> StoreResult<DaemonLease> {
        let mut pending = self
            .daemon_lease
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(lease) = pending.take() {
            return Ok(lease);
        }
        DaemonLease::acquire(&self.state_directory, &self.mirror)
    }
}

#[cfg(test)]
mod tests;
