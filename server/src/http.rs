use axum::{
    Router,
    body::Bytes,
    extract::{DefaultBodyLimit, State, rejection::BytesRejection},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use drip_core::protocol::{EnrollmentRequest, SelectionUpdate};
use tokio::net::TcpListener;

use crate::{ApiFailure, Store};

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
        .route("/v1/enroll", axum::routing::post(enroll))
        .route("/v1/selections", get(get_selections).put(put_selections))
        .layer(middleware::from_fn(require_control_body_limit))
        .layer(DefaultBodyLimit::max(CONTROL_LIMIT))
        .with_state(state);
    health.merge(control)
}

/// Serves the router and stops on process termination signals.
pub async fn serve(listener: TcpListener, store: Store) -> std::io::Result<()> {
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
    let update: SelectionUpdate = parse_json(body)?;
    state
        .store
        .update_selections(credential, update)
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
    let response = next.run(request).await;
    if response.status() == StatusCode::PAYLOAD_TOO_LARGE {
        return ApiFailure::payload_too_large().into_response();
    }
    response
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
