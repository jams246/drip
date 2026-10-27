use super::*;
use crate::engine::UploadPhase;

#[test]
fn source_processing_and_uploaded_bytes_remain_separate() {
    let (sender, _) = mpsc::channel(1);
    let (progress, current) = watch::channel(None);
    let sink = EventSink::new(sender, progress);
    let path = SourcePath::new('C', vec!["file.bin".into()]).unwrap();
    let hashing = ProgressSnapshot {
        phase: UploadPhase::Hashing,
        source_path: path.clone(),
        processed_bytes: 10,
        total_bytes: Some(10),
        source_hashed_bytes: 10,
        source_chunked_bytes: 0,
        chunk_count: 0,
        planned_payload_bytes: 0,
        file_payload_sent: 0,
    };
    sink.progress("selection", "first", hashing.clone());
    let mut uploading = hashing;
    uploading.phase = UploadPhase::Uploading;
    uploading.processed_bytes = 3;
    uploading.planned_payload_bytes = 5;
    uploading.file_payload_sent = 3;
    sink.progress("selection", "first", uploading);
    assert_eq!(
        current
            .borrow()
            .as_ref()
            .map(|value| (value.source_bytes_processed, value.uploaded_file_bytes)),
        Some((10, 3))
    );

    let next = ProgressSnapshot {
        phase: UploadPhase::Hashing,
        source_path: path,
        processed_bytes: 2,
        total_bytes: Some(10),
        source_hashed_bytes: 2,
        source_chunked_bytes: 0,
        chunk_count: 0,
        planned_payload_bytes: 0,
        file_payload_sent: 0,
    };
    sink.progress("selection", "next", next);
    assert_eq!(
        current
            .borrow()
            .as_ref()
            .map(|value| (value.source_bytes_processed, value.uploaded_file_bytes)),
        Some((2, 0))
    );
}

#[tokio::test]
async fn phase_changes_are_ordered_typed_events() {
    let (sender, mut receiver) = mpsc::channel(8);
    let (progress, _) = watch::channel(None);
    let sink = EventSink::new(sender, progress);
    let path = SourcePath::new('C', vec!["file.bin".into()]).unwrap();
    for phase in [
        UploadPhase::Hashing,
        UploadPhase::Chunking,
        UploadPhase::Uploading,
        UploadPhase::Verifying,
    ] {
        sink.progress_phase(
            "selection",
            "operation",
            ProgressSnapshot {
                phase,
                source_path: path.clone(),
                processed_bytes: 0,
                total_bytes: None,
                source_hashed_bytes: 10,
                source_chunked_bytes: 10,
                chunk_count: 1,
                planned_payload_bytes: 2,
                file_payload_sent: 2,
            },
        )
        .await
        .unwrap();
    }
    assert_eq!(
        (0..4)
            .map(|_| receiver.try_recv().unwrap().kind)
            .collect::<Vec<_>>(),
        [
            EventKind::Hashing,
            EventKind::Chunking,
            EventKind::Uploading,
            EventKind::Verifying,
        ]
    );
}

#[tokio::test]
async fn saturated_queue_waits_and_preserves_terminal_order() {
    let (sender, mut receiver) = mpsc::channel(1);
    let (progress, _) = watch::channel(None);
    let sink = EventSink::new(sender, progress);
    sink.emit(
        EventKind::Queued,
        Some("selection"),
        None,
        "operation",
        "queued",
    )
    .await
    .unwrap();
    let pending = tokio::spawn(async move {
        sink.emit(
            EventKind::UploadCompleted,
            Some("selection"),
            None,
            "operation",
            "verified publication",
        )
        .await
    });
    tokio::task::yield_now().await;
    assert!(!pending.is_finished());
    assert_eq!(receiver.recv().await.unwrap().kind, EventKind::Queued);
    pending.await.unwrap().unwrap();
    let terminal = receiver.recv().await.unwrap();
    assert_eq!(terminal.kind, EventKind::UploadCompleted);
    assert_eq!(terminal.operation_id, "operation");
}

#[tokio::test]
async fn closed_subscriber_does_not_block_engine() {
    let (sender, receiver) = mpsc::channel(1);
    drop(receiver);
    let (progress, _) = watch::channel(None);
    EventSink::new(sender, progress)
        .emit(EventKind::Paused, None, None, "pause", "paused")
        .await
        .unwrap();
}
