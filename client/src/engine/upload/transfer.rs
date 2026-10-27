use drip_core::Blake3Digest;
use drip_core::protocol::{UploadCommitResponse, UploadPrepareResponse};
use reqwest::Method;
use tokio::sync::mpsc;

use crate::{
    engine::{
        ActivityEvent, EngineError, UploadResult, api, selection, selection::ActiveSelection,
    },
    source::SourceFile,
    transport::HttpTransport,
};

use super::reconcile;

mod delta;
mod requests;

pub(super) async fn run(
    transport: &HttpTransport,
    credential: &str,
    active: &ActiveSelection,
    source: &SourceFile,
    prepared: &UploadPrepareResponse,
    digest: Blake3Digest,
    events: &mpsc::Sender<ActivityEvent>,
) -> Result<UploadResult, EngineError> {
    let size = source.size();
    selection::require_current(transport, credential, active).await?;
    delta::send(
        transport,
        credential,
        active,
        source,
        &prepared.upload_id,
        digest,
        events,
    )
    .await?;
    selection::require_current(transport, credential, active).await?;
    source.verify().await?;
    let commit = commit(transport, credential, &prepared.upload_id).await;
    let result = match commit {
        Ok(response) => {
            reconcile::verify_entry(
                &response.entry,
                source.protocol_path(),
                size,
                digest,
                source.last_write(),
            )?;
            UploadResult {
                source_path: source.protocol_path().clone(),
                size,
                digest,
            }
        }
        Err(error) if error.is_uncertain_commit() => {
            reconcile::resolve_uncertain(
                transport,
                credential,
                active,
                &prepared.upload_id,
                source,
                size,
                digest,
            )
            .await?
        }
        Err(error) => return Err(error),
    };
    finish(source, result, events).await
}

async fn finish(
    source: &SourceFile,
    result: UploadResult,
    events: &mpsc::Sender<ActivityEvent>,
) -> Result<UploadResult, EngineError> {
    source.verify().await?;
    let _ = events
        .send(ActivityEvent::UploadCompleted {
            source_path: result.source_path.clone(),
            size: result.size,
            digest: result.digest,
        })
        .await;
    Ok(result)
}

async fn commit(
    transport: &HttpTransport,
    credential: &str,
    upload_id: &str,
) -> Result<UploadCommitResponse, EngineError> {
    let path = format!("v1/uploads/{upload_id}/commit");
    api::json::<UploadCommitResponse, ()>(
        transport,
        Method::POST,
        &path,
        Some(credential),
        &[],
        None,
    )
    .await
}

#[cfg(test)]
mod tests;
