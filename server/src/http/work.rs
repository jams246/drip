use std::{sync::Arc, time::Duration};

use axum::{
    body::{Body, to_bytes},
    extract::{Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

use crate::{ApiFailure, Store, StoreError, StoreResult};

use super::{CONTROL_LIMIT, HttpState};

#[derive(Clone)]
pub(super) struct ControlWork {
    store: Store,
    workers: Arc<Semaphore>,
    admission: Arc<OwnedSemaphorePermit>,
}

impl ControlWork {
    pub(super) async fn run<T: Send + 'static>(
        &self,
        operation: impl FnOnce(Store) -> StoreResult<T> + Send + 'static,
    ) -> Result<T, ApiFailure> {
        let worker = Arc::clone(&self.workers)
            .acquire_owned()
            .await
            .map_err(|_| StoreError::Busy)?;
        let store = self.store.clone();
        let admission = Arc::clone(&self.admission);
        tokio::task::spawn_blocking(move || {
            let _permits = (worker, admission);
            operation(store)
        })
        .await
        .map_err(|_| StoreError::Busy)?
        .map_err(Into::into)
    }
}

pub(super) async fn control(
    State(state): State<HttpState>,
    request: Request,
    next: Next,
) -> Response {
    match control_request(state, request, next).await {
        Ok(response) => response,
        Err(error) => error.into_response(),
    }
}

async fn control_request(
    state: HttpState,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiFailure> {
    let admission = Arc::clone(&state.controls)
        .try_acquire_owned()
        .map_err(|_| StoreError::CapacityExhausted)?;
    let work = ControlWork {
        store: state.store,
        workers: state.workers,
        admission: Arc::new(admission),
    };
    if !matches!(request.uri().path(), "/healthz" | "/v1/enroll") {
        let credential = crate::auth::credential(request.headers())?.to_owned();
        work.run(move |store| crate::upload_store::authenticated_device(&store, &credential))
            .await?;
    }
    request.extensions_mut().insert(work);
    let (parts, body) = request.into_parts();
    let result = tokio::time::timeout(Duration::from_secs(15), to_bytes(body, CONTROL_LIMIT))
        .await
        .map_err(|_| ApiFailure::request_timeout())?;
    match result {
        Ok(body) => Ok(next.run(Request::from_parts(parts, Body::from(body))).await),
        Err(error)
            if std::error::Error::source(&error)
                .is_some_and(|source| source.is::<http_body_util::LengthLimitError>()) =>
        {
            Err(ApiFailure::payload_too_large())
        }
        Err(_) => Err(ApiFailure::invalid_input()),
    }
}

pub(super) async fn binary(
    State(state): State<HttpState>,
    request: Request,
    next: Next,
) -> Response {
    let Ok(permit) = Arc::clone(&state.streams).try_acquire_owned() else {
        return ApiFailure::from(StoreError::CapacityExhausted).into_response();
    };
    // The task owns admission until streaming and any blocking cleanup have finished.
    match tokio::spawn(async move {
        let _permit = permit;
        next.run(request).await
    })
    .await
    {
        Ok(response) => response,
        Err(_) => ApiFailure::from(StoreError::Busy).into_response(),
    }
}

#[cfg(test)]
mod tests;
