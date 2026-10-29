use super::CacheState;
use crate::engine::cache::database::{CacheDatabase, DatabaseError};

impl CacheState {
    pub(super) fn recover_save(
        &mut self,
        error: DatabaseError,
        selection: &crate::settings::SavedSelection,
        source_path: &drip_core::SourcePath,
        record: &crate::engine::cache::database::CacheRecord,
    ) -> Option<String> {
        let warning = self.recover_database(error);
        let Some(database) = self.database.as_mut() else {
            return warning;
        };
        let retry = database.save(selection, source_path, record);
        self.warning_reported = false;
        self.handle_database_result(retry).or(warning)
    }

    pub(super) fn handle_database_result(
        &mut self,
        result: Result<(), DatabaseError>,
    ) -> Option<String> {
        match result {
            Ok(()) => {
                self.warning_reported = false;
                None
            }
            Err(error) if error.is_corrupt() => self.recover_database(error),
            Err(error) => self.database_failure(error),
        }
    }

    pub(super) fn recover_database(&mut self, error: DatabaseError) -> Option<String> {
        self.database = None;
        let Some(path) = self.path.clone() else {
            return self.database_failure(error);
        };
        let selections = self.selections.values().cloned().collect::<Vec<_>>();
        match CacheDatabase::recover(&path, self.binding.as_ref(), &selections) {
            Ok(database) => {
                self.database = Some(database);
                self.warning_reported = false;
                self.warn_once(format!(
                    "persistent fingerprint store was corrupt and has been rebuilt; prior database preserved: {error}"
                ))
            }
            Err(error) => self.database_failure(error),
        }
    }

    pub(super) fn database_failure(&mut self, error: impl std::fmt::Display) -> Option<String> {
        self.database = None;
        self.unavailable = true;
        self.warn_once(format!(
            "persistent fingerprint store disabled; this session keeps only newly verified in-memory records, and another content scan may be needed after restart: {error}"
        ))
    }

    fn warn_once(&mut self, diagnostic: String) -> Option<String> {
        if self.warning_reported {
            None
        } else {
            self.warning_reported = true;
            Some(diagnostic)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::cache::database::CacheRecord;
    use drip_core::{Blake3Digest, SourcePath, Timestamp};

    #[test]
    fn failed_write_after_recovery_disables_backend_despite_recovery_warning() {
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join(".verification/persistent-cache/cache-tests")
            .join(crate::source::random_id().unwrap());
        std::fs::create_dir_all(&directory).unwrap();
        let path = SourcePath::new('C', vec!["file.txt".into()]).unwrap();
        let selection = crate::settings::SavedSelection {
            id: "a".repeat(32),
            source: path.clone(),
            root_kind: drip_core::protocol::EntryKind::File,
            volume_identity: "volume".into(),
            root_identity: None,
            active: true,
        };
        let record = CacheRecord {
            path: path.clone(),
            observation: crate::source::Observation {
                volume: 1,
                file_id: [1; 16],
                size: 1,
                last_write: Timestamp::new(0, 0).unwrap(),
                change_time: 1,
            },
            volume_identity: "volume".into(),
            digest: Blake3Digest::new([1; 32]),
            verified_at: Timestamp::new(0, 0).unwrap(),
        };
        let mut state = CacheState {
            path: Some(directory.join("drip.state.sqlite3")),
            ..Default::default()
        };
        // Missing selection context makes the retry fail its actual SQLite foreign key.
        let warning = state
            .recover_save(
                DatabaseError::Corrupt("test".into()),
                &selection,
                &path,
                &record,
            )
            .unwrap();

        assert!(state.unavailable);
        assert!(state.database.is_none());
        assert!(warning.contains("disabled"));
        assert!(warning.contains("FOREIGN KEY"));
        std::fs::remove_dir_all(directory).unwrap();
    }
}
