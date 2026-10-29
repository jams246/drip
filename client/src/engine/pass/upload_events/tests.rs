use super::*;
use crate::engine::{ProgressSnapshot, UploadPhase};
use tokio::sync::watch;

#[tokio::test]
async fn rapid_page_phase_changes_reach_lifecycle_and_keep_final_counters() {
    let (lifecycle, mut received) = mpsc::channel(16);
    let (progress, mut latest) = watch::channel(None);
    let (sender, report) = start(
        EventSink::new(lifecycle, progress),
        "selection".into(),
        "operation".into(),
    );
    let path = drip_core::SourcePath::new('C', vec!["file.bin".into()]).unwrap();
    for (phase, sent) in [
        (UploadPhase::Hashing, 0),
        (UploadPhase::Chunking, 0),
        (UploadPhase::Uploading, 1),
        (UploadPhase::Chunking, 1),
        (UploadPhase::Uploading, 2),
        (UploadPhase::Verifying, 2),
    ] {
        sender
            .send(ActivityEvent::Progress(ProgressSnapshot {
                phase,
                source_path: path.clone(),
                processed_bytes: sent,
                total_bytes: None,
                source_hashed_bytes: 10,
                source_chunked_bytes: 10,
                chunk_count: 2,
                planned_payload_bytes: 2,
                file_payload_sent: sent,
            }))
            .await
            .unwrap();
    }
    sender
        .send(ActivityEvent::UploadCompleted {
            source_path: path,
            size: 10,
            digest: drip_core::Blake3Digest::new(*blake3::hash(b"0123456789").as_bytes()),
        })
        .await
        .unwrap();
    drop(sender);
    report.await.unwrap().unwrap();

    let mut phases = Vec::new();
    let mut completed = 0;
    while let Ok(event) = received.try_recv() {
        match event.kind {
            EventKind::Hashing
            | EventKind::Chunking
            | EventKind::Uploading
            | EventKind::Verifying => phases.push(event.kind),
            EventKind::UploadCompleted => completed += 1,
            _ => {}
        }
    }
    assert_eq!(
        phases,
        [
            EventKind::Hashing,
            EventKind::Chunking,
            EventKind::Uploading,
            EventKind::Chunking,
            EventKind::Uploading,
            EventKind::Verifying,
        ]
    );
    assert_eq!(completed, 1);
    let snapshot = latest.borrow_and_update().clone().unwrap();
    assert_eq!(snapshot.file.phase, UploadPhase::Verifying);
    assert_eq!(snapshot.file.file_payload_sent, 2);
    assert_eq!(snapshot.file.planned_payload_bytes, 2);
}

#[tokio::test]
async fn control_retry_logs_failed_and_recovered_http_statuses() {
    let (lifecycle, mut received) = mpsc::channel(8);
    let (progress, _) = watch::channel(None);
    let (sender, report) = start(
        EventSink::new(lifecycle, progress),
        "selection".into(),
        "operation".into(),
    );
    let source_path = drip_core::SourcePath::new('C', vec!["file.bin".into()]).unwrap();
    for (attempt, status, recovered) in [(1, 503, false), (2, 200, true)] {
        sender
            .send(ActivityEvent::ControlRetry {
                source_path: source_path.clone(),
                attempt,
                max_attempts: 3,
                status: Some(status),
                recovered,
            })
            .await
            .unwrap();
    }
    drop(sender);
    report.await.unwrap().unwrap();

    let failed = received.try_recv().unwrap();
    let recovered = received.try_recv().unwrap();
    assert_eq!(failed.kind, EventKind::ControlRetry);
    assert_eq!(failed.source_path, Some(source_path.clone()));
    assert!(failed.message.contains("HTTP 503"));
    assert!(failed.message.contains("retry 1 of 3"));
    assert_eq!(recovered.kind, EventKind::ControlRetry);
    assert_eq!(recovered.source_path, Some(source_path));
    assert!(recovered.message.contains("HTTP 200"));
    assert!(recovered.message.contains("recovered on attempt 2 of 3"));
    assert!(received.try_recv().is_err());
}
