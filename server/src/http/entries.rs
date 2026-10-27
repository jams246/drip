use axum::{
    body::Bytes,
    extract::{State, rejection::BytesRejection},
    http::{HeaderMap, Uri},
};
use drip_core::protocol::{DirectoryCreateRequest, EntryDeleteRequest, EntryDeleteResponse};

use crate::ApiFailure;

use super::{HttpState, parse_json, read_control_body};

pub(super) async fn list_entries(
    State(state): State<HttpState>,
    uri: Uri,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Result<axum::Json<drip_core::protocol::EntryPage>, ApiFailure> {
    read_control_body(body)?;
    let credential = crate::auth::credential(&headers)?;
    let (selection_id, generation, cursor) = parse_entry_query(uri.query())?;
    state
        .store
        .list_entries_at_generation(credential, &selection_id, generation, cursor.as_deref())
        .map(axum::Json)
        .map_err(Into::into)
}

pub(super) async fn create_directory(
    State(state): State<HttpState>,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Result<axum::Json<drip_core::protocol::DirectoryCreateResponse>, ApiFailure> {
    let credential = crate::auth::credential(&headers)?;
    let request: DirectoryCreateRequest = parse_json(body)?;
    state
        .store
        .create_directory(credential, request)
        .map(axum::Json)
        .map_err(Into::into)
}

pub(super) async fn delete_entry(
    State(state): State<HttpState>,
    headers: HeaderMap,
    body: Result<Bytes, BytesRejection>,
) -> Result<axum::Json<EntryDeleteResponse>, ApiFailure> {
    let credential = crate::auth::credential(&headers)?;
    let request: EntryDeleteRequest = parse_json(body)?;
    state
        .store
        .delete_entry(credential, request)
        .map(axum::Json)
        .map_err(Into::into)
}

fn parse_entry_query(
    query: Option<&str>,
) -> Result<(String, Option<u64>, Option<String>), ApiFailure> {
    let mut selection_id = None;
    let mut generation = None;
    let mut cursor = None;
    for pair in query.ok_or_else(ApiFailure::invalid_input)?.split('&') {
        let (key, value) = pair.split_once('=').ok_or_else(ApiFailure::invalid_input)?;
        if value.contains('%') || value.is_empty() {
            return Err(ApiFailure::invalid_input());
        }
        match key {
            "selection_id" if selection_id.is_none() => selection_id = Some(value.to_owned()),
            "generation" if generation.is_none() => {
                generation = Some(value.parse().map_err(|_| ApiFailure::invalid_input())?)
            }
            "cursor" if cursor.is_none() => cursor = Some(value.to_owned()),
            _ => return Err(ApiFailure::invalid_input()),
        }
    }
    Ok((
        selection_id.ok_or_else(ApiFailure::invalid_input)?,
        generation,
        cursor,
    ))
}
