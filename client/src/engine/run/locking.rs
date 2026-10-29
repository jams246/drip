use super::{EngineError, RunControl};
use std::{
    fs::{File, OpenOptions},
    os::windows::fs::OpenOptionsExt,
};
use tokio::sync::watch;

pub(super) fn worker_lock(directory: &std::path::Path) -> Result<File, EngineError> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .open(directory.join("drip.run.lock"))
        .map_err(|error| {
            std::io::Error::new(
                error.kind(),
                format!(
                    "another sync worker may be running, or drip.run.lock is inaccessible: {error}"
                ),
            )
        })
        .map_err(EngineError::Io)
}

pub(super) fn pristine_stop(
    store: &crate::settings::SettingsStore,
    control: &watch::Receiver<RunControl>,
    closed: bool,
) -> bool {
    let stopping = control.borrow().stop || closed;
    stopping && store.pristine_shutdown_state().unwrap_or(false)
}
