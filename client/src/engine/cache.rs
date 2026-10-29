use std::{
    collections::{HashMap, HashSet},
    sync::{Mutex, MutexGuard},
};

use drip_core::{Blake3Digest, SourcePath};

use crate::{
    settings::SavedSelection,
    source::{Observation, ScannedEntry, SourceFile},
};

struct CachedFile {
    observation: Observation,
    volume_identity: String,
    digest: Blake3Digest,
}

#[derive(Default)]
pub(super) struct FileCache(Mutex<HashMap<(String, SourcePath), CachedFile>>);

impl FileCache {
    fn entries(&self) -> MutexGuard<'_, HashMap<(String, SourcePath), CachedFile>> {
        self.0.lock().unwrap_or_else(|poisoned| {
            let mut entries = poisoned.into_inner();
            entries.clear();
            entries
        })
    }

    pub fn get(&self, selection: &str, source: &SourceFile) -> Option<Blake3Digest> {
        let observation = source.observation();
        // shortcut: metadata cannot detect silent corruption, use a full rescan to audit contents.
        if observation.change_time == 0 {
            return None;
        }
        self.entries()
            .get(&(selection.to_owned(), source.protocol_path().clone()))
            .filter(|cached| {
                cached.observation == observation
                    && cached.volume_identity == source.volume_identity()
            })
            .map(|cached| cached.digest)
    }

    pub fn remember(
        &self,
        selection: &str,
        path: &SourcePath,
        observation: Observation,
        volume_identity: &str,
        digest: Blake3Digest,
    ) {
        self.entries().insert(
            (selection.to_owned(), path.clone()),
            CachedFile {
                observation,
                volume_identity: volume_identity.to_owned(),
                digest,
            },
        );
    }

    pub fn clear(&self) {
        self.entries().clear();
    }

    pub fn forget(&self, selection: &str) {
        self.entries().retain(|(id, _), _| id != selection);
    }

    pub fn selections(&self, previous: &[SavedSelection], latest: &[SavedSelection]) {
        self.entries().retain(|(id, _), _| {
            latest.iter().any(|selection| {
                selection.id == *id && selection.active && previous.contains(selection)
            })
        });
    }

    pub fn scanned(&self, selection: &str, entries: &[ScannedEntry]) {
        let paths: HashSet<_> = entries.iter().map(|entry| &entry.source_path).collect();
        self.entries()
            .retain(|(id, path), _| id != selection || paths.contains(path));
    }
}

#[cfg(test)]
mod tests;
