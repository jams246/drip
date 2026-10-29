use std::collections::HashSet;

use drip_core::protocol::{EntryDeleteRequest, EntryDeleteResponse, EntryKind, EntryPrecondition};
use reqwest::Method;

use crate::{
    settings::SavedSelection,
    source::{SourceScan, confirm_absent, confirm_root_absent},
};

use super::PassContext;
use crate::engine::{
    EngineError, EventKind, api, mirror_api,
    selection::{self, ActiveSelection},
};

impl PassContext<'_> {
    pub(super) async fn delete_extras(
        &self,
        active: &ActiveSelection,
        saved: &SavedSelection,
        scan: Option<&SourceScan>,
        epoch: crate::engine::watcher::ChangeEpoch,
    ) -> Result<(), EngineError> {
        let source_paths: HashSet<_> = scan
            .into_iter()
            .flat_map(|scan| &scan.entries)
            .map(|entry| &entry.source_path)
            .collect();
        let mut remote =
            mirror_api::inventory(self.transport, self.pairing.credential(), active).await?;
        remote.sort_by_key(|entry| std::cmp::Reverse(entry.source_path.components().len()));
        for mut entry in remote {
            if source_paths.contains(&entry.source_path)
                || scan.is_some_and(|scan| {
                    scan.protected_paths
                        .iter()
                        .any(|protected| mirror_api::contains(protected, &entry.source_path))
                })
            {
                continue;
            }
            self.check_deletion(active, saved, &entry.source_path, epoch, scan.is_none())?;
            selection::require_current(self.transport, self.pairing.credential(), active).await?;
            if entry.kind == EntryKind::Directory {
                let fresh =
                    mirror_api::inventory(self.transport, self.pairing.credential(), active)
                        .await?;
                let Some(current) = fresh
                    .into_iter()
                    .find(|current| current.source_path == entry.source_path)
                else {
                    continue;
                };
                if current.kind != EntryKind::Directory {
                    return Err(EngineError::DestinationMismatch);
                }
                // Child deletions change directory metadata, so the old revision cannot authorize removal.
                entry.revision = current.revision;
            }
            self.check_deletion(active, saved, &entry.source_path, epoch, scan.is_none())?;
            let operation = crate::source::random_id()?;
            let request = EntryDeleteRequest {
                selection_id: active.selection.id.clone(),
                generation: active.generation,
                source_path: entry.source_path.clone(),
                expected_entry: EntryPrecondition {
                    kind: entry.kind,
                    revision: entry.revision,
                },
            };
            let _: EntryDeleteResponse = api::json(
                self.transport,
                Method::POST,
                "v1/entries/delete",
                Some(self.pairing.credential()),
                &[],
                Some(&request),
            )
            .await?;
            self.emit(
                EventKind::Deleted,
                active,
                &entry.source_path,
                &operation,
                "confirmed absent source entry deleted",
            )
            .await?;
        }
        Ok(())
    }

    pub(super) fn check_deletion(
        &self,
        active: &ActiveSelection,
        saved: &SavedSelection,
        target: &drip_core::SourcePath,
        epoch: crate::engine::watcher::ChangeEpoch,
        missing_root: bool,
    ) -> Result<(), EngineError> {
        self.check_selection(active, saved)?;
        let absent = if missing_root {
            confirm_root_absent(saved)?
        } else {
            self.check(active, saved)?;
            confirm_absent(saved, target, self.store.directory())?
        };
        if !absent || self.signal.changed_since(&saved.id, epoch) {
            return Err(EngineError::SelectionChanged);
        }
        Ok(())
    }
}
