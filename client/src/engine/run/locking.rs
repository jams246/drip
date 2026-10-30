use super::{EngineError, RunControl};
use std::{
    fs::{File, OpenOptions},
    os::windows::fs::{MetadataExt, OpenOptionsExt},
};
use tokio::sync::watch;
use windows::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT,
};

pub(super) fn worker_lock(directory: &std::path::Path) -> Result<File, EngineError> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
        .open(directory.join("drip.run.lock"))
        .map_err(|error| {
            std::io::Error::new(
                error.kind(),
                format!(
                    "another sync worker may be running, or drip.run.lock is inaccessible: {error}"
                ),
            )
        })
        .map_err(EngineError::Io)?;
    let metadata = file.metadata().map_err(EngineError::Io)?;
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        return Err(EngineError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "drip.run.lock is not a regular file",
        )));
    }
    Ok(file)
}

pub(super) fn pristine_stop(
    store: &crate::settings::SettingsStore,
    control: &watch::Receiver<RunControl>,
    closed: bool,
) -> bool {
    let stopping = control.borrow().stop || closed;
    stopping && store.pristine_shutdown_state().unwrap_or(false)
}
