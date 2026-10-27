use std::{io, sync::Arc};

use drip_core::protocol::{UploadCommitResponse, UploadPrepareResponse};
use drip_core::{Blake3Digest, SourcePath};
use futures_util::stream;
use reqwest::{
    Method,
    header::{CONTENT_LENGTH, CONTENT_TYPE},
};
use tokio::{
    fs::File,
    io::AsyncReadExt,
    sync::{Mutex, mpsc},
};

use crate::{
    engine::{
        ActivityEvent, EngineError, ProgressSnapshot, UploadPhase, UploadResult, api, selection,
        selection::ActiveSelection,
    },
    source::SourceFile,
    transport::{HttpTransport, TransportError},
};

use super::reconcile;

const CHUNK_BYTES: usize = 64 * 1024;
const REPORT_BYTES: u64 = 1024 * 1024;

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
    send_data(
        transport, credential, active, source, prepared, size, events,
    )
    .await?;
    selection::require_current(transport, credential, active).await?;
    source.verify().await?;
    let commit = commit(transport, credential, &prepared.upload_id).await;
    let result = match commit {
        Ok(response) => {
            reconcile::verify_entry(
                &response.entry,
                &active.selection.source,
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
    let _ = events.try_send(ActivityEvent::UploadCompleted {
        source_path: result.source_path.clone(),
        size,
        digest,
    });
    Ok(result)
}

async fn send_data(
    transport: &HttpTransport,
    credential: &str,
    active: &ActiveSelection,
    source: &SourceFile,
    prepared: &UploadPrepareResponse,
    size: u64,
    events: &mpsc::Sender<ActivityEvent>,
) -> Result<(), EngineError> {
    selection::require_current(transport, credential, active).await?;
    let stream = body_stream(
        source.reader(),
        size,
        active.selection.source.clone(),
        events.clone(),
    );
    let path = format!("v1/uploads/{}/data", prepared.upload_id);
    let request = transport
        .request(Method::PUT, &path)?
        .bearer_auth(credential)
        .header(CONTENT_TYPE, "application/octet-stream")
        .header(CONTENT_LENGTH, size.to_string())
        .body(reqwest::Body::wrap_stream(stream))
        .build()
        .map_err(TransportError::Build)?;
    api::ensure_success(transport.send(request).await?).await
}

fn body_stream(
    reader: Arc<Mutex<File>>,
    size: u64,
    source_path: SourcePath,
    events: mpsc::Sender<ActivityEvent>,
) -> impl futures_util::TryStream<Ok = Vec<u8>, Error = io::Error> + Send + 'static {
    stream::try_unfold(
        (reader, 0u64, 0u64, source_path, events),
        move |(reader, sent, next_report, source_path, events)| async move {
            if sent == size {
                return Ok(None);
            }
            let capacity = usize::try_from((size - sent).min(CHUNK_BYTES as u64))
                .map_err(|_| io::Error::other("file chunk length is invalid"))?;
            let mut bytes = vec![0; capacity];
            let count = reader.lock().await.read(&mut bytes).await?;
            if count == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "source ended before its recorded size",
                ));
            }
            bytes.truncate(count);
            let processed = sent + count as u64;
            let next_report = if processed >= next_report || processed == size {
                let _ = events.try_send(ActivityEvent::Progress(ProgressSnapshot {
                    phase: UploadPhase::Uploading,
                    source_path: source_path.clone(),
                    processed_bytes: processed,
                    total_bytes: size,
                }));
                processed.saturating_add(REPORT_BYTES)
            } else {
                next_report
            };
            Ok(Some((
                bytes,
                (reader, processed, next_report, source_path, events),
            )))
        },
    )
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
