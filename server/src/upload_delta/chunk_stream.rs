use std::sync::{Arc, Mutex};

use axum::body::Body;
use tokio::io::AsyncWriteExt;

use crate::{
    Store, StoreError, StoreResult, upload_data::StreamReader, upload_sessions::UploadSession,
};

use super::{lock, state};

pub(super) async fn receive(
    store: &Store,
    credential: &str,
    session: &Arc<Mutex<UploadSession>>,
    descriptor: state::ChunkRow,
    stage: Option<std::fs::File>,
    body: &mut Body,
) -> StoreResult<()> {
    let mut stage = stage.map(tokio::fs::File::from_std);
    let mut reader = StreamReader::new(store, credential, session);
    let mut hasher = blake3::Hasher::new();
    let mut received = 0u64;
    let outcome = async {
        while let Some(bytes) = reader.next(body).await? {
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
            if let Some(stage) = &mut stage {
                stage.write_all(&bytes).await.map_err(super::io_error)?;
            }
            hasher.update(&bytes);
            received = next;
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
        if let Some(stage) = &stage {
            stage.sync_data().await.map_err(super::io_error)?;
        }
        let current = lock(session);
        if current.state != drip_core::protocol::UploadState::Active
            || !current.receiving
            || !current.delta_mode
        {
            return Err(StoreError::Conflict);
        }
        Ok(())
    }
    .await;
    if let Some(stage) = stage {
        drop(stage.into_std().await);
    }
    outcome
}
