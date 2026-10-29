use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use drip_core::SourcePath;

use crate::{
    engine::watcher::{ChangeEpoch, DirtySignal},
    settings::SavedSelection,
    source::Observation,
};

use super::{
    CacheLookup,
    database::{Binding, CacheDatabase, CacheRecord},
};

mod fallback;

#[derive(Default)]
pub(super) struct CacheState {
    database: Option<CacheDatabase>,
    path: Option<PathBuf>,
    unavailable: bool,
    warning_reported: bool,
    memory: HashMap<(String, SourcePath), CacheRecord>,
    binding: Option<Binding>,
    selections: HashMap<String, SavedSelection>,
    #[cfg(test)]
    fail_next_save: bool,
}

impl CacheState {
    pub fn reconcile(
        &mut self,
        path: Option<&Path>,
        binding: Binding,
        selections: Vec<SavedSelection>,
    ) -> Option<String> {
        let definitions = selection_map(&selections);
        if self
            .binding
            .as_ref()
            .is_some_and(|current| current != &binding)
        {
            self.memory.clear();
        } else {
            self.memory
                .retain(|(id, _), _| self.selections.get(id) == definitions.get(id));
        }
        self.binding = Some(binding.clone());
        self.selections = definitions;
        self.path = path.map(Path::to_owned);
        let path = path?;
        if self.unavailable {
            return None;
        }
        let result = match self.database.as_mut() {
            Some(database) => database.reconcile(&binding, &selections),
            None => CacheDatabase::open(path, &binding, &selections).map(|database| {
                self.database = Some(database);
            }),
        };
        self.handle_database_result(result)
    }

    pub fn get(
        &mut self,
        selection: &SavedSelection,
        path: &SourcePath,
        path_json: &str,
        observation: Observation,
        volume_identity: &str,
    ) -> CacheLookup {
        if !self.selection_is_current(selection) || observation.change_time == 0 {
            return miss(None);
        }
        let stored = if !self.unavailable {
            self.database
                .as_mut()
                .map(|database| database.get(selection, path_json))
        } else {
            None
        };
        match stored {
            Some(Ok(Some(record))) => {
                if record.matches(selection, path, observation, volume_identity) {
                    return CacheLookup {
                        digest: Some(record.digest),
                        warning: None,
                    };
                }
                let deleted = self
                    .database
                    .as_mut()
                    .map(|database| database.delete(&selection.id, path_json))
                    .unwrap_or(Ok(()));
                if let Err(error) = deleted {
                    return miss(self.database_failure(error));
                }
            }
            Some(Ok(None)) | None => {}
            Some(Err(error)) if error.is_corrupt() => {
                return miss(self.recover_database(error));
            }
            Some(Err(error)) => return miss(self.database_failure(error)),
        }
        let digest = self
            .memory
            .get(&(selection.id.clone(), path.clone()))
            .filter(|record| record.matches(selection, path, observation, volume_identity))
            .map(|record| record.digest);
        CacheLookup {
            digest,
            warning: None,
        }
    }

    pub fn remember(
        &mut self,
        path: Option<&Path>,
        selection: &SavedSelection,
        source_path: SourcePath,
        record: CacheRecord,
    ) -> Option<String> {
        if !self.selection_is_current(selection) || record.observation.change_time == 0 {
            return None;
        }
        self.memory
            .insert((selection.id.clone(), source_path.clone()), record.clone());
        #[cfg(test)]
        if std::mem::take(&mut self.fail_next_save) {
            return self.database_failure("injected SQLITE_FULL capacity failure");
        }
        if self.unavailable {
            return None;
        }
        let path = path?;
        let result = if let Some(database) = self.database.as_mut() {
            database.save(selection, &source_path, &record)
        } else {
            let binding = self.binding.as_ref()?;
            let selections = self.selections.values().cloned().collect::<Vec<_>>();
            match CacheDatabase::open(path, binding, &selections) {
                Ok(mut database) => match database.save(selection, &source_path, &record) {
                    Ok(()) => {
                        self.database = Some(database);
                        Ok(())
                    }
                    Err(error) => Err(error),
                },
                Err(error) => Err(error),
            }
        };
        if let Err(error) = result {
            if error.is_corrupt() {
                let warning = self.recover_database(error);
                let retry = self
                    .database
                    .as_mut()
                    .map(|database| database.save(selection, &source_path, &record))
                    .unwrap_or(Ok(()));
                return warning.or_else(|| self.handle_database_result(retry));
            }
            return self.database_failure(error);
        }
        self.warning_reported = false;
        None
    }

    pub fn prune(
        &mut self,
        selection_id: &str,
        paths: &[String],
        signal: &DirtySignal,
        epoch: ChangeEpoch,
    ) -> Option<String> {
        let keep: HashSet<_> = paths.iter().map(String::as_str).collect();
        let result = if self.unavailable {
            None
        } else {
            self.database
                .as_mut()
                .map(|database| database.prune(selection_id, paths, signal, epoch))
        };
        match result {
            Some(Ok(true)) => {
                self.prune_memory(selection_id, &keep);
                None
            }
            Some(Ok(false)) => None,
            Some(Err(error)) => {
                let warning = self.handle_database_result(Err(error));
                signal.while_unchanged(selection_id, epoch, || {
                    self.prune_memory(selection_id, &keep)
                });
                warning
            }
            None => {
                signal.while_unchanged(selection_id, epoch, || {
                    self.prune_memory(selection_id, &keep)
                });
                None
            }
        }
    }

    fn prune_memory(&mut self, selection_id: &str, keep: &HashSet<&str>) {
        self.memory.retain(|(id, path), _| {
            id != selection_id
                || serde_json::to_string(path).is_ok_and(|path| keep.contains(path.as_str()))
        });
    }

    fn selection_is_current(&self, selection: &SavedSelection) -> bool {
        selection.active && self.selections.get(&selection.id) == Some(selection)
    }

    #[cfg(test)]
    pub fn fail_next_save_for_test(&mut self) {
        self.fail_next_save = true;
    }
}

fn miss(warning: Option<String>) -> CacheLookup {
    CacheLookup {
        digest: None,
        warning,
    }
}

fn selection_map(selections: &[SavedSelection]) -> HashMap<String, SavedSelection> {
    selections
        .iter()
        .filter(|selection| selection.active)
        .map(|selection| (selection.id.clone(), selection.clone()))
        .collect()
}
