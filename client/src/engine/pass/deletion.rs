use std::collections::HashSet;

use drip_core::protocol::{EntryDeleteRequest, EntryDeleteResponse, EntryKind, EntryPrecondition};
use reqwest::Method;

use crate::{
    settings::SavedSelection,
    source::{SourceScan, confirm_absent},
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
        scan: &SourceScan,
        epoch: crate::engine::watcher::ChangeEpoch,
    ) -> Result<(), EngineError> {
        let source_paths: HashSet<_> = scan
            .entries
            .iter()
            .map(|entry| &entry.source_path)
            .collect();
        let mut remote =
            mirror_api::inventory(self.transport, self.pairing.credential(), active).await?;
        remote.sort_by_key(|entry| std::cmp::Reverse(entry.source_path.components().len()));
        for mut entry in remote {
            if source_paths.contains(&entry.source_path)
                || scan
                    .protected_paths
                    .iter()
                    .any(|protected| mirror_api::contains(protected, &entry.source_path))
            {
                continue;
            }
            self.check(active, saved)?;
            if self.signal.changed_since(&saved.id, epoch) {
                return Err(EngineError::SelectionChanged);
            }
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
            if !confirm_absent(saved, &entry.source_path, self.store.directory())? {
                return Err(EngineError::SelectionChanged);
            }
            self.check(active, saved)?;
            if self.signal.changed_since(&saved.id, epoch) {
                return Err(EngineError::SelectionChanged);
            }
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
}
