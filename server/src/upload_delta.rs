mod accept;
mod chunks;
mod manifest;
mod pages;
mod ranges;
#[cfg(test)]
mod recovery_tests;
mod state;

use std::sync::{Mutex, MutexGuard};

pub(crate) use manifest::record_committed_manifest;
pub(crate) use state::{delete_plan, require_complete, require_complete_transaction};

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub(crate) fn io_error(error: std::io::Error) -> crate::StoreError {
    if error.kind() == std::io::ErrorKind::StorageFull {
        crate::StoreError::InsufficientStagingSpace
    } else if matches!(
        error.kind(),
        std::io::ErrorKind::Interrupted
            | std::io::ErrorKind::WouldBlock
            | std::io::ErrorKind::TimedOut
            | std::io::ErrorKind::Other
    ) {
        crate::StoreError::RetryableIo
    } else {
        crate::database::io_error(error)
    }
}
