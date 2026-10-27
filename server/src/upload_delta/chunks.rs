use std::{
    future::Future,
    io::{Seek, SeekFrom, Write},
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::body::Body;
use drip_core::protocol::UploadState;
use http_body_util::BodyExt;

use crate::{
    Store, StoreError, StoreResult, entry_files,
    upload_data::{SESSION_TTL, expire_body, progress},
    upload_sessions::UploadSession,
    upload_store::authenticated_device,
};

use super::{lock, state};

const BODY_IDLE_TIMEOUT: Duration = Duration::from_secs(SESSION_TTL as u64);

impl Store {
    /// Accepts one declared missing chunk into its final staging range.
    pub async fn put_upload_chunk(
        &self,
        credential: &str,
        upload_id: &str,
        ordinal: u64,
        mut body: Body,
    ) -> StoreResult<()> {
        let device = authenticated_device(self, credential)?;
        let now = self.now()?;
        self.reap_uploads(now)?;
        let session = self.uploads.get(upload_id, &device.pc_id, now)?;
        let (stage_name, source, root, selection_id, generation, baseline, revision) = {
            let mut current = lock(&session);
            if current.state != UploadState::Active
                || !current.delta_mode
                || current.planning
                || current.receiving
                || current.committing
            {
                return Err(StoreError::Conflict);
            }
            current.receiving = true;
            (
                current.stage_name.clone(),
                current.source_path.clone(),
                current.selection_root.clone(),
                current.selection_id.clone(),
                current.generation,
                current.base_snapshot.clone(),
                current.base_revision,
            )
        };
        let descriptor = match state::chunk_row(self, upload_id, ordinal) {
            Ok(descriptor) if descriptor.originally_missing => descriptor,
            Ok(_) => {
                lock(&session).receiving = false;
                return Err(StoreError::Conflict);
            }
            Err(error) => {
                lock(&session).receiving = false;
                return Err(error);
            }
        };
        let outcome = receive(self, &session, &stage_name, descriptor, &mut body).await;
        if let Err(error) = outcome {
            let expired = {
                let mut current = lock(&session);
                current.receiving = false;
                current.state == UploadState::Aborted
            };
            if expired {
                let stage_result = self.uploads.remove_stage(&stage_name);
                let plan_result = state::delete_plan(self, upload_id);
                stage_result?;
                plan_result?;
            }
            return Err(error);
        }
        let now = match self.now() {
            Ok(now) => now,
            Err(error) => {
                lock(&session).receiving = false;
                return Err(error);
            }
        };
        if descriptor.accepted {
            let mut current = lock(&session);
            if current.state != UploadState::Active || !current.receiving {
                return Err(StoreError::Conflict);
            }
            current.receiving = false;
            current.last_progress = now;
            return Ok(());
        }
        let result = self.with_authorized_selection_mutation(
            credential,
            &selection_id,
            generation,
            |transaction, current_device, selection, directory| {
                if current_device.pc_id != device.pc_id
                    || selection.source != root
                    || !crate::selections::selection_contains_path(selection, &source)
                {
                    return Err(StoreError::Conflict);
                }
                let mut current = lock(&session);
                if current.state != UploadState::Active
                    || !current.receiving
                    || !current.delta_mode
                    || current.source_path != source
                    || current.selection_root != root
                    || current.base_revision != revision
                    || current.base_snapshot != baseline
                {
                    return Err(StoreError::Conflict);
                }
                entry_files::check_entry(directory, &source, baseline.as_ref())?;
                let observed_revision = crate::upload_metadata::record_entry_snapshot(
                    transaction,
                    &device.pc_id,
                    &source,
                    baseline.as_ref(),
                )?;
                if observed_revision != revision {
                    return Err(StoreError::Conflict);
                }
                state::mark_accepted(transaction, upload_id, ordinal)?;
                current.receiving = false;
                current.last_progress = now;
                Ok(())
            },
        );
        if result.is_err() {
            lock(&session).receiving = false;
        }
        result
    }
}

async fn receive(
    store: &Store,
    session: &Arc<Mutex<UploadSession>>,
    stage_name: &str,
    descriptor: state::ChunkRow,
    body: &mut Body,
) -> StoreResult<()> {
    let mut stage = if descriptor.accepted {
        None
    } else {
        let mut file = store.uploads.open_delta_stage(stage_name)?;
        file.seek(SeekFrom::Start(descriptor.offset))
            .map_err(super::io_error)?;
        Some(file)
    };
    let mut hasher = blake3::Hasher::new();
    let mut received = 0u64;
    let mut deadline = tokio::time::Instant::now() + BODY_IDLE_TIMEOUT;
    loop {
        let frame = match wait_frame(body.frame(), deadline).await {
            Ok(frame) => frame,
            Err(()) => {
                let now = store
                    .now()
                    .unwrap_or_else(|_| lock(session).last_progress.saturating_add(SESSION_TTL));
                expire_body(store, session, stage_name, now);
                return Err(StoreError::NotFound);
            }
        };
        let Some(frame) = frame else {
            break;
        };
        let frame = frame.map_err(|_| StoreError::InvalidInput)?;
        let Ok(bytes) = frame.into_data() else {
            continue;
        };
        if bytes.is_empty() {
            continue;
        }
        let next = received
            .checked_add(bytes.len() as u64)
            .ok_or(StoreError::InvalidInput)?;
        if next > u64::from(descriptor.length) {
            return Err(if descriptor.accepted {
                StoreError::Conflict
            } else {
                StoreError::InvalidInput
            });
        }
        progress(session, store.now()?)?;
        if let Some(stage) = &mut stage {
            stage.write_all(&bytes).map_err(super::io_error)?;
        }
        hasher.update(&bytes);
        received = next;
        deadline = tokio::time::Instant::now() + BODY_IDLE_TIMEOUT;
    }
    let valid = received == u64::from(descriptor.length)
        && hasher.finalize().as_bytes() == descriptor.digest.as_bytes();
    if !valid {
        return Err(if descriptor.accepted {
            StoreError::Conflict
        } else {
            StoreError::CorruptContent
        });
    }
    if let Some(stage) = stage {
        stage.sync_data().map_err(super::io_error)?;
    }
    let current = lock(session);
    if current.state != UploadState::Active || !current.receiving || !current.delta_mode {
        return Err(StoreError::Conflict);
    }
    Ok(())
}

async fn wait_frame<F: Future>(future: F, deadline: tokio::time::Instant) -> Result<F::Output, ()> {
    tokio::time::timeout_at(deadline, future)
        .await
        .map_err(|_| ())
}
