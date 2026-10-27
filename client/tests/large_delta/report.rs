use std::collections::BTreeSet;

use drip_client::{ActivityEvent, ProgressSnapshot};
use tokio::sync::mpsc;

pub async fn collect(
    mut receiver: mpsc::Receiver<ActivityEvent>,
) -> (u64, BTreeSet<String>, ProgressSnapshot, u64) {
    let mut sent = 0;
    let mut phases = BTreeSet::new();
    let mut latest: Option<ProgressSnapshot> = None;
    let mut completed = 0;
    while let Some(event) = receiver.recv().await {
        match event {
            ActivityEvent::Progress(snapshot) => {
                if let Some(previous) = &latest {
                    assert!(snapshot.source_hashed_bytes >= previous.source_hashed_bytes);
                    assert!(snapshot.source_chunked_bytes >= previous.source_chunked_bytes);
                    assert!(snapshot.chunk_count >= previous.chunk_count);
                    assert!(snapshot.planned_payload_bytes >= previous.planned_payload_bytes);
                    assert!(snapshot.file_payload_sent >= previous.file_payload_sent);
                }
                phases.insert(format!("{:?}", snapshot.phase));
                sent = sent.max(snapshot.file_payload_sent);
                latest = Some(snapshot);
            }
            ActivityEvent::UploadCompleted { .. } => completed += 1,
            _ => {}
        }
    }
    (sent, phases, latest.unwrap(), completed)
}

#[tokio::test]
async fn fast_early_upload_is_retained_across_later_zero_missing_pages() {
    use drip_client::UploadPhase;
    use drip_core::{Blake3Digest, CHUNK_MIN_SIZE, CHUNK_PLAN_MAX_DESCRIPTORS, SourcePath};

    let page_chunks = CHUNK_PLAN_MAX_DESCRIPTORS as u64;
    let page_bytes = page_chunks * u64::from(CHUNK_MIN_SIZE);
    let size = 3 * page_bytes;
    let payload = u64::from(CHUNK_MIN_SIZE);
    let path = SourcePath::new('C', vec!["multi-page.bin".into()]).unwrap();
    let (sender, receiver) = mpsc::channel(16);
    let reporting = tokio::spawn(collect(receiver));
    // The first page uploads before a counter tick; later pages need no bytes.
    for (phase, chunked, chunks, planned, sent) in [
        (UploadPhase::Hashing, 0, 0, 0, 0),
        (UploadPhase::Chunking, 0, 0, 0, 0),
        (UploadPhase::Uploading, page_bytes, page_chunks, payload, 0),
        (
            UploadPhase::Chunking,
            page_bytes,
            page_chunks,
            payload,
            payload,
        ),
        (
            UploadPhase::Chunking,
            2 * page_bytes,
            2 * page_chunks,
            payload,
            payload,
        ),
        (
            UploadPhase::Verifying,
            size,
            3 * page_chunks,
            payload,
            payload,
        ),
    ] {
        sender
            .send(ActivityEvent::Progress(ProgressSnapshot {
                phase,
                source_path: path.clone(),
                processed_bytes: match phase {
                    UploadPhase::Hashing => size,
                    UploadPhase::Chunking => chunked,
                    UploadPhase::Uploading => sent,
                    UploadPhase::Verifying => 0,
                },
                total_bytes: None,
                source_hashed_bytes: size,
                source_chunked_bytes: chunked,
                chunk_count: chunks,
                planned_payload_bytes: planned,
                file_payload_sent: sent,
            }))
            .await
            .unwrap();
    }
    sender
        .send(ActivityEvent::UploadCompleted {
            source_path: path,
            size,
            digest: Blake3Digest::new(*blake3::hash(b"synthetic event fixture").as_bytes()),
        })
        .await
        .unwrap();
    drop(sender);
    let (sent, phases, latest, completed) = reporting.await.unwrap();
    assert_eq!(sent, payload);
    assert_eq!(latest.file_payload_sent, payload);
    assert_eq!(latest.planned_payload_bytes, payload);
    assert_eq!(latest.source_chunked_bytes, size);
    assert_eq!(latest.chunk_count, 3 * page_chunks);
    assert_eq!(completed, 1);
    for phase in ["Hashing", "Chunking", "Uploading", "Verifying"] {
        assert!(phases.contains(phase));
    }
}
