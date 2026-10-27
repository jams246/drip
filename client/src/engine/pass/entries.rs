use drip_core::protocol::{
    DirectoryCreateRequest, DirectoryCreateResponse, Entry, EntryKind, EntryPrecondition,
};
use reqwest::Method;

use crate::{
    settings::SavedSelection,
    source::{ScannedEntry, SourceFile},
};

use super::{PassContext, upload_events};
use crate::engine::{EngineError, EventKind, api, selection::ActiveSelection, upload};

impl PassContext<'_> {
    pub(super) async fn entry(
        &self,
        active: &ActiveSelection,
        saved: &SavedSelection,
        entry: &ScannedEntry,
        remote: Option<&Entry>,
        complete_scan: bool,
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
        if conflict.is_some() && !complete_scan {
            return Err(EngineError::InvalidResponse(
                "incomplete source scan cannot authorize a type exchange",
            ));
        }
        self.fresh_kind(saved, entry)?;
        let expected_entry = conflict.map(|entry| EntryPrecondition {
            kind: entry.kind,
            revision: entry.revision,
        });
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
        let source = SourceFile::open(&entry.local_path)?;
        if source.protocol_path() != &entry.source_path
            || source.volume_identity() != saved.volume_identity
        {
            return Err(crate::source::SourceError::Replaced.into());
        }
        let (sender, report) = upload_events::start(
            self.events.clone(),
            active.selection.id.clone(),
            operation.to_owned(),
        );
        let _ = sender
            .send(crate::engine::ActivityEvent::Progress(
                crate::engine::ProgressSnapshot {
                    phase: crate::engine::UploadPhase::Hashing,
                    source_path: entry.source_path.clone(),
                    processed_bytes: 0,
                    total_bytes: Some(source.size()),
                    source_hashed_bytes: 0,
                    source_chunked_bytes: 0,
                    chunk_count: 0,
                    planned_payload_bytes: 0,
                    file_payload_sent: 0,
                },
            ))
            .await;
        let digest = source.hash(&sender).await;
        let result = match digest {
            Ok(digest)
                if remote.is_some_and(|remote| {
                    remote.kind == EntryKind::File
                        && remote.size == source.size()
                        && remote.digest == Some(digest)
                        && remote.last_write == Some(source.last_write())
                }) =>
            {
                Ok(())
            }
            Ok(digest) => {
                self.check(active, saved)?;
                self.fresh_kind(saved, entry)?;
                upload::upload_selected(
                    self.transport,
                    self.pairing,
                    source,
                    active,
                    expected_entry,
                    Some(digest),
                    &sender,
                )
                .await
                .map(|_| ())
            }
            Err(error) => Err(error.into()),
        };
        drop(sender);
        report
            .await
            .map_err(|error| std::io::Error::other(error.to_string()))??;
        result
    }
}
