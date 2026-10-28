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
    assert!(matches!(event, ActivityEvent::Progress(snapshot) if snapshot.file_payload_sent == 42));
    waiting.abort();
}

#[test]
fn retry_event_exposes_status_without_server_error_text() {
    let error = EngineError::Api {
        status: reqwest::StatusCode::SERVICE_UNAVAILABLE,
        code: drip_core::protocol::ApiErrorCode::RetryableIo,
        message: "authorization=synthetic-secret".into(),
        retryable: true,
    };
    let path = SourcePath::new('C', vec!["file.bin".into()]).unwrap();
    let event = control_retry(&path, 1, retry_status(&error), false);

    assert!(matches!(
        &event,
        ActivityEvent::ControlRetry {
            attempt: 1,
            max_attempts: MAX_ATTEMPTS,
            status: Some(503),
            recovered: false,
            ..
        }
    ));
    assert!(!format!("{event:?}").contains("synthetic-secret"));
}
