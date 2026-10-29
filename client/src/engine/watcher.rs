mod routing;
mod signal;

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

use crate::settings::SavedSelection;
pub(super) use signal::{ChangeEpoch, DirtySignal};

pub(super) struct SourceWatcher {
    watcher: RecommendedWatcher,
    watched: Vec<(PathBuf, RecursiveMode)>,
    selections: Vec<SavedSelection>,
    pub failed: HashSet<String>,
}

impl SourceWatcher {
    pub fn new(
        signal: DirtySignal,
        app_directory: PathBuf,
        selections: Vec<SavedSelection>,
    ) -> notify::Result<Self> {
        let roots = selections.clone();
        let watcher = notify::recommended_watcher(move |result| {
            routing::route(&signal, &roots, &app_directory, result);
        })?;
        Ok(Self {
            watcher,
            watched: Vec::new(),
            selections,
            failed: HashSet::new(),
        })
    }

    pub fn current(&self, selections: &[SavedSelection]) -> bool {
        self.selections == selections && self.failed.is_empty()
    }

    pub fn watch(&mut self, path: &Path, directory: bool) -> notify::Result<()> {
        if let Some(parent) = path.parent() {
            self.register(parent, RecursiveMode::NonRecursive)?;
        }
        if directory {
            self.register(path, RecursiveMode::Recursive)?;
        }
        Ok(())
    }

    pub fn watch_settings(&mut self, directory: &Path) -> notify::Result<()> {
        self.register(directory, RecursiveMode::NonRecursive)
    }

    fn register(&mut self, path: &Path, mode: RecursiveMode) -> notify::Result<()> {
        if let Some((_, current)) = self.watched.iter_mut().find(|(watched, _)| watched == path) {
            if *current == RecursiveMode::Recursive || *current == mode {
                return Ok(());
            }
            self.watcher.unwatch(path)?;
            self.watcher.watch(path, mode)?;
            *current = mode;
        } else {
            self.watcher.watch(path, mode)?;
            self.watched.push((path.to_owned(), mode));
        }
        Ok(())
    }
}

impl Drop for SourceWatcher {
    fn drop(&mut self) {
        for (path, _) in &self.watched {
            let _ = self.watcher.unwatch(path);
        }
    }
}

#[cfg(test)]
mod tests;
