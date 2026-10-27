use drip_core::{ChunkDescriptor, SourcePath};
use tokio::sync::mpsc;

use crate::engine::{ActivityEvent, EngineError, ProgressSnapshot, UploadPhase};

use super::Counters;

pub(super) fn missing_indices(
    descriptors: &[ChunkDescriptor],
    missing_ordinals: &[u64],
) -> Result<Vec<usize>, EngineError> {
    let first_ordinal = descriptors.first().map_or(0, |item| item.ordinal);
    let mut seen = vec![false; descriptors.len()];
    let mut indices = Vec::with_capacity(missing_ordinals.len());
    for ordinal in missing_ordinals {
        let index = ordinal
            .checked_sub(first_ordinal)
            .and_then(|index| usize::try_from(index).ok())
            .filter(|index| *index < descriptors.len())
            .ok_or(EngineError::InvalidResponse(
                "server requested an undeclared chunk ordinal",
            ))?;
        if seen[index] {
            return Err(EngineError::InvalidResponse(
                "server repeated a missing chunk ordinal",
            ));
        }
        seen[index] = true;
        indices.push(index);
    }
    Ok(indices)
}

pub(super) fn snapshot(
    phase: UploadPhase,
    source_path: &SourcePath,
    size: u64,
    counters: Counters,
    sent: u64,
    total_bytes: Option<u64>,
) -> ProgressSnapshot {
    ProgressSnapshot {
        phase,
        source_path: source_path.clone(),
        processed_bytes: match phase {
            UploadPhase::Hashing => counters.source_hashed,
            UploadPhase::Chunking => counters.source_chunked,
            UploadPhase::Uploading => sent,
            UploadPhase::Verifying => 0,
        },
        total_bytes: match phase {
            UploadPhase::Hashing | UploadPhase::Chunking => Some(size),
            UploadPhase::Uploading | UploadPhase::Verifying => total_bytes,
        },
        source_hashed_bytes: counters.source_hashed,
        source_chunked_bytes: counters.source_chunked,
        chunk_count: counters.chunk_count,
        planned_payload_bytes: counters.planned,
        file_payload_sent: sent,
    }
}

pub(super) async fn emit(events: &mpsc::Sender<ActivityEvent>, progress: ProgressSnapshot) {
    let _ = events.send(ActivityEvent::Progress(progress)).await;
}
