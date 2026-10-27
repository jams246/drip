mod chunking;
mod reconcile;
mod transfer;

use drip_core::protocol::{UploadPrepareRequest, UploadPrepareResponse};
use reqwest::Method;
use tokio::sync::mpsc;

use crate::{
    credentials::Pairing,
    engine::{ActivityEvent, EngineError, UploadResult, api, selection},
    source::SourceFile,
    transport::HttpTransport,
};

pub(super) async fn upload(
    transport: &HttpTransport,
    pairing: &Pairing,
    source: SourceFile,
    events: &mpsc::Sender<ActivityEvent>,
) -> Result<UploadResult, EngineError> {
    let active =
        selection::activate(transport, pairing.credential(), source.protocol_path()).await?;
    upload_selected(transport, pairing, source, &active, None, None, events).await
}

pub(super) async fn upload_selected(
    transport: &HttpTransport,
    pairing: &Pairing,
    source: SourceFile,
    active: &selection::ActiveSelection,
    expected_entry: Option<drip_core::protocol::EntryPrecondition>,
    digest: Option<drip_core::Blake3Digest>,
    events: &mpsc::Sender<ActivityEvent>,
) -> Result<UploadResult, EngineError> {
    let source_path = source.protocol_path().clone();
    let size = source.size();
    let _ = events
        .send(ActivityEvent::UploadStarted {
            source_path: source_path.clone(),
            size,
        })
        .await;
    let credential = pairing.credential();
    source.verify().await?;
    let digest = match digest {
        Some(digest) => digest,
        None => source.hash(events).await?,
    };
    selection::require_current(transport, credential, active).await?;
    let request = UploadPrepareRequest {
        selection_id: active.selection.id.clone(),
        generation: active.generation,
        source_path,
        expected_size: size,
        digest,
        last_write: source.last_write(),
        expected_entry,
    };
    let prepared: UploadPrepareResponse = api::json(
        transport,
        Method::POST,
        "v1/uploads",
        Some(credential),
        &[],
        Some(&request),
    )
    .await?;
    if !lower_hex(&prepared.upload_id, 32) {
        return Err(EngineError::InvalidResponse(
            "upload ID must be 32 lowercase hexadecimal characters",
        ));
    }
    let result = transfer::run(
        transport, credential, active, &source, &prepared, digest, events,
    )
    .await;
    match result {
        Ok(result) => Ok(result),
        Err(transfer_error) => {
            match reconcile::abort(transport, credential, &prepared.upload_id).await {
                Ok(()) => Err(transfer_error),
                Err(abort_error) => Err(EngineError::AbortFailed {
                    transfer: Box::new(transfer_error),
                    abort: Box::new(abort_error),
                }),
            }
        }
    }
}

fn lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
