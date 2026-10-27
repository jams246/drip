mod deletion;
mod entries;
mod upload_events;

use std::collections::HashMap;

use drip_core::{SourcePath, protocol::EntryKind};
use tokio::sync::watch;

use crate::{
    credentials::Pairing,
    settings::{SavedSelection, SettingsStore},
    source::{ScannedEntry, capture_root, scan_root, validate_root},
    transport::HttpTransport,
};

use super::{
    EngineError, EventKind, EventSink, mirror_api, run::RunControl, selection::ActiveSelection,
    watcher::DirtySignal,
};

pub(super) struct PassContext<'a> {
    pub store: &'a SettingsStore,
    pub transport: &'a HttpTransport,
    pub pairing: &'a Pairing,
    pub events: &'a EventSink,
    pub signal: &'a DirtySignal,
    pub control: &'a watch::Receiver<RunControl>,
}

impl PassContext<'_> {
    pub async fn root(
        &self,
        active: &ActiveSelection,
        saved: &SavedSelection,
        operation: &str,
    ) -> Result<bool, EngineError> {
        self.emit(
            EventKind::Scanning,
            active,
            &saved.source,
            operation,
            "scanning source and actual mirror",
        )
        .await?;
        self.check(active, saved)?;
        let scan = scan_root(saved, self.store.directory())?;
        let remote =
            mirror_api::inventory(self.transport, self.pairing.credential(), active).await?;
        let remote: HashMap<_, _> = remote
            .into_iter()
            .map(|entry| (entry.source_path.clone(), entry))
            .collect();
        let mut successful = scan.complete;
        for issue in &scan.issues {
            self.emit(
                EventKind::Error,
                active,
                issue.source_path.as_ref().unwrap_or(&saved.source),
                operation,
                &issue.message,
            )
            .await?;
        }
        let mut entries: Vec<_> = scan.entries.iter().collect();
        entries.sort_by_key(|entry| {
            (
                entry.source_path.components().len(),
                entry.kind == EntryKind::File,
            )
        });
        for entry in entries {
            self.check(active, saved)?;
            let entry_operation = crate::source::random_id()?;
            if let Err(error) = self
                .entry(
                    active,
                    saved,
                    entry,
                    remote.get(&entry.source_path),
                    scan.complete,
                    &entry_operation,
                )
                .await
            {
                self.events.reachability().observe_error(&error);
                if matches!(
                    error,
                    EngineError::Cancelled | EngineError::SelectionChanged
                ) || auth_failure(&error)
                {
                    self.emit(
                        EventKind::Error,
                        active,
                        &entry.source_path,
                        &entry_operation,
                        error.to_string(),
                    )
                    .await?;
                    return Err(error);
                }
                successful = false;
                self.emit(
                    EventKind::Error,
                    active,
                    &entry.source_path,
                    &entry_operation,
                    error.to_string(),
                )
                .await?;
                self.emit(
                    EventKind::Retry,
                    active,
                    &entry.source_path,
                    &entry_operation,
                    "source entry retained; retry scheduled",
                )
                .await?;
            }
        }
        if successful && !self.signal.is_dirty() {
            self.check(active, saved)?;
            let after = scan_root(saved, self.store.directory())?;
            if after.complete && fingerprint(&after.entries) == fingerprint(&scan.entries) {
                self.delete_extras(active, saved, &scan).await?;
            } else {
                successful = false;
            }
        } else {
            successful = false;
        }
        self.check(active, saved)?;
        if successful && !self.signal.is_dirty() {
            self.emit(
                EventKind::Synced,
                active,
                &saved.source,
                operation,
                "source and actual mirror agree",
            )
            .await?;
            Ok(true)
        } else {
            self.emit(
                EventKind::Retry,
                active,
                &saved.source,
                operation,
                "root remains unsynced; source uncertainty or activity requires another pass",
            )
            .await?;
            Ok(false)
        }
    }

    pub fn check(
        &self,
        active: &ActiveSelection,
        saved: &SavedSelection,
    ) -> Result<(), EngineError> {
        let control = self.control.borrow();
        if control.paused || control.stop {
            return Err(EngineError::Cancelled);
        }
        drop(control);
        let settings = self.store.load()?;
        if settings.pending_update.is_some()
            || settings.acknowledged_generation != active.generation
            || !settings
                .selections
                .iter()
                .any(|item| item == saved && item.active)
        {
            return Err(EngineError::SelectionChanged);
        }
        validate_root(saved)?;
        Ok(())
    }

    pub fn fresh_kind(
        &self,
        saved: &SavedSelection,
        entry: &ScannedEntry,
    ) -> Result<(), EngineError> {
        let current = capture_root(&entry.local_path)?;
        if current.source != entry.source_path
            || current.kind != entry.kind
            || current.volume_identity != saved.volume_identity
        {
            return Err(crate::source::SourceError::Changed.into());
        }
        Ok(())
    }

    pub async fn emit(
        &self,
        kind: EventKind,
        active: &ActiveSelection,
        path: &SourcePath,
        operation: &str,
        message: impl Into<String>,
    ) -> Result<(), EngineError> {
        self.events
            .emit(
                kind,
                Some(&active.selection.id),
                Some(path),
                operation,
                message,
            )
            .await
    }
}

fn fingerprint(
    entries: &[ScannedEntry],
) -> HashMap<SourcePath, (EntryKind, u64, Option<drip_core::Timestamp>)> {
    entries
        .iter()
        .map(|entry| {
            (
                entry.source_path.clone(),
                (entry.kind, entry.size, entry.last_write),
            )
        })
        .collect()
}

pub(super) fn auth_failure(error: &EngineError) -> bool {
    match error {
        EngineError::Api { status, .. } => *status == reqwest::StatusCode::UNAUTHORIZED,
        EngineError::AbortFailed { transfer, abort } => {
            auth_failure(transfer) || auth_failure(abort)
        }
        _ => false,
    }
}
