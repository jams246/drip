use std::collections::HashSet;

use drip_core::Blake3Digest;
use drip_core::protocol::{Entry, EntryKind, EntryPage, UploadState, UploadStateResponse};
use reqwest::{Method, StatusCode};

use crate::{
    engine::{EngineError, UploadResult, api, selection, selection::ActiveSelection},
    source::SourceFile,
    transport::HttpTransport,
};

pub(super) async fn resolve_uncertain(
    transport: &HttpTransport,
    credential: &str,
    active: &ActiveSelection,
    upload_id: &str,
    source: &SourceFile,
    size: u64,
    digest: Blake3Digest,
) -> Result<UploadResult, EngineError> {
    let state = session_state(transport, credential, upload_id).await?;
    if state
        .as_ref()
        .is_some_and(|state| state.upload_id != upload_id)
    {
        return Err(EngineError::InvalidResponse(
            "upload state returned a different upload ID",
        ));
    }
    if state
        .as_ref()
        .is_some_and(|state| state.state == UploadState::Aborted)
    {
        return Err(EngineError::UploadAborted);
    }
    let barrier = selection::fresh_barrier(transport, credential, active).await?;
    if inventory_matches(transport, credential, &barrier, source, size, digest).await? {
        return Ok(UploadResult {
            source_path: source.protocol_path().clone(),
            size,
            digest,
        });
    }
    Err(EngineError::CommitUncertain)
}

async fn session_state(
    transport: &HttpTransport,
    credential: &str,
    upload_id: &str,
) -> Result<Option<UploadStateResponse>, EngineError> {
    let path = format!("v1/uploads/{upload_id}");
    match api::json::<UploadStateResponse, ()>(
        transport,
        Method::GET,
        &path,
        Some(credential),
        &[],
        None,
    )
    .await
    {
        Ok(state) => Ok(Some(state)),
        Err(EngineError::Api { status, code, .. })
            if status == StatusCode::NOT_FOUND
                && code == drip_core::protocol::ApiErrorCode::NotFound =>
        {
            Ok(None)
        }
        Err(error) if error.is_uncertain_commit() => Ok(None),
        Err(error) => Err(error),
    }
}

async fn inventory_matches(
    transport: &HttpTransport,
    credential: &str,
    active: &ActiveSelection,
    source: &SourceFile,
    size: u64,
    digest: Blake3Digest,
) -> Result<bool, EngineError> {
    let mut cursor: Option<String> = None;
    let mut seen = HashSet::new();
    let mut found = false;
    loop {
        let page = if let Some(cursor_value) = cursor.as_deref() {
            let query = [
                ("selection_id", active.selection.id.as_str()),
                ("cursor", cursor_value),
            ];
            request_page(transport, credential, &query).await?
        } else {
            let query = [("selection_id", active.selection.id.as_str())];
            request_page(transport, credential, &query).await?
        };
        if page.selection_id != active.selection.id || page.entries.len() > 1024 {
            return Err(EngineError::InvalidResponse(
                "inventory page has invalid selection or size",
            ));
        }
        for entry in &page.entries {
            if entry.source_path != active.selection.source {
                continue;
            }
            if found {
                return Err(EngineError::InvalidResponse(
                    "inventory contains duplicate source entries",
                ));
            }
            match verify_entry(
                entry,
                &active.selection.source,
                size,
                digest,
                source.last_write(),
            ) {
                Ok(()) => found = true,
                Err(EngineError::DestinationMismatch) => return Ok(false),
                Err(error) => return Err(error),
            }
        }
        let Some(next) = page.next_cursor else {
            return Ok(found);
        };
        if next.is_empty() || next.len() > 4096 || !seen.insert(next.clone()) {
            return Err(EngineError::InvalidResponse(
                "inventory cursor is empty, oversized, or repeated",
            ));
        }
        cursor = Some(next);
    }
}

async fn request_page(
    transport: &HttpTransport,
    credential: &str,
    query: &[(&str, &str)],
) -> Result<EntryPage, EngineError> {
    api::json::<EntryPage, ()>(
        transport,
        Method::GET,
        "v1/entries",
        Some(credential),
        query,
        None,
    )
    .await
}

pub(super) async fn abort(
    transport: &HttpTransport,
    credential: &str,
    upload_id: &str,
) -> Result<(), EngineError> {
    let _ = selection::get(transport, credential).await;
    let path = format!("v1/uploads/{upload_id}");
    match api::status(transport, Method::DELETE, &path, credential).await {
        Ok(()) => Ok(()),
        Err(EngineError::Api { status, .. }) if status == StatusCode::NOT_FOUND => Ok(()),
        Err(error) => Err(error),
    }
}

pub(super) fn verify_entry(
    entry: &Entry,
    expected_path: &drip_core::SourcePath,
    size: u64,
    digest: Blake3Digest,
    last_write: drip_core::Timestamp,
) -> Result<(), EngineError> {
    if entry.kind != EntryKind::File
        || entry.source_path != *expected_path
        || entry.size != size
        || entry.digest != Some(digest)
        || entry.last_write != Some(last_write)
    {
        return Err(EngineError::DestinationMismatch);
    }
    Ok(())
}
