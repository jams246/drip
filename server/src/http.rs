use std::sync::Arc;

use axum::{
    Extension, Router,
    body::Bytes,
    extract::{DefaultBodyLimit, rejection::BytesRejection},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use drip_core::protocol::{EnrollmentRequest, SelectionRelocationRequest};
use tokio::sync::Semaphore;

use crate::{ApiFailure, Store};

mod daemon;
mod entries;
mod uploads;
mod work;

pub use daemon::serve;
use work::ControlWork;

const CONTROL_LIMIT: usize = 256 * 1024;

#[derive(Clone)]
struct HttpState {
    store: Store,
    controls: Arc<Semaphore>,
    workers: Arc<Semaphore>,
    streams: Arc<Semaphore>,
}

impl HttpState {
    fn new(store: Store) -> Self {
        Self {
            store,
            controls: Arc::new(Semaphore::new(32)),
            workers: Arc::new(Semaphore::new(8)),
            streams: Arc::new(Semaphore::new(8)),
        }
    }

    async fn drain(&self) {
        let _controls = self.controls.acquire_many(32).await;
        let _streams = self.streams.acquire_many(8).await;
    }
}

/// Builds health and capped control routes. Add streamed upload routes outside the control router.
pub fn router(store: Store) -> Router {
    router_with_state(HttpState::new(store))
}

fn router_with_state(state: HttpState) -> Router {
    let control = Router::new()
        .route("/healthz", get(health))
        .route("/v1/enroll", post(enroll))
        .route("/v1/selections", get(get_selections).put(put_selections))
        .route("/v1/uploads", post(uploads::prepare_upload))
        .route("/v1/uploads/{upload_id}/plan", post(uploads::plan_upload))
        .route(
            "/v1/uploads/{upload_id}",
            get(uploads::get_upload_state).delete(uploads::abort_upload),
        )
        .route(
            "/v1/uploads/{upload_id}/commit",
            post(uploads::commit_upload),
        )
        .route("/v1/entries", get(entries::list_entries))
        .route("/v1/directories", post(entries::create_directory))
        .route("/v1/entries/delete", post(entries::delete_entry))
        .layer(middleware::from_fn_with_state(state.clone(), work::control))
        .layer(DefaultBodyLimit::max(CONTROL_LIMIT))
        .with_state(state.clone());
    let binary = Router::new()
        .route("/v1/uploads/{upload_id}/data", put(uploads::upload_data))
        .route(
            "/v1/uploads/{upload_id}/chunks/{ordinal}",
            put(uploads::upload_chunk),
        )
        .layer(middleware::from_fn_with_state(state.clone(), work::binary))
        .layer(DefaultBodyLimit::disable())
        .with_state(state);
    control
        .merge(binary)
        .layer(middleware::from_fn(normalize_api_rejections))
}

async fn health(body: Result<Bytes, BytesRejection>) -> Result<impl IntoResponse, ApiFailure> {
    read_control_body(body)?;
    Ok((StatusCode::OK, "ok"))
}

async fn enroll(
    Extension(work): Extension<ControlWork>,
    body: Result<Bytes, BytesRejection>,
) -> Result<axum::Json<drip_core::protocol::EnrollmentResponse>, ApiFailure> {
    let request: EnrollmentRequest = parse_json(body)?;
    work.run(move |store| store.enroll(request))
        .await
        .map(axum::Json)
}

async fn get_selections(
    Extension(work): Extension<ControlWork>,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Result<axum::Json<drip_core::protocol::SelectionSet>, ApiFailure> {
    read_control_body(body)?;
    let credential = crate::auth::credential(&headers)?.to_owned();
    work.run(move |store| store.get_selections(&credential))
        .await
        .map(axum::Json)
}

async fn put_selections(
    Extension(work): Extension<ControlWork>,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Result<axum::Json<drip_core::protocol::SelectionUpdateResponse>, ApiFailure> {
    let credential = crate::auth::credential(&headers)?.to_owned();
    let request: SelectionRelocationRequest = parse_json(body)?;
    work.run(move |store| {
        store.update_selections_relocating(&credential, request.update, &request.root_moves)
    })
    .await
    .map(axum::Json)
}

fn parse_json<T: serde::de::DeserializeOwned>(
    body: Result<Bytes, BytesRejection>,
) -> Result<T, ApiFailure> {
    let body = read_control_body(body)?;
    serde_json::from_slice(&body).map_err(|_| ApiFailure::invalid_input())
}

fn read_control_body(body: Result<Bytes, BytesRejection>) -> Result<Bytes, ApiFailure> {
    match body {
        Ok(body) => Ok(body),
        Err(error) if error.status() == StatusCode::PAYLOAD_TOO_LARGE => {
            Err(ApiFailure::payload_too_large())
        }
        Err(_) => Err(ApiFailure::invalid_input()),
    }
}

async fn normalize_api_rejections(request: axum::extract::Request, next: Next) -> Response {
    normalize_response(next.run(request).await)
}

fn normalize_response(response: Response) -> Response {
    if ApiFailure::is_marked(&response) {
        return response;
    }
    match response.status() {
        StatusCode::BAD_REQUEST => ApiFailure::invalid_input().into_response(),
        StatusCode::NOT_FOUND => ApiFailure::not_found().into_response(),
        StatusCode::PAYLOAD_TOO_LARGE => ApiFailure::payload_too_large().into_response(),
        _ => response,
    }
}
