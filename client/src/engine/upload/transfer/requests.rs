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
mod tests {
    use super::*;

    #[tokio::test]
    async fn pending_response_reports_the_latest_sent_bytes() {
        let (events, mut received) = mpsc::channel(2);
        let progress = ChunkProgress {
            sent: Arc::new(AtomicU64::new(42)),
            report_clock: Arc::new(Mutex::new(std::time::Instant::now())),
            events,
            source_path: SourcePath::new('C', vec!["file.bin".into()]).unwrap(),
            source_hashed_bytes: 42,
            source_chunked_bytes: 42,
            chunk_count: 1,
            planned_payload_bytes: 42,
            total_bytes: None,
        };
        let waiting = tokio::spawn(async move {
            report_while_waiting(std::future::pending::<()>(), &progress).await;
        });
        let event = tokio::time::timeout(Duration::from_secs(1), received.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(
            matches!(event, ActivityEvent::Progress(snapshot) if snapshot.file_payload_sent == 42)
        );
        waiting.abort();
    }
}
