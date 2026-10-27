use std::{
    future::Future,
    io::{self, Write},
    sync::{Arc, Mutex, MutexGuard},
    time::Duration,
};

use axum::body::Body;
use drip_core::protocol::UploadState;
use http_body_util::BodyExt;

use crate::{
    Store, StoreError, StoreResult, upload_sessions::UploadSession,
    upload_store::authenticated_device,
};

const SESSION_TTL: i64 = 30 * 60;
const BODY_IDLE_TIMEOUT: Duration = Duration::from_secs(SESSION_TTL as u64);

impl Store {
    /// Streams a complete file body into its session's private staging file.
    pub async fn put_upload_data(
        &self,
        credential: &str,
        upload_id: &str,
        mut body: Body,
    ) -> StoreResult<()> {
        let device = authenticated_device(self, credential)?;
        let now = self.now()?;
        self.reap_uploads(now)?;
        let session = self.uploads.get(upload_id, &device.pc_id, now)?;
        let (stage_name, expected_size, expected_digest) = {
            let mut session = lock(&session);
            if session.state != UploadState::Active
                || session.receiving
                || session.committing
                || session.planning
                || session.delta_mode
            {
                return Err(StoreError::Conflict);
            }
            session.receiving = true;
            session.has_data = false;
            (
                session.stage_name.clone(),
                session.expected_size,
                session.digest,
            )
        };
        let mut file = match self.uploads.open_stage_for_write(&stage_name) {
            Ok(file) => file,
            Err(error) => {
                lock(&session).receiving = false;
                return Err(error);
            }
        };
        let mut hasher = blake3::Hasher::new();
        let mut received = 0u64;
        let mut frame_deadline = tokio::time::Instant::now() + BODY_IDLE_TIMEOUT;
        let outcome = async {
            loop {
                let frames = match wait_frame(body.frame(), frame_deadline).await {
                    Ok(frames) => frames,
                    Err(()) => {
                        let now = self.now().unwrap_or_else(|_| {
                            lock(&session).last_progress.saturating_add(SESSION_TTL)
                        });
                        expire_body(self, &session, &stage_name, now);
                        return Err(StoreError::NotFound);
                    }
                };
                let Some(frame) = frames else {
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
                if next > expected_size {
                    return Err(StoreError::InvalidInput);
                }
                let now = self.now()?;
                progress(&session, now)?;
                file.write_all(&bytes).map_err(map_staging_io)?;
                hasher.update(&bytes);
                received = next;
                frame_deadline = tokio::time::Instant::now() + BODY_IDLE_TIMEOUT;
            }
            if received != expected_size
                || hasher.finalize().as_bytes() != expected_digest.as_bytes()
            {
                return Err(StoreError::InvalidInput);
            }
            file.sync_all().map_err(map_staging_io)?;
            Ok(())
        }
        .await;
        drop(file);
        if let Err(error) = outcome {
            reset_body(self, &session, &stage_name);
            return Err(error);
        }
        let now = self.now()?;
        let mut session = lock(&session);
        if session.state != UploadState::Active || !session.receiving {
            drop(session);
            let _ = self.uploads.remove_stage(&stage_name);
            return Err(StoreError::Conflict);
        }
        session.receiving = false;
        session.has_data = true;
        session.last_progress = now;
        Ok(())
    }
}

async fn wait_frame<F: Future>(future: F, deadline: tokio::time::Instant) -> Result<F::Output, ()> {
    tokio::time::timeout_at(deadline, future)
        .await
        .map_err(|_| ())
}

fn progress(session: &Arc<Mutex<UploadSession>>, now: i64) -> StoreResult<()> {
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
    if session.state == UploadState::Active {
        session.receiving = false;
        session.has_data = false;
    }
    drop(session);
    let _ = store.uploads.remove_stage(stage_name);
}

fn expire_body(store: &Store, session: &Arc<Mutex<UploadSession>>, stage_name: &str, now: i64) {
    let mut session = lock(session);
    if session.state == UploadState::Active {
        session.state = UploadState::Aborted;
        session.receiving = false;
        session.has_data = false;
        session.last_progress = now;
    }
    drop(session);
    let _ = store.uploads.remove_stage(stage_name);
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

#[cfg(test)]
mod tests {
    use std::{future::pending, time::Duration};

    use super::wait_frame;

    #[tokio::test]
    async fn body_frame_timeout_ends_a_stalled_request() {
        assert!(
            wait_frame(
                pending::<()>(),
                tokio::time::Instant::now() + Duration::from_millis(1),
            )
            .await
            .is_err()
        );
    }
}
