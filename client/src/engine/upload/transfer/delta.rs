use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::time::Instant;

use drip_core::protocol::UploadPlanRequest;
use drip_core::{Blake3Digest, ChunkConfig, ChunkStream};
use tokio::sync::{Mutex, mpsc};

use crate::{
    engine::{ActivityEvent, EngineError, UploadPhase, selection},
    source::SourceFile,
    transport::HttpTransport,
};

use super::{
    super::chunking,
    requests::{self, ChunkProgress},
};
use crate::engine::selection::ActiveSelection;

mod progress;

use progress::{emit, missing_indices, snapshot};

#[derive(Clone, Copy, Default)]
struct Counters {
    source_hashed: u64,
    source_chunked: u64,
    chunk_count: u64,
    planned: u64,
}

struct CancelChunking(Arc<AtomicBool>);

impl Drop for CancelChunking {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

pub(super) async fn send(
    transport: &HttpTransport,
    credential: &str,
    active: &ActiveSelection,
    source: &SourceFile,
    upload_id: &str,
    digest: Blake3Digest,
    events: &mpsc::Sender<ActivityEvent>,
) -> Result<(), EngineError> {
    let size = source.size();
    let source_path = source.protocol_path().clone();
    let mut counters = Counters {
        source_hashed: size,
        ..Counters::default()
    };
    let sent = Arc::new(AtomicU64::new(0));
    let report_clock = Arc::new(Mutex::new(Instant::now()));
    let cancelled = Arc::new(AtomicBool::new(false));
    let _cancel_chunking = CancelChunking(Arc::clone(&cancelled));
    source.verify().await?;
    let mut stream = Some(ChunkStream::new(source.chunk_reader()?));
    loop {
        emit(
            events,
            snapshot(
                UploadPhase::Chunking,
                &source_path,
                size,
                counters,
                sent.load(Ordering::Relaxed),
                None,
            ),
        )
        .await;
        selection::require_current(transport, credential, active).await?;
        let current = stream.take().ok_or(EngineError::InvalidResponse(
            "chunk stream ended before the complete file",
        ))?;
        let event_sender = events.clone();
        let path = source_path.clone();
        let sent_counter = Arc::clone(&sent);
        let cancellation = Arc::clone(&cancelled);
        let page = tokio::task::spawn_blocking(move || {
            chunking::next_page(
                current,
                size,
                counters.source_chunked,
                counters.chunk_count,
                chunking::ChunkingContext {
                    source_hashed_bytes: counters.source_hashed,
                    planned_payload_bytes: counters.planned,
                    file_payload_sent: sent_counter.load(Ordering::Relaxed),
                    source_path: path,
                    events: event_sender,
                    cancelled: cancellation,
                },
            )
        })
        .await
        .map_err(|error| std::io::Error::other(error.to_string()))??;
        counters.source_chunked = page.source_chunked_bytes;
        counters.chunk_count = page.chunk_count;
        let final_page = page.digest.is_some();
        if page.digest.is_some_and(|actual| actual != digest) {
            return Err(crate::source::SourceError::Changed.into());
        }
        selection::require_current(transport, credential, active).await?;
        let request = UploadPlanRequest {
            config: ChunkConfig::V1,
            descriptors: page.descriptors.clone(),
        };
        let response = requests::plan(transport, credential, upload_id, &request).await?;
        let missing = missing_indices(&page.descriptors, &response.missing_ordinals)?;
        let missing_bytes = missing.iter().try_fold(0u64, |total, index| {
            total.checked_add(u64::from(page.descriptors[*index].length))
        });
        counters.planned = counters
            .planned
            .checked_add(missing_bytes.ok_or(EngineError::InvalidResponse(
                "missing payload size overflowed",
            ))?)
            .ok_or(EngineError::InvalidResponse(
                "planned payload size overflowed",
            ))?;
        let uploaded_page = !missing.is_empty();
        if uploaded_page {
            *report_clock.lock().await = Instant::now();
            emit(
                events,
                snapshot(
                    UploadPhase::Uploading,
                    &source_path,
                    size,
                    counters,
                    sent.load(Ordering::Relaxed),
                    None,
                ),
            )
            .await;
        }
        for index in missing {
            let descriptor = page.descriptors[index];
            let bytes = source
                .read_range(descriptor.offset, descriptor.length)
                .await?;
            if Blake3Digest::new(*blake3::hash(&bytes).as_bytes()) != descriptor.digest {
                return Err(crate::source::SourceError::Changed.into());
            }
            let progress = ChunkProgress {
                sent: Arc::clone(&sent),
                report_clock: Arc::clone(&report_clock),
                events: events.clone(),
                source_path: source_path.clone(),
                source_hashed_bytes: counters.source_hashed,
                source_chunked_bytes: counters.source_chunked,
                chunk_count: counters.chunk_count,
                planned_payload_bytes: counters.planned,
                total_bytes: None,
            };
            requests::chunk(
                transport, credential, upload_id, descriptor, bytes, progress,
            )
            .await?;
        }
        let sent_bytes = sent.load(Ordering::Relaxed);
        if final_page {
            if counters.source_chunked != size {
                return Err(crate::source::SourceError::Changed.into());
            }
            if uploaded_page {
                emit(
                    events,
                    snapshot(
                        UploadPhase::Uploading,
                        &source_path,
                        size,
                        counters,
                        sent_bytes,
                        Some(sent_bytes),
                    ),
                )
                .await;
            }
            break;
        }
        stream = page.stream;
        if stream.is_none() {
            return Err(EngineError::InvalidResponse(
                "non-final chunk page lost its stream",
            ));
        }
    }

    selection::require_current(transport, credential, active).await?;
    source.verify().await?;
    emit(
        events,
        snapshot(
            UploadPhase::Verifying,
            &source_path,
            size,
            counters,
            sent.load(Ordering::Relaxed),
            None,
        ),
    )
    .await;
    Ok(())
}
