#![expect(
    unsafe_code,
    reason = "Windows mutex creation requires a raw object-name pointer"
)]

//! Per-user Windows process coordination and sign-in startup integration.

mod identity;
mod ipc;
pub(crate) mod startup;

use std::{
    io,
    os::windows::io::{FromRawHandle, OwnedHandle},
    path::Path,
};

use thiserror::Error;
use windows::{
    Win32::{
        Foundation::{ERROR_ALREADY_EXISTS, GetLastError},
        System::Threading::CreateMutexW,
    },
    core::PCWSTR,
};

pub use ipc::OpenListener;

/// Binary role that requests process-wide ownership.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstanceMode {
    Gui,
    Diagnostic,
}

/// Result of acquiring the per-user, per-installation engine mutex.
pub enum InstanceAcquire {
    Acquired(InstanceGuard),
    AlreadyRunning,
}

/// Keeps the process mutex and initial GUI pipe instance alive.
pub struct InstanceGuard {
    listener: Option<OpenListener>,
    _mutex: OwnedHandle,
}

impl InstanceGuard {
    /// Moves out the GUI Open listener for a cancellable background task.
    pub fn take_open_listener(&mut self) -> Option<OpenListener> {
        self.listener.take()
    }
}

/// Errors from Windows process coordination and same-user IPC.
#[derive(Debug, Error)]
pub enum InstanceError {
    #[error("could not identify the current Windows user: {0}")]
    CurrentUser(#[source] io::Error),
    #[error("could not canonicalize app data directory {path}: {source}")]
    DataDirectory { path: String, source: io::Error },
    #[error("another Drip instance already owns this app directory")]
    AlreadyRunning,
    #[error(
        "another Drip engine owns this app directory, but no GUI Open listener is available; stop diagnostic mode or restart the existing GUI"
    )]
    OpenListenerUnavailable,
    #[error("the existing GUI could not queue the Open request")]
    OpenRequestRejected,
    #[error("timed out waiting for the existing GUI to accept the Open request")]
    OpenRequestTimeout,
    #[error("could not reach the existing GUI Open channel: {0}")]
    OpenRequestFailed(#[source] io::Error),
    #[error("Windows process coordination failed: {0}")]
    Windows(#[source] windows::core::Error),
    #[error("Windows named pipe failed: {0}")]
    Pipe(#[source] io::Error),
}

/// Acquires the engine mutex or asks an existing GUI instance to open.
pub async fn acquire(
    directory: &Path,
    mode: InstanceMode,
) -> Result<InstanceAcquire, InstanceError> {
    let key = identity::instance_key(directory)?;
    let name: Vec<u16> = key.mutex_name.encode_utf16().chain(Some(0)).collect();
    // SAFETY: `name` is a live, NUL-terminated Windows object name.
    let mutex = unsafe { CreateMutexW(None, false, PCWSTR(name.as_ptr())) }
        .map_err(InstanceError::Windows)?;
    let existed = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    // SAFETY: CreateMutexW returned a new owned kernel handle.
    let mutex = unsafe { OwnedHandle::from_raw_handle(mutex.0) };

    if existed {
        if mode == InstanceMode::Gui {
            match ipc::request_open(&key.pipe_name).await {
                Ok(()) => {}
                Err(ipc::OpenRequestError::Unavailable) => {
                    return Err(InstanceError::OpenListenerUnavailable);
                }
                Err(ipc::OpenRequestError::Rejected) => {
                    return Err(InstanceError::OpenRequestRejected);
                }
                Err(ipc::OpenRequestError::Timeout) => {
                    return Err(InstanceError::OpenRequestTimeout);
                }
                Err(ipc::OpenRequestError::Transport(error)) => {
                    return Err(InstanceError::OpenRequestFailed(error));
                }
            }
        }
        return Ok(InstanceAcquire::AlreadyRunning);
    }

    let listener = if mode == InstanceMode::Gui {
        Some(ipc::create_listener(&key.pipe_name).map_err(InstanceError::Pipe)?)
    } else {
        None
    };
    Ok(InstanceAcquire::Acquired(InstanceGuard {
        listener,
        _mutex: mutex,
    }))
}

#[cfg(test)]
mod tests;
