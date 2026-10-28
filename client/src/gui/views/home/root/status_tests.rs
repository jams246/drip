use super::*;
use drip_client::UploadPhase;

fn progress() -> SelectionProgress {
    SelectionProgress {
        selection_id: "root".into(),
        operation_id: "upload".into(),
        source_bytes_processed: 10,
        uploaded_file_bytes: 0,
        file: drip_client::ProgressSnapshot {
            phase: UploadPhase::Uploading,
            source_path: drip_core::SourcePath::new('C', vec!["file".into()]).unwrap(),
            processed_bytes: 1,
            total_bytes: None,
            source_hashed_bytes: 10,
            source_chunked_bytes: 10,
            chunk_count: 1,
            planned_payload_bytes: 10,
            file_payload_sent: 0,
        },
    }
}

#[test]
fn acknowledged_pause_and_offline_override_a_previously_synced_root() {
    let progress = progress();
    let mut status = RootStatus {
        selection_id: "root".into(),
        source_path: progress.file.source_path.clone(),
        state: SelectionState::Synced,
        message: "previous pass completed".into(),
    };
    for (connection, expected) in [
        (ConnectionState::Disconnected, "Waiting for connection"),
        (ConnectionState::Paused, "Paused"),
        (ConnectionState::Connected, "Up to date"),
    ] {
        assert_eq!(
            label(connection, false, false, Some(&status), None, None),
            expected
        );
    }
    status.state = SelectionState::Paused;
    assert_eq!(
        label(
            ConnectionState::Disconnected,
            false,
            false,
            Some(&status),
            None,
            None
        ),
        "Waiting for connection"
    );
}

#[test]
fn complete_stage_is_not_file_or_root_completion() {
    let mut progress = progress();
    progress.file.total_bytes = Some(10);
    progress.file.processed_bytes = 10;
    let mut status = RootStatus {
        selection_id: "root".into(),
        source_path: progress.file.source_path.clone(),
        state: SelectionState::Uploading,
        message: String::new(),
    };
    assert_eq!(
        label(
            ConnectionState::Connected,
            false,
            false,
            Some(&status),
            Some(&progress),
            None
        ),
        "Sending changes"
    );
    for kind in [EventKind::UploadCompleted, EventKind::FileChecked] {
        let event = drip_client::LifecycleEvent {
            timestamp: drip_core::Timestamp::new(1, 0).unwrap(),
            kind,
            selection_id: Some("root".into()),
            source_path: Some(progress.file.source_path.clone()),
            operation_id: "upload".into(),
            message: "confirmed".into(),
            progress: None,
        };
        assert_eq!(
            label(
                ConnectionState::Connected,
                false,
                false,
                Some(&status),
                None,
                Some(&event)
            ),
            "Finishing sync"
        );
    }
    status.state = SelectionState::Synced;
    assert_eq!(
        label(
            ConnectionState::Connected,
            false,
            false,
            Some(&status),
            None,
            None
        ),
        "Up to date"
    );
}

#[test]
fn terminal_and_connection_states_override_stale_progress() {
    let progress = progress();
    for state in [
        SelectionState::Paused,
        SelectionState::Offline,
        SelectionState::UnavailableRoot,
        SelectionState::Cancelled,
        SelectionState::Error,
        SelectionState::Synced,
    ] {
        let status = RootStatus {
            selection_id: "root".into(),
            source_path: progress.file.source_path.clone(),
            state,
            message: "raw backend detail".into(),
        };
        assert!(!visible_progress(
            ConnectionState::Connected,
            false,
            false,
            Some(&progress),
            "root",
            Some(&status)
        ));
    }
    assert!(!visible_progress(
        ConnectionState::Disconnected,
        false,
        false,
        Some(&progress),
        "root",
        None
    ));
    assert!(!visible_progress(
        ConnectionState::Connected,
        true,
        false,
        Some(&progress),
        "root",
        None
    ));
    assert_eq!(
        label(
            ConnectionState::Disconnected,
            false,
            false,
            None,
            Some(&progress),
            None
        ),
        "Waiting for connection"
    );
    assert_eq!(
        label(
            ConnectionState::Pausing,
            true,
            false,
            None,
            Some(&progress),
            None
        ),
        "Pausing..."
    );
}

#[test]
fn retrying_upload_does_not_report_success_or_reuse_old_progress() {
    let progress = progress();
    let event = drip_client::LifecycleEvent {
        timestamp: drip_core::Timestamp::new(1, 0).unwrap(),
        kind: EventKind::Retry,
        selection_id: Some("root".into()),
        source_path: None,
        operation_id: "upload".into(),
        message: "retryable failure".into(),
        progress: None,
    };
    let error = RootStatus {
        selection_id: "root".into(),
        source_path: progress.file.source_path.clone(),
        state: SelectionState::Error,
        message: event.message.clone(),
    };
    assert_eq!(
        label(
            ConnectionState::Connected,
            false,
            false,
            Some(&error),
            None,
            Some(&event)
        ),
        "Needs attention"
    );
    let recovering = RootStatus {
        state: SelectionState::Uploading,
        ..error
    };
    assert_eq!(
        label(
            ConnectionState::Connected,
            false,
            false,
            Some(&recovering),
            None,
            Some(&event)
        ),
        "Sending changes"
    );
    assert_ne!(
        label(
            ConnectionState::Connected,
            false,
            false,
            Some(&recovering),
            None,
            Some(&event)
        ),
        "Up to date"
    );
}
