mod deleted;
mod refresh;
mod shutdown;
mod startup;
mod ui;

use std::{
    fs::{File, OpenOptions},
    io,
    os::windows::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    thread,
    time::Duration,
};

use drip_core::protocol::EntryKind;
use thiserror::Error;
use windows::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT,
};

use super::{ClientSettings, PendingSelectionUpdate, selection, storage};
use crate::source::SourceRoot;

/// Atomic settings persistence beside the executable.
pub struct SettingsStore {
    directory: PathBuf,
    path: PathBuf,
}

impl SettingsStore {
    /// Opens `drip.json` in an executable directory.
    pub fn open(directory: &Path) -> Self {
        Self {
            directory: directory.to_owned(),
            path: directory.join("drip.json"),
        }
    }

    /// Locates the executable's directory and opens its settings store.
    pub fn beside_executable() -> Result<Self, SettingsError> {
        let executable = std::env::current_exe().map_err(SettingsError::ExecutablePath)?;
        let directory = executable
            .parent()
            .ok_or(SettingsError::MissingExecutableDirectory)?;
        Ok(Self::open(directory))
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// Loads settings, returning defaults when `drip.json` does not exist.
    pub fn load(&self) -> Result<ClientSettings, SettingsError> {
        let _lock = self.lock()?;
        self.load_unlocked()
    }

    /// Validates and atomically replaces `drip.json` with a protected file.
    pub fn save(&self, settings: &ClientSettings) -> Result<(), SettingsError> {
        let _lock = self.lock()?;
        self.save_unlocked(settings)
    }

    fn save_unlocked(&self, settings: &ClientSettings) -> Result<(), SettingsError> {
        super::validation::validate(settings)?;
        let bytes = serde_json::to_vec_pretty(settings).map_err(SettingsError::Encode)?;
        storage::replace_with(&self.path, &bytes)
    }

    /// Adds or reactivates a source and stages its server selection update.
    pub fn add(&self, root: SourceRoot) -> Result<selection::AddSelectionResult, SettingsError> {
        let _lock = self.lock()?;
        let mut settings = self.load_unlocked()?;
        let result = selection::add(&mut settings, root)?;
        self.save_unlocked(&settings)?;
        Ok(result)
    }

    /// Deactivates a source while retaining its mirror identity.
    pub fn remove(&self, source: &drip_core::SourcePath) -> Result<bool, SettingsError> {
        let _lock = self.lock()?;
        let mut settings = self.load_unlocked()?;
        let changed = selection::remove(&mut settings, source)?;
        if changed {
            self.save_unlocked(&settings)?;
        }
        Ok(changed)
    }

    /// Records a server acknowledgement without erasing a newer local update.
    pub fn acknowledge_update(
        &self,
        sent: &PendingSelectionUpdate,
        generation: u64,
    ) -> Result<(), SettingsError> {
        let _lock = self.lock()?;
        let mut settings = self.load_unlocked()?;
        if sent.expected_generation.checked_add(1) != Some(generation) {
            return Err(SettingsError::UnexpectedGeneration);
        }
        if settings.pending_update.as_ref() == Some(sent) {
            settings.pending_update = None;
        } else if settings.pending_update.is_some() {
            selection::restage_after_ack(&mut settings, generation)?;
        } else if settings.acknowledged_generation != generation {
            return Err(SettingsError::UnexpectedGeneration);
        }
        settings.acknowledged_generation = generation;
        self.save_unlocked(&settings)
    }

    /// Stages the complete local selection set against the server generation.
    pub fn prepare_update(
        &self,
        server_generation: u64,
    ) -> Result<PendingSelectionUpdate, SettingsError> {
        let _lock = self.lock()?;
        let mut settings = self.load_unlocked()?;
        settings.acknowledged_generation = server_generation;
        selection::restage_after_ack(&mut settings, server_generation)?;
        let pending = settings
            .pending_update
            .clone()
            .ok_or(SettingsError::NoPendingUpdate)?;
        self.save_unlocked(&settings)?;
        Ok(pending)
    }

    /// Stages a fresh root kind confirmed by source inspection.
    pub fn refresh_root_kind(&self, id: &str, kind: EntryKind) -> Result<bool, SettingsError> {
        let _lock = self.lock()?;
        let mut settings = self.load_unlocked()?;
        let changed = selection::refresh_root_kind(&mut settings, id, kind)?;
        if changed {
            self.save_unlocked(&settings)?;
        }
        Ok(changed)
    }

    fn lock(&self) -> Result<File, SettingsError> {
        let path = self.directory.join("drip.settings.lock");
        // 500 x 10 ms: a holder that never releases must surface as an error, not a hang.
        for attempt in 1..=500 {
            match OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .share_mode(0)
                .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT.0)
                .open(&path)
            {
                Ok(file) => {
                    let metadata = file
                        .metadata()
                        .map_err(|source| path_error("inspect lock", &path, source))?;
                    if !metadata.is_file()
                        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
                    {
                        return Err(SettingsError::InvalidTarget(path));
                    }
                    return Ok(file);
                }
                Err(error) if matches!(error.raw_os_error(), Some(32 | 33)) && attempt < 500 => {
                    thread::sleep(Duration::from_millis(10))
                }
                Err(source) => return Err(path_error("lock", &path, source)),
            }
        }
        unreachable!("the final attempt returns")
    }
}

pub(super) fn path_error(action: &'static str, path: &Path, source: io::Error) -> SettingsError {
    SettingsError::Io {
        action,
        path: path.to_owned(),
        source,
    }
}

#[derive(Debug, Error)]
pub enum SettingsError {
    #[error("could not locate executable path: {0}")]
    ExecutablePath(#[source] io::Error),
    #[error("executable path has no parent directory")]
    MissingExecutableDirectory,
    #[error("could not {action} {path}: {source}")]
    Io {
        action: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not decode drip.json: {0}")]
    Decode(#[source] serde_json::Error),
    #[error("could not encode drip.json: {0}")]
    Encode(#[source] serde_json::Error),
    #[error("drip.json is not a regular file: {0}")]
    InvalidTarget(PathBuf),
    #[error("could not create a unique temporary drip.json file beside {0}")]
    TemporaryNameExhausted(PathBuf),
    #[error("unsupported drip.json schema version")]
    UnsupportedVersion,
    #[error("invalid client selection state: {0}")]
    InvalidState(&'static str),
    #[error("no selection update is pending")]
    NoPendingUpdate,
    #[error("server acknowledged an unexpected selection generation")]
    UnexpectedGeneration,
    #[error("Windows sign-in startup registration belongs to another executable")]
    StartupConflict,
    #[error("could not update Windows sign-in startup: {0}")]
    Startup(#[source] io::Error),
    #[error("could not encode the executable path for Windows sign-in startup")]
    StartupPath,
    #[error(
        "Windows sign-in command exceeds the 260-character Run-key limit; move Drip to a shorter path"
    )]
    StartupPathTooLong,
    #[error(
        "startup change failed: {operation}; restoring the prior registration failed: {rollback}"
    )]
    StartupRollback {
        operation: Box<SettingsError>,
        rollback: io::Error,
    },
    #[error("settings operation failed: {0}")]
    Selection(#[from] selection::SelectionError),
}
