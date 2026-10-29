use super::CacheState;
use crate::engine::cache::database::{CacheDatabase, DatabaseError};

impl CacheState {
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
