mod context;
mod database;
mod state;
#[cfg(test)]
mod tests;

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use drip_core::{Blake3Digest, SourcePath};

use crate::{
    credentials::Pairing,
    settings::{SavedSelection, SettingsStore},
    source::{Observation, ScannedEntry, SourceFile},
};

use self::{
    context::{active_selections, verified_record},
    database::Binding,
    state::CacheState,
};

#[derive(Default)]
pub(super) struct FileCache {
    path: Option<PathBuf>,
    state: Arc<Mutex<CacheState>>,
}

pub(super) struct CacheLookup {
    pub digest: Option<Blake3Digest>,
    pub warning: Option<String>,
}

impl FileCache {
    pub fn beside_settings(store: &SettingsStore) -> Self {
        Self {
            path: Some(store.directory().join("drip.state.sqlite3")),
            state: Arc::default(),
        }
    }

    pub async fn reconcile(
        &self,
        pairing: &Pairing,
        selections: &[SavedSelection],
    ) -> Option<String> {
        let binding = Binding::from(pairing);
        let selections = active_selections(selections);
        let path = self.path.clone();
        let state = Arc::clone(&self.state);
        tokio::task::spawn_blocking(move || {
            let mut state = lock(&state);
            state.reconcile(path.as_deref(), binding, selections)
        })
        .await
        .unwrap_or_else(|error| Some(format!("persistent fingerprint store failed: {error}")))
    }

    pub async fn get(
        &self,
        selection: &SavedSelection,
        source: &SourceFile,
        audit: bool,
    ) -> CacheLookup {
        let path_json = match serde_json::to_string(source.protocol_path()) {
            Ok(path) => path,
            Err(error) => {
                return CacheLookup {
                    digest: None,
                    warning: Some(format!("could not encode cached source path: {error}")),
                };
            }
        };
        if audit || source.observation().change_time == 0 {
            return CacheLookup {
                digest: None,
                warning: None,
            };
        }
        let selection = selection.clone();
        let source_path = source.protocol_path().clone();
        let observation = source.observation();
        let volume_identity = source.volume_identity().to_owned();
        let state = Arc::clone(&self.state);
        tokio::task::spawn_blocking(move || {
            let mut state = lock(&state);
            state.get(
                &selection,
                &source_path,
                &path_json,
                observation,
                &volume_identity,
            )
        })
        .await
        .unwrap_or_else(|error| CacheLookup {
            digest: None,
            warning: Some(format!("persistent fingerprint lookup failed: {error}")),
        })
    }

    pub async fn remember(
        &self,
        selection: &SavedSelection,
        path: &SourcePath,
        observation: Observation,
        volume_identity: &str,
        digest: Blake3Digest,
    ) -> Option<String> {
        let record = match verified_record(path, observation, volume_identity, digest) {
            Ok(record) => record,
            Err(error) => return Some(error),
        };
        let database_path = self.path.clone();
        let selection = selection.clone();
        let source_path = record.path.clone();
        let state = Arc::clone(&self.state);
        tokio::task::spawn_blocking(move || {
            lock(&state).remember(database_path.as_deref(), &selection, source_path, record)
        })
        .await
        .unwrap_or_else(|error| Some(format!("persistent fingerprint write failed: {error}")))
    }

    pub async fn prune(&self, selection_id: &str, entries: &[ScannedEntry]) -> Option<String> {
        let paths = match entries
            .iter()
            .map(|entry| serde_json::to_string(&entry.source_path))
            .collect::<Result<Vec<_>, _>>()
        {
            Ok(paths) => paths,
            Err(error) => return Some(format!("could not encode scanned source paths: {error}")),
        };
        let selection_id = selection_id.to_owned();
        let state = Arc::clone(&self.state);
        tokio::task::spawn_blocking(move || lock(&state).prune(&selection_id, &paths))
            .await
            .unwrap_or_else(|error| Some(format!("persistent fingerprint pruning failed: {error}")))
    }
}

fn lock(state: &Mutex<CacheState>) -> std::sync::MutexGuard<'_, CacheState> {
    state.lock().unwrap_or_else(|error| error.into_inner())
}
