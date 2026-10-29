mod deleted_root;
mod deletion;
mod entries;
mod file;
mod upload_events;
mod validation;

use std::collections::HashMap;

use drip_core::{SourcePath, protocol::EntryKind};
use tokio::sync::watch;

use crate::{
    credentials::Pairing,
    settings::{SavedSelection, SettingsStore},
    source::{ScannedEntry, scan_root},
    transport::HttpTransport,
};

use super::{
    EngineError, EventKind, EventSink, mirror_api, run::RunControl, selection::ActiveSelection,
    watcher::DirtySignal,
};

pub(super) struct PassContext<'a> {
    pub cache: &'a super::cache::FileCache,
    pub store: &'a SettingsStore,
    pub transport: &'a HttpTransport,
    pub pairing: &'a Pairing,
    pub events: &'a EventSink,
    pub signal: &'a DirtySignal,
    pub control: &'a watch::Receiver<RunControl>,
}

#[derive(Clone, Copy)]
struct PassOptions {
    complete_scan: bool,
    audit: bool,
    expected_entry: Option<drip_core::protocol::EntryPrecondition>,
}

impl PassContext<'_> {
    pub async fn root(
        &self,
        active: &ActiveSelection,
        saved: &SavedSelection,
        operation: &str,
        audit: bool,
    ) -> Result<bool, EngineError> {
        let epoch = self.signal.epoch(&saved.id);
        self.emit(
            EventKind::Scanning,
            active,
            &saved.source,
            operation,
            "scanning source and actual mirror",
        )
        .await?;
        self.check_selection(active, saved)?;
        if crate::source::confirm_root_absent(saved)? {
            return self.delete_root(active, saved, operation, epoch).await;
        }
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
                    PassOptions {
                        complete_scan: scan.complete,
                        audit,
                        expected_entry: None,
                    },
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
                        entry_error_kind(&error),
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
        if successful && !self.signal.changed_since(&saved.id, epoch) {
            self.check(active, saved)?;
            let after = scan_root(saved, self.store.directory())?;
            if after.complete && fingerprint(&after.entries) == fingerprint(&scan.entries) {
                self.delete_extras(active, saved, Some(&scan), epoch)
                    .await?;
                if !self.signal.changed_since(&saved.id, epoch) {
                    if let Some(warning) = self
                        .cache
                        .prune(&saved.id, &scan.entries, self.signal.clone(), epoch)
                        .await
                    {
                        let _ = self
                            .events
                            .emit(EventKind::Error, None, None, "local-cache", warning)
                            .await;
                    }
                } else {
                    successful = false;
                }
            } else {
                successful = false;
            }
        } else {
            successful = false;
        }
        self.check(active, saved)?;
        if successful && !self.signal.changed_since(&saved.id, epoch) {
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
    error.registration_required()
}

fn entry_error_kind(error: &EngineError) -> EventKind {
    match error {
        EngineError::Cancelled | EngineError::SelectionChanged => EventKind::Cancelled,
        _ => EventKind::Error,
    }
}

#[cfg(test)]
mod tests;
