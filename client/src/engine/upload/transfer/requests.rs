use std::{
    future::Future,
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
/// Chunk bodies are at most 1 MiB; whole-file streaming PUTs stay bounded by the read timeout.
const CHUNK_TIMEOUT: Duration = Duration::from_secs(600);

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
    source_path: &SourcePath,
    events: &mpsc::Sender<ActivityEvent>,
) -> Result<UploadPlanResponse, EngineError> {
    let path = format!("v1/uploads/{upload_id}/plan");
    for attempt in 1..=MAX_ATTEMPTS {
        #[cfg(test)]
        crate::engine::test_metrics::plan_request(source_path);
        match api::json_with_status::<UploadPlanResponse, _>(
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
                let _ = events
                    .send(control_retry(
                        source_path,
                        attempt,
                        retry_status(&error),
                        false,
                    ))
                    .await;
                tokio::time::sleep(Duration::from_millis(100 * attempt as u64)).await;
            }
            Ok((response, status)) => {
                if attempt > 1 {
                    let _ = events
                        .send(control_retry(
                            source_path,
                            attempt,
                            Some(status.as_u16()),
                            true,
                        ))
                        .await;
                }
                return Ok(response);
            }
            Err(error) => return Err(error),
        }
    }
    Err(EngineError::InvalidResponse(
        "bounded upload plan retries exhausted without a result",
    ))
}

fn retry_status(error: &EngineError) -> Option<u16> {
    match error {
        EngineError::Api { status, .. } => Some(status.as_u16()),
        _ => None,
    }
}

fn control_retry(
    source_path: &SourcePath,
    attempt: usize,
    status: Option<u16>,
    recovered: bool,
) -> ActivityEvent {
    ActivityEvent::ControlRetry {
        source_path: source_path.clone(),
        attempt,
        max_attempts: MAX_ATTEMPTS,
        status,
        recovered,
    }
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
            .timeout(CHUNK_TIMEOUT)
            .build()
            .map_err(TransportError::Build)?;
        let result = report_while_waiting(
            async {
                match transport.send(request).await {
                    Ok(response) => api::ensure_success(response).await,
                    Err(error) => Err(error.into()),
                }
            },
            &progress,
        )
        .await;
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
            #[cfg(test)]
            crate::engine::test_metrics::upload_payload(&progress.source_path, chunk.len());
            progress
                .sent
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |total| {
                    total.checked_add(chunk_len)
                })
                .map_err(|_| io::Error::other("sent payload counter overflowed"))?;
            progress.report().await;
            Ok(Some((chunk, (bytes, end))))
        }
    })
}

impl ChunkProgress {
    async fn report(&self) {
        let mut last_report = self.report_clock.lock().await;
        if last_report.elapsed() < REPORT_INTERVAL {
            return;
        }
        *last_report = std::time::Instant::now();
        drop(last_report);
        let sent = self.sent.load(Ordering::Relaxed);
        let _ = self
            .events
            .try_send(ActivityEvent::Progress(ProgressSnapshot {
                phase: UploadPhase::Uploading,
                source_path: self.source_path.clone(),
                processed_bytes: sent,
                total_bytes: self.total_bytes,
                source_hashed_bytes: self.source_hashed_bytes,
                source_chunked_bytes: self.source_chunked_bytes,
                chunk_count: self.chunk_count,
                planned_payload_bytes: self.planned_payload_bytes,
                file_payload_sent: sent,
            }));
    }
}

async fn report_while_waiting<F: Future>(pending: F, progress: &ChunkProgress) -> F::Output {
    tokio::pin!(pending);
    let mut ticks = tokio::time::interval_at(
        tokio::time::Instant::now() + REPORT_INTERVAL,
        REPORT_INTERVAL,
    );
    ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            result = &mut pending => return result,
            _ = ticks.tick() => progress.report().await,
        }
    }
}

#[cfg(test)]
#[path = "requests/tests.rs"]
mod tests;
