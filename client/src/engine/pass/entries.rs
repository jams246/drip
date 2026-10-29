use drip_core::protocol::{
    DirectoryCreateRequest, DirectoryCreateResponse, Entry, EntryKind, EntryPrecondition,
};
use reqwest::Method;

use crate::{settings::SavedSelection, source::ScannedEntry};

use super::{PassContext, PassOptions};
use crate::engine::{EngineError, EventKind, api, selection::ActiveSelection};

impl PassContext<'_> {
    pub(super) async fn entry(
        &self,
        active: &ActiveSelection,
        saved: &SavedSelection,
        entry: &ScannedEntry,
        remote: Option<&Entry>,
        options: PassOptions,
        operation: &str,
    ) -> Result<(), EngineError> {
        self.emit(
            EventKind::SourceChanged,
            active,
            &entry.source_path,
            operation,
            "checking source version",
        )
        .await?;
        self.emit(
            EventKind::Queued,
            active,
            &entry.source_path,
            operation,
            "source queued for reconciliation",
        )
        .await?;
        let conflict = remote.filter(|remote| remote.kind != entry.kind);
        if conflict.is_some() && !options.complete_scan {
            return Err(EngineError::InvalidResponse(
                "incomplete source scan cannot authorize a type exchange",
            ));
        }
        self.fresh_kind(saved, entry)?;
        let expected_entry = conflict.map(|entry| EntryPrecondition {
            kind: entry.kind,
            revision: entry.revision,
        });
        let options = PassOptions {
            expected_entry,
            ..options
        };
        if entry.kind == EntryKind::Directory {
            if remote.is_some_and(|remote| remote.kind == EntryKind::Directory) {
                return Ok(());
            }
            self.check(active, saved)?;
            self.fresh_kind(saved, entry)?;
            let request = DirectoryCreateRequest {
                selection_id: active.selection.id.clone(),
                generation: active.generation,
                source_path: entry.source_path.clone(),
                expected_entry,
            };
            let response: DirectoryCreateResponse = api::json(
                self.transport,
                Method::POST,
                "v1/directories",
                Some(self.pairing.credential()),
                &[],
                Some(&request),
            )
            .await?;
            if response.entry.source_path != entry.source_path
                || response.entry.kind != EntryKind::Directory
            {
                return Err(EngineError::DestinationMismatch);
            }
            self.emit(
                EventKind::DirectoryCreated,
                active,
                &entry.source_path,
                operation,
                "directory published",
            )
            .await?;
            return Ok(());
        }
        self.file(active, saved, entry, remote, options, operation)
            .await
    }
}
