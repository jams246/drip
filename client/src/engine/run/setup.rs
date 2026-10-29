use std::path::PathBuf;

use crate::{
    engine::{
        Engine, EngineError, EventKind, pass::PassContext, saved, selection::ActiveSelection,
        watcher::SourceWatcher,
    },
    settings::{ClientSettings, SavedSelection},
    source::{resolve_root, same_path},
};

#[derive(Default)]
pub(super) struct PassState {
    pub watcher: Option<SourceWatcher>,
    seen: Vec<SavedSelection>,
}

impl PassState {
    pub async fn prepare(
        &mut self,
        engine: &Engine,
        context: &PassContext<'_>,
        barrier: bool,
    ) -> Result<(Vec<ActiveSelection>, ClientSettings), EngineError> {
        if context.signal.rebuild() {
            self.watcher = None;
        }
        let local = engine.settings.load()?;
        for saved in local.selections.iter().filter(|saved| saved.active) {
            if let Ok(Some(current)) = resolve_root(saved) {
                if same_path(&current.source, &saved.source) {
                    engine.settings.refresh_root(&saved.id, &current)?;
                } else if let Err(error) = engine.settings.follow_root(saved, &current) {
                    context
                        .events
                        .emit(
                            EventKind::Error,
                            Some(&saved.id),
                            Some(&saved.source),
                            "root-move",
                            error.to_string(),
                        )
                        .await?;
                }
            }
        }
        let active = saved::synchronize(
            &engine.settings,
            context.transport,
            context.pairing,
            barrier,
        )
        .await?;
        let latest = engine.settings.load()?;
        if let Some(warning) = context
            .cache
            .reconcile(context.pairing, &latest.selections)
            .await
        {
            let _ = context
                .events
                .emit(EventKind::Error, None, None, "local-cache", warning)
                .await;
        }
        for selection in &latest.selections {
            if !self.seen.contains(selection) {
                if selection.active {
                    context.signal.retry(&selection.id);
                }
                context
                    .events
                    .emit(
                        EventKind::SelectionChanged,
                        Some(&selection.id),
                        Some(&selection.source),
                        &crate::source::random_id()?,
                        if selection.active {
                            "active selection acknowledged"
                        } else {
                            "selection removed"
                        },
                    )
                    .await?;
            }
        }
        self.configure(context, &latest.selections).await?;
        self.seen = latest.selections.clone();
        Ok((active, latest))
    }

    async fn configure(
        &mut self,
        context: &PassContext<'_>,
        selections: &[SavedSelection],
    ) -> Result<(), EngineError> {
        let selections: Vec<_> = selections
            .iter()
            .filter(|selection| selection.active)
            .cloned()
            .collect();
        if self
            .watcher
            .as_ref()
            .is_some_and(|watcher| watcher.current(&selections))
            && !context.signal.rebuild()
        {
            return Ok(());
        }
        let mut watcher = SourceWatcher::new(
            context.signal.clone(),
            context.store.directory().to_owned(),
            selections.clone(),
        )
        .map_err(std::io::Error::other)?;
        for selection in selections {
            if let Err(error) = watcher.watch(
                &PathBuf::from(selection.source.to_windows_path()),
                selection.root_kind == drip_core::protocol::EntryKind::Directory,
            ) {
                watcher.failed.insert(selection.id.clone());
                context.events.emit(EventKind::Error, Some(&selection.id), Some(&selection.source), "watcher",
                    format!("source watcher unavailable; full reconciliation remains active: {error}")).await?;
            }
        }
        watcher
            .watch_settings(context.store.directory())
            .map_err(std::io::Error::other)?;
        self.watcher = Some(watcher);
        Ok(())
    }
}
