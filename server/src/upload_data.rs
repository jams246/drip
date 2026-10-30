use std::{
    io,
    sync::{Arc, Mutex, MutexGuard},
};

use axum::body::Body;
use drip_core::protocol::UploadState;
use tokio::io::AsyncWriteExt;

use crate::{
    Store, StoreError, StoreResult, upload_sessions::UploadSession,
    upload_store::authenticated_device,
};

mod stream;
pub(crate) use stream::{StreamReader, blocking};

pub(crate) const SESSION_TTL: i64 = 30 * 60;

impl Store {
    /// Streams a complete file body into its session's private staging file.
    pub async fn put_upload_data(
        &self,
        credential: &str,
        upload_id: &str,
        mut body: Body,
    ) -> StoreResult<()> {
        let store = self.clone();
        let secret = credential.to_owned();
        let id = upload_id.to_owned();
        let (session, stage_name, expected_size, expected_digest, file) = blocking(move || {
            let device = authenticated_device(&store, &secret)?;
            let now = store.now()?;
            store.reap_uploads(now)?;
            let session = store.uploads.get(&id, &device.pc_id, now)?;
            let mut current = lock(&session);
            if current.state != UploadState::Active
                || current.receiving
                || current.committing
                || current.planning
                || current.delta_mode
            {
                return Err(StoreError::Conflict);
            }
            let file = store.uploads.open_stage_for_write(&current.stage_name)?;
            current.receiving = true;
            current.has_data = false;
            let details = (
                current.stage_name.clone(),
                current.expected_size,
                current.digest,
            );
            drop(current);
            Ok((session, details.0, details.1, details.2, file.into_std()))
        })
        .await?;
        let mut file = tokio::fs::File::from_std(file);
        let mut reader = StreamReader::new(self, credential, &session);
        let mut hasher = blake3::Hasher::new();
        let mut received = 0u64;
        let outcome = async {
            while let Some(bytes) = reader.next(&mut body).await? {
                let next = received
                    .checked_add(bytes.len() as u64)
                    .ok_or(StoreError::InvalidInput)?;
                if next > expected_size {
                    return Err(StoreError::InvalidInput);
                }
                file.write_all(&bytes).await.map_err(map_staging_io)?;
                hasher.update(&bytes);
                received = next;
            }
            if received != expected_size
                || hasher.finalize().as_bytes() != expected_digest.as_bytes()
            {
                return Err(StoreError::InvalidInput);
            }
            file.sync_all().await.map_err(map_staging_io)?;
            Ok(())
        }
        .await;
        // Complete pending async file work before removing or publishing its staging handle.
        drop(file.into_std().await);
        let store = self.clone();
        let id = upload_id.to_owned();
        blocking(move || {
            if let Err(error) = outcome {
                reset_body(&store, &session, &stage_name);
                if lock(&session).state == UploadState::Aborted {
                    store.delete_upload_plans(&[id])?;
                }
                return Err(error);
            }
            let now = match store.now() {
                Ok(now) => now,
                Err(error) => {
                    reset_body(&store, &session, &stage_name);
                    return Err(error);
                }
            };
            let mut current = lock(&session);
            if current.state != UploadState::Active || !current.receiving {
                drop(current);
                let _ = store.uploads.remove_stage(&stage_name);
                return Err(StoreError::Conflict);
            }
            current.receiving = false;
            current.has_data = true;
            current.last_progress = now;
            Ok(())
        })
        .await
    }
}

pub(crate) fn progress(session: &Arc<Mutex<UploadSession>>, now: i64) -> StoreResult<()> {
    let mut session = lock(session);
    if session.state != UploadState::Active || !session.receiving {
        return Err(StoreError::Conflict);
    }
    if now.saturating_sub(session.last_progress) >= SESSION_TTL {
        session.state = UploadState::Aborted;
        session.receiving = false;
        session.has_data = false;
        session.last_progress = now;
        return Err(StoreError::NotFound);
    }
    session.last_progress = now;
    Ok(())
}

fn reset_body(store: &Store, session: &Arc<Mutex<UploadSession>>, stage_name: &str) {
    let mut session = lock(session);
    let _ = store.uploads.remove_stage(stage_name);
    if session.state == UploadState::Active {
        session.receiving = false;
        session.has_data = false;
    }
}

fn map_staging_io(error: io::Error) -> StoreError {
    if error.kind() == io::ErrorKind::StorageFull {
        StoreError::InsufficientStagingSpace
    } else {
        crate::database::io_error(error)
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
