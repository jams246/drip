use drip_client::{ConnectionState, EventKind, RootStatus, SelectionProgress, SelectionState};

use super::super::progress;
use crate::gui::{theme, views::components::Glyph};

pub(in crate::gui::views::home) fn visible_progress(
    connection: ConnectionState,
    pause_requested: bool,
    stopping: bool,
    progress: Option<&SelectionProgress>,
    selection_id: &str,
    root_status: Option<&RootStatus>,
) -> bool {
    if stopping
        || pause_requested
        || connection != ConnectionState::Connected
        || matches!(
            root_status.map(|root| root.state),
            Some(
                SelectionState::Paused
                    | SelectionState::Offline
                    | SelectionState::UnavailableRoot
                    | SelectionState::Cancelled
                    | SelectionState::Error
                    | SelectionState::Synced
            )
        )
    {
        return false;
    }
    progress.is_some_and(|progress| progress.selection_id == selection_id)
}

pub(super) fn label(
    connection: ConnectionState,
    pause_requested: bool,
    stopping: bool,
    status: Option<&RootStatus>,
    current: Option<&SelectionProgress>,
    last_file: Option<&drip_client::LifecycleEvent>,
) -> &'static str {
    if stopping {
        return "Stopped";
    }
    if pause_requested && connection != ConnectionState::Paused {
        return "Pausing...";
    }
    match status.map(|root| root.state) {
        Some(SelectionState::Paused) => return "Paused",
        Some(SelectionState::Offline) => return "Waiting for connection",
        Some(SelectionState::UnavailableRoot) => return "File or folder unavailable",
        Some(SelectionState::Cancelled) => return "Stopped",
        Some(SelectionState::Error) => return "Needs attention",
        Some(SelectionState::Synced) => return "Up to date",
        _ => {}
    }
    match connection {
        ConnectionState::Pausing => return "Pausing...",
        ConnectionState::Paused => return "Paused",
        ConnectionState::Disconnected => return "Waiting for connection",
        ConnectionState::Connected => {}
    }
    if let Some(current) = current {
        return progress::stage(current.file.phase).label;
    }
    match status.map(|root| root.state) {
        Some(SelectionState::Scanning) => return "Checking for changes",
        Some(SelectionState::Uploading)
            if last_file.is_some_and(|event| event.kind == EventKind::UploadCompleted) =>
        {
            return "Finishing sync";
        }
        Some(SelectionState::Uploading) => return "Sending changes",
        _ => {}
    }
    "Waiting"
}

pub(super) fn icon(label: &str) -> (Glyph, u32) {
    match label {
        "Up to date" => (Glyph::Check, theme::SUCCESS),
        "Needs attention" | "File or folder unavailable" => (Glyph::Attention, theme::ERROR),
        "Paused" | "Pausing..." => (Glyph::Clock, theme::WARNING),
        _ => (Glyph::Clock, theme::MUTED),
    }
}

pub(super) fn last_file_label(kind: EventKind) -> Option<&'static str> {
    match kind {
        EventKind::UploadCompleted => Some("Last file uploaded"),
        EventKind::FileChecked => Some("Last file checked"),
        EventKind::Cancelled => Some("Last file stopped"),
        EventKind::Retry | EventKind::Error => Some("Needs attention"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
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
}
