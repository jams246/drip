use std::time::{Duration, Instant};

use tokio::{sync::mpsc, task::JoinHandle};

use crate::engine::{
    ActivityEvent, EngineError, EventKind, EventSink, ProgressSnapshot, UploadPhase,
};

const REPORT_INTERVAL: Duration = Duration::from_millis(200);

pub(super) fn start(
    events: EventSink,
    selection: String,
    operation: String,
) -> (
    mpsc::Sender<ActivityEvent>,
    JoinHandle<Result<(), EngineError>>,
) {
    let (sender, mut receiver) = mpsc::channel(16);
    let report = tokio::spawn(async move {
        let mut phase = None;
        let mut pending = None;
        let mut last_report = None;
        let timer = tokio::time::sleep(REPORT_INTERVAL);
        tokio::pin!(timer);
        loop {
            tokio::select! {
                event = receiver.recv() => match event {
                    Some(ActivityEvent::Progress(progress)) => {
                        let now = Instant::now();
                        if phase != Some(progress.phase) {
                            phase = Some(progress.phase);
                            pending = None;
                            events.progress(&selection, &operation, progress.clone());
                            events.progress_phase(&selection, &operation, progress).await?;
                            last_report = Some(now);
                            timer
                                .as_mut()
                                .reset(tokio::time::Instant::now() + REPORT_INTERVAL);
                        } else if last_report.is_none_or(|last| now.duration_since(last) >= REPORT_INTERVAL) {
                            events.progress(&selection, &operation, progress);
                            last_report = Some(now);
                            timer
                                .as_mut()
                                .reset(tokio::time::Instant::now() + REPORT_INTERVAL);
                        } else {
                            pending = Some(progress);
                        }
                    }
                    Some(ActivityEvent::UploadStarted { source_path, size }) => {
                        events.emit(
                            EventKind::UploadStarted,
                            Some(&selection),
                            Some(&source_path),
                            &operation,
                            format!("uploading {size} bytes"),
                        ).await?;
                        if phase.is_none() {
                            let progress = initial(size, source_path);
                            phase = Some(progress.phase);
                            events.progress(&selection, &operation, progress.clone());
                            events.progress_phase(&selection, &operation, progress).await?;
                            last_report = Some(Instant::now());
                            timer
                                .as_mut()
                                .reset(tokio::time::Instant::now() + REPORT_INTERVAL);
                        }
                    }
                    Some(ActivityEvent::UploadCompleted { source_path, size, .. }) => {
                        events.emit(
                            EventKind::UploadCompleted,
                            Some(&selection),
                            Some(&source_path),
                            &operation,
                            format!("verified publication of {size} bytes"),
                        ).await?;
                    }
                    None => {
                        if let Some(progress) = pending.take() {
                            timer.as_mut().await;
                            events.progress(&selection, &operation, progress);
                        }
                        break;
                    }
                },
                _ = &mut timer, if pending.is_some() => {
                    if let Some(progress) = pending.take() {
                        events.progress(&selection, &operation, progress);
                        last_report = Some(Instant::now());
                        timer
                            .as_mut()
                            .reset(tokio::time::Instant::now() + REPORT_INTERVAL);
                    }
                }
            }
        }
        Ok(())
    });
    (sender, report)
}

fn initial(size: u64, source_path: drip_core::SourcePath) -> ProgressSnapshot {
    ProgressSnapshot {
        phase: UploadPhase::Hashing,
        source_path,
        processed_bytes: 0,
        total_bytes: Some(size),
        source_hashed_bytes: 0,
        source_chunked_bytes: 0,
        chunk_count: 0,
        planned_payload_bytes: 0,
        file_payload_sent: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
