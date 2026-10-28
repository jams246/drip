use drip_client::{EventKind, SelectionProgress, SyncSnapshot};

use super::{LogRow, UiState};

impl UiState {
    pub(in crate::gui) fn progress(
        &mut self,
        generation: u64,
        progress: Option<SelectionProgress>,
    ) {
        if generation != self.worker_generation {
            return;
        }
        if progress
            .as_ref()
            .is_some_and(|progress| self.finished_operations.contains(&progress.operation_id))
        {
            return;
        }
        self.progress = progress;
    }

    pub(in crate::gui) fn status(&mut self, generation: u64, status: SyncSnapshot) {
        if generation == self.worker_generation {
            self.status = status;
        }
    }

    pub(in crate::gui) fn activity(&mut self, row: LogRow) {
        let event = &row.event;
        if matches!(
            event.kind,
            EventKind::UploadCompleted
                | EventKind::FileChecked
                | EventKind::Cancelled
                | EventKind::Error
                | EventKind::Retry
        ) {
            if self
                .progress
                .as_ref()
                .is_some_and(|progress| progress.operation_id == event.operation_id)
            {
                self.progress = None;
            }
            if !self.finished_operations.contains(&event.operation_id) {
                if self.finished_operations.len() == 1000 {
                    self.finished_operations.pop_front();
                }
                self.finished_operations
                    .push_back(event.operation_id.clone());
            }
            if let Some(selection_id) = &event.selection_id
                && event.source_path.is_some()
            {
                self.last_file.insert(selection_id.clone(), event.clone());
            }
        }
        if self.logs.len() == 1000 {
            self.logs.pop_front();
        }
        self.logs.push_back(row);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use drip_client::{LifecycleEvent, ProgressSnapshot, UploadPhase};

    fn progress() -> SelectionProgress {
        SelectionProgress {
            selection_id: "root".into(),
            operation_id: "upload".into(),
            source_bytes_processed: 10,
            uploaded_file_bytes: 3,
            file: ProgressSnapshot {
                phase: UploadPhase::Verifying,
                source_path: drip_core::SourcePath::new('C', vec!["file.bin".into()]).unwrap(),
                processed_bytes: 3,
                total_bytes: Some(3),
                source_hashed_bytes: 10,
                source_chunked_bytes: 10,
                chunk_count: 1,
                planned_payload_bytes: 3,
                file_payload_sent: 3,
            },
        }
    }

    fn completed() -> LogRow {
        LogRow {
            local_time: "2026-10-27 00:00:00 +00:00".into(),
            event: LifecycleEvent {
                timestamp: drip_core::Timestamp::new(1, 0).unwrap(),
                kind: EventKind::UploadCompleted,
                selection_id: Some("root".into()),
                source_path: Some(progress().file.source_path),
                operation_id: "upload".into(),
                message: "verified publication".into(),
                progress: None,
            },
        }
    }

    #[test]
    fn late_progress_cannot_resurrect_a_verified_upload() {
        let mut state = UiState {
            worker_generation: 1,
            ..UiState::default()
        };
        state.progress(1, Some(progress()));
        state.activity(completed());
        state.progress(1, Some(progress()));
        assert!(state.progress.is_none());
        assert_eq!(state.last_file["root"].kind, EventKind::UploadCompleted);
        assert_eq!(state.logs.len(), 1);
    }

    #[test]
    fn checked_file_rejects_late_hash_ticks_without_clearing_another_root() {
        let mut state = UiState {
            worker_generation: 1,
            ..UiState::default()
        };
        let mut next = progress();
        next.selection_id = "other-root".into();
        next.operation_id = "next-file".into();
        state.progress(1, Some(next));
        let mut checked = completed();
        checked.event.kind = EventKind::FileChecked;
        state.activity(checked);
        state.progress(1, Some(progress()));
        assert_eq!(state.progress.as_ref().unwrap().operation_id, "next-file");
        assert_eq!(state.last_file["root"].kind, EventKind::FileChecked);
    }

    #[test]
    fn late_progress_cannot_resurrect_a_cancelled_upload() {
        let mut state = UiState {
            worker_generation: 1,
            ..UiState::default()
        };
        state.progress(1, Some(progress()));
        let mut cancelled = completed();
        cancelled.event.kind = EventKind::Cancelled;
        cancelled.event.message = "sync operation cancelled by a control request".into();
        state.activity(cancelled);
        state.progress(1, Some(progress()));
        assert!(state.progress.is_none());
        assert_eq!(state.last_file["root"].kind, EventKind::Cancelled);
    }

    #[test]
    fn control_retry_keeps_same_upload_open_for_next_progress_tick() {
        let mut state = UiState {
            worker_generation: 1,
            ..UiState::default()
        };
        state.progress(1, Some(progress()));
        let mut retry = completed();
        retry.event.kind = EventKind::ControlRetry;
        retry.event.message = "HTTP 503; upload plan retry 1 of 3".into();

        state.activity(retry);

        assert_eq!(state.progress, Some(progress()));
        assert!(state.finished_operations.is_empty());
        assert!(state.last_file.is_empty());
        let mut next = progress();
        next.file.phase = UploadPhase::Uploading;
        next.file.file_payload_sent = 2;
        next.uploaded_file_bytes = 2;
        state.progress(1, Some(next.clone()));
        assert_eq!(state.progress, Some(next));
        assert!(state.finished_operations.is_empty());
    }

    #[test]
    fn prior_pairing_worker_cannot_override_current_progress_or_status() {
        let mut state = UiState {
            worker_generation: 2,
            ..UiState::default()
        };
        state.progress(1, Some(progress()));
        let mut stale = SyncSnapshot::default();
        stale.connection = drip_client::ConnectionState::Paused;
        state.status(1, stale);
        assert!(state.progress.is_none());
        assert_eq!(
            state.status.connection,
            drip_client::ConnectionState::Disconnected
        );
    }

    #[test]
    fn recent_logs_and_terminal_correlations_remain_bounded() {
        let mut state = UiState::default();
        for index in 0..1100 {
            let mut row = completed();
            row.event.operation_id = format!("upload-{index}");
            state.activity(row);
        }
        assert_eq!(state.logs.len(), 1000);
        assert_eq!(state.finished_operations.len(), 1000);
        assert_eq!(state.logs.front().unwrap().event.operation_id, "upload-100");
    }
}
