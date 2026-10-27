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
    } else if error.raw_os_error() == Some(rustix::io::Errno::IO.raw_os_error())
        || matches!(
            error.kind(),
            std::io::ErrorKind::Interrupted
                | std::io::ErrorKind::WouldBlock
                | std::io::ErrorKind::TimedOut
                | std::io::ErrorKind::Other
        )
    {
        crate::StoreError::RetryableIo
    } else {
        crate::database::io_error(error)
    }
}

#[cfg(all(test, target_os = "linux"))]
mod io_tests {
    #[test]
    fn linux_eio_is_a_retryable_io_failure() {
        let failure = std::io::Error::from_raw_os_error(rustix::io::Errno::IO.raw_os_error());
        assert!(matches!(
            super::io_error(failure),
            crate::StoreError::RetryableIo
        ));
    }
}
