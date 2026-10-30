use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::body::{Body, Bytes};
use http_body_util::BodyExt;
use tokio::time::Instant;

use crate::{
    Store, StoreError, StoreResult, upload_sessions::UploadSession,
    upload_store::authenticated_device,
};

use super::{SESSION_TTL, lock, progress};

const AUTH_INTERVAL: Duration = Duration::from_secs(5);
const IDLE_TIMEOUT: Duration = Duration::from_secs(SESSION_TTL as u64);

pub(crate) async fn blocking<T: Send + 'static>(
    operation: impl FnOnce() -> StoreResult<T> + Send + 'static,
) -> StoreResult<T> {
    tokio::task::spawn_blocking(operation)
        .await
        .map_err(|_| StoreError::Busy)?
}

pub(crate) struct StreamReader {
    store: Store,
    credential: String,
    session: Arc<Mutex<UploadSession>>,
    idle: Instant,
    authenticate_at: Instant,
}

impl StreamReader {
    pub(crate) fn new(
        store: &Store,
        credential: &str,
        session: &Arc<Mutex<UploadSession>>,
    ) -> Self {
        Self {
            store: store.clone(),
            credential: credential.to_owned(),
            session: Arc::clone(session),
            idle: Instant::now() + IDLE_TIMEOUT,
            authenticate_at: Instant::now() + AUTH_INTERVAL,
        }
    }

    pub(crate) async fn next(&mut self, body: &mut Body) -> StoreResult<Option<Bytes>> {
        loop {
            let frame = tokio::select! {
                biased;
                _ = tokio::time::sleep_until(self.idle) => {
                    self.abort();
                    return Err(StoreError::NotFound);
                }
                _ = tokio::time::sleep_until(self.authenticate_at) => {
                    self.authorize().await?;
                    continue;
                }
                frame = body.frame() => frame,
            };
            let Some(frame) = frame else {
                self.authorize().await?;
                return Ok(None);
            };
            let Ok(bytes) = frame.map_err(|_| StoreError::InvalidInput)?.into_data() else {
                continue;
            };
            if bytes.is_empty() {
                continue;
            }
            progress(&self.session, self.store.now()?)?;
            self.idle = Instant::now() + IDLE_TIMEOUT;
            return Ok(Some(bytes));
        }
    }

    async fn authorize(&mut self) -> StoreResult<()> {
        let store = self.store.clone();
        let credential = self.credential.clone();
        if let Err(error) = blocking(move || authenticated_device(&store, &credential)).await {
            if matches!(error, StoreError::InvalidCredential) {
                self.abort();
            }
            return Err(error);
        }
        let current = lock(&self.session);
        if current.state != drip_core::protocol::UploadState::Active || !current.receiving {
            return Err(StoreError::Conflict);
        }
        self.authenticate_at = Instant::now() + AUTH_INTERVAL;
        Ok(())
    }

    fn abort(&self) {
        let mut current = lock(&self.session);
        if current.state == drip_core::protocol::UploadState::Active {
            current.state = drip_core::protocol::UploadState::Aborted;
            current.receiving = false;
            current.has_data = false;
        }
    }
}
