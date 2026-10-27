use axum::{
    body::{Body, Bytes},
    extract::{Path, State, rejection::BytesRejection},
    http::{HeaderMap, StatusCode, Uri},
};
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

pub(super) async fn list_entries(
    State(state): State<HttpState>,
    uri: Uri,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Result<axum::Json<drip_core::protocol::EntryPage>, ApiFailure> {
    read_control_body(body)?;
    let credential = crate::auth::credential(&headers)?;
    let (selection_id, cursor) = parse_entry_query(uri.query())?;
    state
        .store
        .list_entries(credential, &selection_id, cursor.as_deref())
        .map(axum::Json)
        .map_err(Into::into)
}

fn parse_entry_query(query: Option<&str>) -> Result<(String, Option<String>), ApiFailure> {
    let mut selection_id = None;
    let mut cursor = None;
    for pair in query.ok_or_else(ApiFailure::invalid_input)?.split('&') {
        let (key, value) = pair.split_once('=').ok_or_else(ApiFailure::invalid_input)?;
        if value.contains('%') || value.is_empty() {
            return Err(ApiFailure::invalid_input());
        }
        match key {
            "selection_id" if selection_id.is_none() => selection_id = Some(value.to_owned()),
            "cursor" if cursor.is_none() => cursor = Some(value.to_owned()),
            _ => return Err(ApiFailure::invalid_input()),
        }
    }
    Ok((selection_id.ok_or_else(ApiFailure::invalid_input)?, cursor))
}
