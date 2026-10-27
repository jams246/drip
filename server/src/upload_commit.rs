use std::sync::{Mutex, MutexGuard};

use drip_core::protocol::{Entry, EntryKind, UploadCommitResponse, UploadState};

use crate::{
    Store, StoreError, StoreResult, upload_files, upload_metadata, upload_paths,
    upload_store::authenticated_device,
};

impl Store {
    /// Rechecks staged bytes and publishes under the shared SQLite mutation barrier.
    pub fn commit_upload(
        &self,
        credential: &str,
        upload_id: &str,
    ) -> StoreResult<UploadCommitResponse> {
        let device = authenticated_device(self, credential)?;
        let session = self.uploads.get(upload_id, &device.pc_id, self.now()?)?;
        let (stage_name, source_path, selection_id, generation, expected_size, digest, last_write) = {
            let mut session_state = lock(&session);
            if session_state.state != UploadState::Active
                || session_state.receiving
                || session_state.committing
                || !session_state.has_data
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
            )
        };
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
                return Err(error);
            }
        };
        let base_snapshot = match self
            .open_device_directory(&device)
            .and_then(|directory| upload_files::read_snapshot(&directory, &source_path))
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
        let result = self.with_authorized_selection_mutation(
            credential,
            &selection_id,
            generation,
            |transaction, current_device, selection, directory| {
                if selection.source != source_path {
                    return Err(StoreError::DisallowedNamespace);
                }
                let mut session = lock(&session);
                if session.state != UploadState::Active
                    || !session.committing
                    || session.pc_id != current_device.pc_id
                    || session.base_snapshot != base_snapshot
                {
                    return Err(StoreError::Conflict);
                }
                let current_time = self.now()?;
                if current_time.saturating_sub(session.last_progress) >= 30 * 60 {
                    self.abort_conflicted(&mut session, current_time)?;
                    drop(session);
                    upload_files::checkpoint("expired_session_before_remove");
                    let _ = self.uploads.remove(upload_id);
                    return Err(StoreError::NotFound);
                }
                if upload_files::check_snapshot(directory, &source_path, base_snapshot.as_ref())
                    .is_err()
                {
                    self.abort_conflicted(&mut session, now)?;
                    return Err(StoreError::Conflict);
                }
                let current_revision = upload_metadata::record_snapshot(
                    transaction,
                    &current_device.pc_id,
                    &source_path,
                    base_snapshot.as_ref(),
                )?;
                if current_revision != session.base_revision {
                    self.abort_conflicted(&mut session, now)?;
                    return Err(StoreError::Conflict);
                }
                let (parent, filename) = upload_paths::create_parent(directory, &source_path)?;
                if upload_files::check_stage_snapshot(&staging, &stage_name, &staged_snapshot)
                    .is_err()
                {
                    return Err(StoreError::Conflict);
                }
                if let Err(failure) =
                    upload_files::publish(&staging, &stage_name, &parent, &filename)
                {
                    if failure.renamed {
                        mark_committed(&mut session, now);
                    } else {
                        session.committing = false;
                    }
                    return Err(failure.error);
                }
                mark_committed(&mut session, now);
                let revision = upload_metadata::record_snapshot(
                    transaction,
                    &current_device.pc_id,
                    &source_path,
                    Some(&staged_snapshot),
                )?
                .ok_or(StoreError::Conflict)?;
                Ok(UploadCommitResponse {
                    entry: Entry {
                        source_path: source_path.clone(),
                        kind: EntryKind::File,
                        size: staged_snapshot.size,
                        digest: Some(staged_snapshot.digest),
                        revision,
                        last_write: Some(staged_snapshot.last_write),
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
        result
    }

    fn abort_conflicted(
        &self,
        session: &mut crate::upload_sessions::UploadSession,
        now: i64,
    ) -> StoreResult<()> {
        self.uploads.remove_stage(&session.stage_name)?;
        session.state = UploadState::Aborted;
        session.committing = false;
        session.has_data = false;
        session.last_progress = now;
        Ok(())
    }
}

fn mark_committed(session: &mut crate::upload_sessions::UploadSession, now: i64) {
    session.state = UploadState::Committed;
    session.receiving = false;
    session.committing = false;
    session.has_data = false;
    session.last_progress = now;
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
