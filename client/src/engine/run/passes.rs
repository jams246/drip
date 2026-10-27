use std::path::PathBuf;

use crate::engine::{
    Engine, EngineError, EventKind,
    pass::{PassContext, auth_failure},
    saved,
    watcher::SourceWatcher,
};
use crate::source::{capture_root, same_path};

impl Engine {
    pub(super) async fn continuous_pass(
        &self,
        context: &PassContext<'_>,
        watcher: &mut Option<SourceWatcher>,
        seen_selections: &mut Vec<crate::settings::SavedSelection>,
        barrier: bool,
    ) -> Result<bool, EngineError> {
        let transport = context.transport;
        let pairing = context.pairing;
        let events = context.events;
        let signal = context.signal;
        let local = self.settings.load()?;
        for saved in local.selections.iter().filter(|saved| saved.active) {
            if let Ok(current) = capture_root(&PathBuf::from(saved.source.to_windows_path()))
                && same_path(&current.source, &saved.source)
                && current.volume_identity == saved.volume_identity
                && (current.kind != saved.root_kind || current.source != saved.source)
            {
                self.settings.refresh_root(&saved.id, &current)?;
            }
        }
        let active = saved::synchronize(&self.settings, transport, pairing, barrier).await?;
        events
            .emit(
                EventKind::Connected,
                None,
                None,
                "connection",
                "server selection state confirmed",
            )
            .await?;
        let latest = self.settings.load()?;
        for selection in &latest.selections {
            if !seen_selections.iter().any(|previous| previous == selection) {
                events
                    .emit(
                        EventKind::SelectionChanged,
                        Some(&selection.id),
                        Some(&selection.source),
                        &crate::source::random_id()?,
                        if selection.active {
                            "active selection acknowledged"
                        } else {
                            "selection removed; mirror retained"
                        },
                    )
                    .await?;
            }
        }
        *seen_selections = latest.selections.clone();
        let mut next_watcher =
            SourceWatcher::new(signal.clone(), self.settings.directory().to_owned())
                .map_err(std::io::Error::other)?;
        for selection in latest
            .selections
            .iter()
            .filter(|selection| selection.active)
        {
            if let Err(error) =
                next_watcher.watch(&PathBuf::from(selection.source.to_windows_path()))
            {
                events.emit(EventKind::Error, Some(&selection.id), Some(&selection.source), "watcher",
                    format!("source watcher unavailable; full reconciliation remains active: {error}")).await?;
            }
        }
        next_watcher
            .watch_settings(self.settings.directory())
            .map_err(std::io::Error::other)?;
        *watcher = Some(next_watcher);
        signal.take();
        let mut successful = true;
        for selection in active {
            let saved = latest
                .selections
                .iter()
                .find(|saved| saved.id == selection.selection.id)
                .ok_or(EngineError::SelectionChanged)?;
            match context.root(&selection, saved).await {
                Ok(complete) => successful &= complete,
                Err(error)
                    if matches!(
                        error,
                        EngineError::Cancelled | EngineError::SelectionChanged
                    ) || auth_failure(&error) =>
                {
                    return Err(error);
                }
                Err(error) => {
                    successful = false;
                    let kind = if matches!(error, EngineError::Source(_)) {
                        EventKind::UnavailableRoot
                    } else {
                        EventKind::Error
                    };
                    events
                        .emit(
                            kind,
                            Some(&saved.id),
                            Some(&saved.source),
                            "scan",
                            error.to_string(),
                        )
                        .await?;
                }
            }
        }
        Ok(successful)
    }
}
