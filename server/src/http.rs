use axum::{
    Router,
    body::{Body, Bytes, to_bytes},
    extract::{DefaultBodyLimit, State, rejection::BytesRejection},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post, put},
};
use drip_core::protocol::{EnrollmentRequest, SelectionRelocationRequest};
use tokio::net::TcpListener;

use crate::{ApiFailure, Store};

mod entries;
mod uploads;

const CONTROL_LIMIT: usize = 256 * 1024;

#[derive(Clone)]
struct HttpState {
    store: Store,
}

/// Builds health and capped control routes. Add streamed upload routes outside the control router.
pub fn router(store: Store) -> Router {
    let state = HttpState { store };
    let health = Router::new()
        .route("/healthz", get(health))
        .layer(DefaultBodyLimit::max(CONTROL_LIMIT))
        .with_state(state.clone());
    let control = Router::new()
        .route("/v1/enroll", post(enroll))
        .route("/v1/selections", get(get_selections).put(put_selections))
        .route("/v1/uploads", post(uploads::prepare_upload))
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
        .layer(middleware::from_fn(require_control_body_limit))
        .layer(DefaultBodyLimit::max(CONTROL_LIMIT))
        .with_state(state.clone());
    let binary = Router::new()
        .route("/v1/uploads/{upload_id}/data", put(uploads::upload_data))
        .layer(DefaultBodyLimit::disable())
        .with_state(state);
    health
        .merge(control)
        .merge(binary)
        .layer(middleware::from_fn(normalize_api_rejections))
}

/// Serves the router and stops on process termination signals.
pub async fn serve(listener: TcpListener, store: Store) -> std::io::Result<()> {
    store
        .prepare_daemon()
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    let expiry_store = store.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            if let Ok(now) = expiry_store.now() {
                expiry_store.uploads.reap_expired(now);
            }
        }
    });
    axum::serve(listener, router(store))
        .with_graceful_shutdown(shutdown_signal())
        .await
}

async fn health(body: Result<Bytes, BytesRejection>) -> Result<impl IntoResponse, ApiFailure> {
    read_control_body(body)?;
    Ok((StatusCode::OK, "ok"))
}

async fn enroll(
    State(state): State<HttpState>,
    body: Result<Bytes, BytesRejection>,
) -> Result<axum::Json<drip_core::protocol::EnrollmentResponse>, ApiFailure> {
    let request: EnrollmentRequest = parse_json(body)?;
    state
        .store
        .enroll(request)
        .map(axum::Json)
        .map_err(Into::into)
}

async fn get_selections(
    State(state): State<HttpState>,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Result<axum::Json<drip_core::protocol::SelectionSet>, ApiFailure> {
    read_control_body(body)?;
    let credential = crate::auth::credential(&headers)?;
    state
        .store
        .get_selections(credential)
        .map(axum::Json)
        .map_err(Into::into)
}

async fn put_selections(
    State(state): State<HttpState>,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Result<axum::Json<drip_core::protocol::SelectionUpdateResponse>, ApiFailure> {
    let credential = crate::auth::credential(&headers)?;
    let request: SelectionRelocationRequest = parse_json(body)?;
    state
        .store
        .update_selections_relocating(credential, request.update, &request.root_moves)
        .map(axum::Json)
        .map_err(Into::into)
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

async fn require_control_body_limit(request: axum::extract::Request, next: Next) -> Response {
    let (parts, body) = request.into_parts();
    match to_bytes(body, CONTROL_LIMIT).await {
        Ok(body) => normalize_response(
            next.run(axum::extract::Request::from_parts(parts, Body::from(body)))
                .await,
        ),
        Err(error)
            if std::error::Error::source(&error)
                .is_some_and(|source| source.is::<http_body_util::LengthLimitError>()) =>
        {
            ApiFailure::payload_too_large().into_response()
        }
        Err(_) => ApiFailure::invalid_input().into_response(),
    }
}

async fn normalize_api_rejections(request: axum::extract::Request, next: Next) -> Response {
    normalize_response(next.run(request).await)
}

fn normalize_response(response: Response) -> Response {
    match response.status() {
        StatusCode::BAD_REQUEST => ApiFailure::invalid_input().into_response(),
        StatusCode::NOT_FOUND => ApiFailure::not_found().into_response(),
        StatusCode::PAYLOAD_TOO_LARGE => ApiFailure::payload_too_large().into_response(),
        _ => response,
    }
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let terminate = signal(SignalKind::terminate());
        if let Ok(mut terminate) = terminate {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {},
                _ = terminate.recv() => {},
            }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}
