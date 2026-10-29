mod files;
mod recovery;
mod schema;
mod selection;

use std::fs::File;
use std::path::{Path, PathBuf};

use drip_core::{Blake3Digest, SourcePath, Timestamp};
use rusqlite::{Connection, OpenFlags};

use crate::{
    credentials::Pairing,
    engine::watcher::{ChangeEpoch, DirtySignal},
    settings::SavedSelection,
    source::Observation,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Binding {
    pub(super) server_url: String,
    pub(super) pc_id: String,
    pub(super) mirror_directory: String,
}

impl From<&Pairing> for Binding {
    fn from(pairing: &Pairing) -> Self {
        Self {
            server_url: pairing.server_url().to_owned(),
            pc_id: pairing.pc_id().to_owned(),
            mirror_directory: pairing.mirror_directory().to_owned(),
        }
    }
}

impl Binding {
    const FINGERPRINT_FORMAT: &'static str = "blake3-v1";
}

#[derive(Clone)]
pub(super) struct CacheRecord {
    pub path: SourcePath,
    pub observation: Observation,
    pub volume_identity: String,
    pub digest: Blake3Digest,
    pub verified_at: Timestamp,
}

impl CacheRecord {
    pub fn matches(
        &self,
        selection: &SavedSelection,
        path: &SourcePath,
        observation: Observation,
        volume_identity: &str,
    ) -> bool {
        self.observation.change_time != 0
            && self.observation == observation
            && self.volume_identity == volume_identity
            && self.volume_identity == selection.volume_identity
            && self.path == *path
            && path_belongs_to_selection(path, selection)
    }
}

pub(super) struct CacheDatabase {
    path: PathBuf,
    connection: Connection,
    _file_guard: File,
}

impl CacheDatabase {
    pub fn open(
        path: &Path,
        binding: &Binding,
        selections: &[SavedSelection],
    ) -> Result<Self, DatabaseError> {
        let mut database = Self::open_once(path)?;
        database.reconcile(binding, selections)?;
        Ok(database)
    }

    pub fn recover(
        path: &Path,
        binding: Option<&Binding>,
        selections: &[SavedSelection],
    ) -> Result<Self, DatabaseError> {
        recovery::preserve(path)?;
        let mut database = Self::open_fresh(path)?;
        if let Some(binding) = binding {
            database.reconcile(binding, selections)?;
        }
        Ok(database)
    }

    fn open_once(path: &Path) -> Result<Self, DatabaseError> {
        let exists = recovery::preflight(path)?;
        schema::inspect(path, exists)?;
        if !exists {
            recovery::create_empty(path)?;
        }
        recovery::secure_database(path)?;
        let file_guard = recovery::open_guard(path)?;
        let mut connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        recovery::preflight(path)?;
        schema::configure(&connection)?;
        let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version == 0 {
            recovery::prepare_journal(path)?;
        }
        schema::ensure_v1(&mut connection)?;
        recovery::secure_database(path)?;
        Ok(Self {
            path: path.to_owned(),
            connection,
            _file_guard: file_guard,
        })
    }

    fn open_fresh(path: &Path) -> Result<Self, DatabaseError> {
        recovery::create_empty(path)?;
        let file_guard = recovery::open_guard(path)?;
        let mut connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        schema::configure(&connection)?;
        recovery::prepare_journal(path)?;
        schema::create_v1(&mut connection)?;
        recovery::secure_database(path)?;
        Ok(Self {
            path: path.to_owned(),
            connection,
            _file_guard: file_guard,
        })
    }

    pub fn reconcile(
        &mut self,
        binding: &Binding,
        selections: &[SavedSelection],
    ) -> Result<(), DatabaseError> {
        if selection::is_current(&self.connection, binding, selections)? {
            return Ok(());
        }
        recovery::prepare_journal(&self.path)?;
        selection::reconcile(&mut self.connection, binding, selections)
    }

    pub fn get(
        &mut self,
        selection: &SavedSelection,
        source_json: &str,
    ) -> Result<Option<CacheRecord>, DatabaseError> {
        files::get(&mut self.connection, &self.path, selection, source_json)
    }

    pub fn delete(&mut self, selection_id: &str, source_json: &str) -> Result<(), DatabaseError> {
        recovery::prepare_journal(&self.path)?;
        files::delete(&self.connection, selection_id, source_json)
    }

    pub fn save(
        &mut self,
        selection: &SavedSelection,
        path: &SourcePath,
        record: &CacheRecord,
    ) -> Result<(), DatabaseError> {
        recovery::prepare_journal(&self.path)?;
        files::save(&mut self.connection, selection, path, record)
    }

    pub fn prune(
        &mut self,
        selection_id: &str,
        paths: &[String],
        signal: &DirtySignal,
        epoch: ChangeEpoch,
    ) -> Result<bool, DatabaseError> {
        recovery::prepare_journal(&self.path)?;
        files::prune(&mut self.connection, selection_id, paths, signal, epoch)
    }
}

#[cfg(test)]
pub(super) fn prepare_journal_for_test(path: &Path) -> Result<(), DatabaseError> {
    recovery::prepare_journal(path)
}

#[derive(Debug, thiserror::Error)]
pub(super) enum DatabaseError {
    #[error("SQLite operation failed: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("persistent fingerprint database is corrupt: {0}")]
    Corrupt(String),
    #[error("persistent fingerprint store is unsupported: {0}")]
    Unsupported(String),
    #[error("persistent fingerprint path is unsafe: {0}")]
    UnsafePath(String),
    #[error("persistent fingerprint recovery failed: {0}")]
    Recovery(String),
}

impl DatabaseError {
    pub fn is_corrupt(&self) -> bool {
        match self {
            Self::Corrupt(_) => true,
            Self::Sqlite(error) => error.sqlite_error_code().is_some_and(|code| {
                matches!(
                    code,
                    rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase
                )
            }),
            _ => false,
        }
    }
}

pub(super) fn path_belongs_to_selection(path: &SourcePath, selection: &SavedSelection) -> bool {
    match selection.root_kind {
        drip_core::protocol::EntryKind::File => crate::source::same_path(path, &selection.source),
        drip_core::protocol::EntryKind::Directory => path.has_windows_prefix(&selection.source),
    }
}
