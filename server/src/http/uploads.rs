use axum::{
    body::{Body, Bytes},
    extract::{Path, State, rejection::BytesRejection},
    http::{HeaderMap, StatusCode},
};
use drip_core::protocol::UploadPlanRequest;
use drip_core::protocol::UploadPrepareRequest;

use crate::ApiFailure;

use super::{HttpState, parse_json, read_control_body};

pub(super) async fn prepare_upload(
    State(state): State<HttpState>,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Result<axum::Json<drip_core::protocol::UploadPrepareResponse>, ApiFailure> {
    let credential = crate::auth::credential(&headers)?;
    let request: UploadPrepareRequest = parse_json(body)?;
    state
        .store
        .prepare_upload(credential, request)
        .map(axum::Json)
        .map_err(Into::into)
}

pub(super) async fn upload_data(
    State(state): State<HttpState>,
    Path(upload_id): Path<String>,
    headers: HeaderMap,
    body: Body,
) -> Result<StatusCode, ApiFailure> {
    let credential = crate::auth::credential(&headers)?;
    state
        .store
        .put_upload_data(credential, &upload_id, body)
        .await?;
    Ok(StatusCode::OK)
}

pub(super) async fn plan_upload(
    State(state): State<HttpState>,
    Path(upload_id): Path<String>,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Result<axum::Json<drip_core::protocol::UploadPlanResponse>, ApiFailure> {
    let credential = crate::auth::credential(&headers)?;
    let request: UploadPlanRequest = parse_json(body)?;
    state
        .store
        .plan_upload(credential, &upload_id, request)
        .map(axum::Json)
        .map_err(Into::into)
}

pub(super) async fn upload_chunk(
    State(state): State<HttpState>,
    Path((upload_id, ordinal)): Path<(String, u64)>,
    headers: HeaderMap,
    body: Body,
) -> Result<StatusCode, ApiFailure> {
    let credential = crate::auth::credential(&headers)?;
    state
        .store
        .put_upload_chunk(credential, &upload_id, ordinal, body)
        .await?;
    Ok(StatusCode::OK)
}

pub(super) async fn commit_upload(
    State(state): State<HttpState>,
    Path(upload_id): Path<String>,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Result<axum::Json<drip_core::protocol::UploadCommitResponse>, ApiFailure> {
    read_control_body(body)?;
    let credential = crate::auth::credential(&headers)?;
    state
        .store
        .commit_upload(credential, &upload_id)
        .map(axum::Json)
        .map_err(Into::into)
}

pub(super) async fn get_upload_state(
    State(state): State<HttpState>,
    Path(upload_id): Path<String>,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Result<axum::Json<drip_core::protocol::UploadStateResponse>, ApiFailure> {
    read_control_body(body)?;
    let credential = crate::auth::credential(&headers)?;
    state
        .store
        .get_upload_state(credential, &upload_id)
        .map(axum::Json)
        .map_err(Into::into)
}

pub(super) async fn abort_upload(
    State(state): State<HttpState>,
    Path(upload_id): Path<String>,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Result<StatusCode, ApiFailure> {
    read_control_body(body)?;
    let credential = crate::auth::credential(&headers)?;
    state
        .store
        .abort_upload(credential, &upload_id)
        .map_err(ApiFailure::from)?;
    Ok(StatusCode::NO_CONTENT)
}
