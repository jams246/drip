use std::{
    io,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use drip_core::protocol::{UploadPlanRequest, UploadPlanResponse};
use drip_core::{ChunkDescriptor, SourcePath};
use futures_util::stream;
use reqwest::{
    Method,
    header::{CONTENT_LENGTH, CONTENT_TYPE},
};
use tokio::sync::{Mutex, mpsc};

use crate::{
    engine::{ActivityEvent, EngineError, ProgressSnapshot, UploadPhase, api},
    transport::{HttpTransport, TransportError},
};

const MAX_ATTEMPTS: usize = 3;
const BODY_CHUNK_BYTES: usize = 64 * 1024;
const REPORT_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Clone)]
pub(super) struct ChunkProgress {
    pub sent: Arc<AtomicU64>,
    pub report_clock: Arc<Mutex<std::time::Instant>>,
    pub events: mpsc::Sender<ActivityEvent>,
    pub source_path: SourcePath,
    pub source_hashed_bytes: u64,
    pub source_chunked_bytes: u64,
    pub chunk_count: u64,
    pub planned_payload_bytes: u64,
    pub total_bytes: Option<u64>,
}

pub(super) async fn plan(
    transport: &HttpTransport,
    credential: &str,
    upload_id: &str,
    request: &UploadPlanRequest,
) -> Result<UploadPlanResponse, EngineError> {
    let path = format!("v1/uploads/{upload_id}/plan");
    for attempt in 1..=MAX_ATTEMPTS {
        match api::json::<UploadPlanResponse, _>(
            transport,
            Method::POST,
            &path,
            Some(credential),
            &[],
            Some(request),
        )
        .await
        {
            Err(error) if error.should_retry_request() && attempt < MAX_ATTEMPTS => {
                tokio::time::sleep(Duration::from_millis(100 * attempt as u64)).await;
            }
            result => return result,
        }
    }
    Err(EngineError::InvalidResponse(
        "bounded upload plan retries exhausted without a result",
    ))
}

pub(super) async fn chunk(
    transport: &HttpTransport,
    credential: &str,
    upload_id: &str,
    descriptor: ChunkDescriptor,
    bytes: Vec<u8>,
    progress: ChunkProgress,
) -> Result<(), EngineError> {
    if bytes.len() != usize::try_from(descriptor.length).unwrap_or(0) {
        return Err(EngineError::InvalidResponse(
            "reread chunk length differs from its descriptor",
        ));
    }
    let path = format!("v1/uploads/{upload_id}/chunks/{}", descriptor.ordinal);
    for attempt in 1..=MAX_ATTEMPTS {
        let body = body_stream(bytes.clone(), progress.clone());
        let request = transport
            .request(Method::PUT, &path)?
            .bearer_auth(credential)
            .header(CONTENT_TYPE, "application/octet-stream")
            .header(CONTENT_LENGTH, bytes.len().to_string())
            .body(reqwest::Body::wrap_stream(body))
            .build()
            .map_err(TransportError::Build)?;
        let result = match transport.send(request).await {
            Ok(response) => api::ensure_success(response).await,
            Err(error) => Err(error.into()),
        };
        match result {
            Err(error) if error.should_retry_request() && attempt < MAX_ATTEMPTS => {
                tokio::time::sleep(Duration::from_millis(100 * attempt as u64)).await;
            }
            result => return result,
        }
    }
    Err(EngineError::InvalidResponse(
        "bounded chunk retries exhausted without a result",
    ))
}

pub(super) fn body_stream(
    bytes: Vec<u8>,
    progress: ChunkProgress,
) -> impl futures_util::TryStream<Ok = Vec<u8>, Error = io::Error> + Send + 'static {
    stream::try_unfold((bytes, 0usize), move |(bytes, offset)| {
        let progress = progress.clone();
        async move {
            if offset == bytes.len() {
                return Ok(None);
            }
            let end = (offset + BODY_CHUNK_BYTES).min(bytes.len());
            let chunk = bytes[offset..end].to_vec();
            let chunk_len = chunk.len() as u64;
            let previous = progress
                .sent
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |total| {
                    total.checked_add(chunk_len)
                })
                .map_err(|_| io::Error::other("sent payload counter overflowed"))?;
            let total_sent = previous + chunk_len;
            let mut last_report = progress.report_clock.lock().await;
            if last_report.elapsed() >= REPORT_INTERVAL {
                *last_report = std::time::Instant::now();
                drop(last_report);
                let _ = progress
                    .events
                    .try_send(ActivityEvent::Progress(ProgressSnapshot {
                        phase: UploadPhase::Uploading,
                        source_path: progress.source_path.clone(),
                        processed_bytes: total_sent,
                        total_bytes: progress.total_bytes,
                        source_hashed_bytes: progress.source_hashed_bytes,
                        source_chunked_bytes: progress.source_chunked_bytes,
                        chunk_count: progress.chunk_count,
                        planned_payload_bytes: progress.planned_payload_bytes,
                        file_payload_sent: total_sent,
                    }));
            }
            Ok(Some((chunk, (bytes, end))))
        }
    })
}
