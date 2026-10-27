use drip_core::protocol::{Entry, EntryKind, UploadCommitResponse, UploadState};

use crate::{
    Store, StoreError, StoreResult, entry_files, entry_files::EntrySnapshot, upload_exchange,
    upload_files, upload_metadata, upload_paths, upload_store::authenticated_device,
};

use crate::upload_delta::lock;

use super::{delta, session::mark_committed};

impl Store {
    /// Rechecks staged bytes and publishes under the shared SQLite mutation barrier.
    pub fn commit_upload(
        &self,
        credential: &str,
        upload_id: &str,
    ) -> StoreResult<UploadCommitResponse> {
        let lookup_time = self.now()?;
        self.reap_uploads(lookup_time)?;
        let device = authenticated_device(self, credential)?;
        let session = self.uploads.get(upload_id, &device.pc_id, lookup_time)?;
        let (
            stage_name,
            source_path,
            selection_id,
            generation,
            expected_size,
            digest,
            last_write,
            expected_entry,
            delta_mode,
        ) = {
            let mut session_state = lock(&session);
            if session_state.state != UploadState::Active
                || session_state.receiving
                || session_state.committing
                || session_state.planning
                || (!session_state.delta_mode && !session_state.has_data)
            {
                return Err(StoreError::Conflict);
            }
            session_state.committing = true;
            (
                session_state.stage_name.clone(),
                session_state.source_path.clone(),
                session_state.selection_id.clone(),
                session_state.generation,
                session_state.expected_size,
                session_state.digest,
                session_state.last_write,
                session_state.expected_entry,
                session_state.delta_mode,
            )
        };
        if let Err(error) = delta::check_before_commit(self, upload_id, expected_size, delta_mode) {
            lock(&session).committing = false;
            return Err(error);
        }
        let staging = match self.uploads.staging_dir() {
            Ok(staging) => staging,
            Err(error) => {
                lock(&session).committing = false;
                return Err(error);
            }
        };
        let staged_snapshot = match upload_files::verify_stage(
            &staging,
            &stage_name,
            expected_size,
            digest,
            last_write,
        ) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                let mut session = lock(&session);
                if session.state == UploadState::Active {
                    session.state = UploadState::Aborted;
                    session.committing = false;
                    session.has_data = false;
                }
                drop(session);
                let _ = self.uploads.remove_stage(&stage_name);
                let _ = crate::upload_delta::delete_plan(self, upload_id);
                return Err(error);
            }
        };
        let base_snapshot = match self
            .open_device_directory(&device)
            .and_then(|directory| entry_files::read_entry(&directory, &source_path))
        {
            Ok(snapshot) => snapshot,
            Err(error) => {
                lock(&session).committing = false;
                return Err(error);
            }
        };
        let now = match self.now() {
            Ok(now) => now,
            Err(error) => {
                lock(&session).committing = false;
                return Err(error);
            }
        };
        upload_files::checkpoint("before_writer_lock");
        let mut retired_kind = None;
        let result = self.with_authorized_selection_mutation(
            credential,
            &selection_id,
            generation,
            |transaction, current_device, selection, directory| {
                if !crate::selections::selection_contains_path(selection, &source_path) {
                    return Err(StoreError::DisallowedNamespace);
                }
                let mut session = lock(&session);
                if session.state != UploadState::Active
                    || !session.committing
                    || session.pc_id != current_device.pc_id
                    || session.base_snapshot != base_snapshot
                    || session.expected_entry != expected_entry
                {
                    return Err(StoreError::Conflict);
                }
                delta::require_complete(transaction, &session)?;
                let current_time = self.now()?;
                if current_time.saturating_sub(session.last_progress) >= 30 * 60 {
                    self.abort_conflicted(&mut session, current_time)?;
                    drop(session);
                    upload_files::checkpoint("expired_session_before_remove");
                    let _ = self.uploads.remove(upload_id);
                    return Err(StoreError::NotFound);
                }
                if entry_files::check_entry(directory, &source_path, base_snapshot.as_ref())
                    .is_err()
                {
                    self.abort_conflicted(&mut session, now)?;
                    return Err(StoreError::Conflict);
                }
                let current_revision = upload_metadata::record_entry_snapshot(
                    transaction,
                    &current_device.pc_id,
                    &source_path,
                    base_snapshot.as_ref(),
                )?;
                if current_revision != session.base_revision {
                    self.abort_conflicted(&mut session, now)?;
                    return Err(StoreError::Conflict);
                }
                if expected_entry.is_some_and(|expected| {
                    base_snapshot.as_ref().is_none_or(|snapshot| {
                        snapshot.kind() != expected.kind
                            || current_revision != Some(expected.revision)
                    })
                }) {
                    self.abort_conflicted(&mut session, now)?;
                    return Err(StoreError::Conflict);
                }
                let is_directory_exchange =
                    matches!(base_snapshot.as_ref(), Some(EntrySnapshot::Directory(_)));
                let (parent, filename) = if is_directory_exchange {
                    upload_paths::open_parent(directory, &source_path, false)?
                        .ok_or(StoreError::Conflict)?
                } else {
                    upload_paths::create_parent(directory, &source_path)?
                };
                if upload_files::check_stage_snapshot(&staging, &stage_name, &staged_snapshot)
                    .is_err()
                {
                    return Err(StoreError::Conflict);
                }
                let publication = if let Some(EntrySnapshot::Directory(expected_directory)) =
                    base_snapshot.as_ref()
                {
                    let paths = upload_exchange::ExchangePaths {
                        staging: &staging,
                        stage_name: &stage_name,
                        parent: &parent,
                        filename: &filename,
                    };
                    let target = upload_exchange::SelectedEntry {
                        root: directory,
                        source: &source_path,
                    };
                    upload_exchange::replace_directory_with_file(
                        &paths,
                        &target,
                        *expected_directory,
                        &staged_snapshot,
                    )
                } else {
                    upload_files::publish(&staging, &stage_name, &parent, &filename)
                };
                if let Err(failure) = publication {
                    if failure.renamed {
                        mark_committed(&mut session, now);
                        if is_directory_exchange {
                            retired_kind = Some(EntryKind::Directory);
                        }
                    } else {
                        session.committing = false;
                    }
                    return Err(failure.error);
                }
                if is_directory_exchange {
                    retired_kind = Some(EntryKind::Directory);
                }
                mark_committed(&mut session, now);
                let published_snapshot = upload_files::snapshot_published_file(
                    directory,
                    &source_path,
                    &staged_snapshot,
                )?;
                let revision = upload_metadata::record_snapshot(
                    transaction,
                    &current_device.pc_id,
                    &source_path,
                    Some(&published_snapshot),
                )?
                .ok_or(StoreError::Conflict)?;
                delta::record_manifest(transaction, &session, revision)?;
                Ok(UploadCommitResponse {
                    entry: Entry {
                        source_path: source_path.clone(),
                        kind: EntryKind::File,
                        size: published_snapshot.size,
                        digest: Some(published_snapshot.digest),
                        revision,
                        last_write: Some(published_snapshot.last_write),
                    },
                })
            },
        );
        if result.is_err() {
            let mut session = lock(&session);
            if session.state == UploadState::Active {
                session.committing = false;
            }
        }
        if lock(&session).state == UploadState::Aborted {
            let _ = crate::upload_delta::delete_plan(self, upload_id);
        }
        if let Some(kind) = retired_kind {
            upload_files::checkpoint("before_retired_cleanup");
            let cleanup = self.uploads.remove_retired_stage(&stage_name, kind);
            if result.is_ok() {
                cleanup?;
            }
        }
        result
    }
}
