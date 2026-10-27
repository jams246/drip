use std::{
    io::Read,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use drip_core::{
    Blake3Digest, CHUNK_PLAN_MAX_DESCRIPTORS, ChunkDescriptor, ChunkStream, SourcePath,
};
use tokio::sync::mpsc;

use crate::{
    engine::{ActivityEvent, ProgressSnapshot, UploadPhase},
    source::SourceError,
};

const REPORT_INTERVAL: Duration = Duration::from_millis(200);

pub(super) struct ChunkPage<R: Read> {
    pub stream: Option<ChunkStream<R>>,
    pub descriptors: Vec<ChunkDescriptor>,
    pub source_chunked_bytes: u64,
    pub chunk_count: u64,
    pub digest: Option<Blake3Digest>,
}

pub(super) struct ChunkingContext {
    pub source_hashed_bytes: u64,
    pub planned_payload_bytes: u64,
    pub file_payload_sent: u64,
    pub source_path: SourcePath,
    pub events: mpsc::Sender<ActivityEvent>,
    pub cancelled: Arc<AtomicBool>,
}

pub(super) fn next_page<R: Read>(
    mut stream: ChunkStream<R>,
    expected_size: u64,
    mut source_chunked_bytes: u64,
    mut chunk_count: u64,
    context: ChunkingContext,
) -> Result<ChunkPage<R>, SourceError> {
    let mut descriptors = Vec::with_capacity(CHUNK_PLAN_MAX_DESCRIPTORS);
    let mut last_report = std::time::Instant::now();
    loop {
        if context.cancelled.load(Ordering::Acquire) {
            return Err(SourceError::Changed);
        }
        let chunk = stream.next_chunk().map_err(|_| SourceError::Changed)?;
        let Some(chunk) = chunk else {
            if expected_size != 0 || source_chunked_bytes != 0 || !descriptors.is_empty() {
                return Err(SourceError::Changed);
            }
            let digest = stream.finish().map_err(|_| SourceError::Changed)?;
            return Ok(ChunkPage {
                stream: None,
                descriptors,
                source_chunked_bytes,
                chunk_count,
                digest: Some(digest),
            });
        };
        let end = chunk
            .descriptor
            .offset
            .checked_add(u64::from(chunk.descriptor.length))
            .ok_or(SourceError::Changed)?;
        if end > expected_size {
            return Err(SourceError::Changed);
        }
        source_chunked_bytes = source_chunked_bytes
            .checked_add(u64::from(chunk.descriptor.length))
            .ok_or(SourceError::Changed)?;
        chunk_count = chunk_count.checked_add(1).ok_or(SourceError::Changed)?;
        descriptors.push(chunk.descriptor);
        drop(chunk);
        if !context.cancelled.load(Ordering::Acquire) && last_report.elapsed() >= REPORT_INTERVAL {
            let _ = context
                .events
                .try_send(ActivityEvent::Progress(ProgressSnapshot {
                    phase: UploadPhase::Chunking,
                    source_path: context.source_path.clone(),
                    processed_bytes: source_chunked_bytes,
                    total_bytes: Some(expected_size),
                    source_hashed_bytes: context.source_hashed_bytes,
                    source_chunked_bytes,
                    chunk_count,
                    planned_payload_bytes: context.planned_payload_bytes,
                    file_payload_sent: context.file_payload_sent,
                }));
            last_report = std::time::Instant::now();
        }
        if context.cancelled.load(Ordering::Acquire) {
            return Err(SourceError::Changed);
        }

        if end == expected_size {
            if stream
                .next_chunk()
                .map_err(|_| SourceError::Changed)?
                .is_some()
            {
                return Err(SourceError::Changed);
            }
            let digest = stream.finish().map_err(|_| SourceError::Changed)?;
            return Ok(ChunkPage {
                stream: None,
                descriptors,
                source_chunked_bytes,
                chunk_count,
                digest: Some(digest),
            });
        }

        if descriptors.len() == CHUNK_PLAN_MAX_DESCRIPTORS {
            return Ok(ChunkPage {
                stream: Some(stream),
                descriptors,
                source_chunked_bytes,
                chunk_count,
                digest: None,
            });
        }
    }
}

#[cfg(test)]
#[path = "chunking_tests.rs"]
mod tests;
