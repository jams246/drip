use std::time::{SystemTime, UNIX_EPOCH};

use drip_core::{SourcePath, Timestamp};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, watch};

use super::{EngineError, ProgressSnapshot};

/// Ordered activity consumed by the diagnostic CLI and future GUI.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    SourceChanged,
    Queued,
    Scanning,
    UploadStarted,
    UploadCompleted,
    Hashing,
    Chunking,
    Uploading,
    Verifying,
    DirectoryCreated,
    Deleted,
    Retry,
    Error,
    SelectionChanged,
    Pausing,
    Paused,
    Resumed,
    Connected,
    Disconnected,
    Synced,
    UnavailableRoot,
}

/// One lifecycle record, never containing credentials or file contents.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LifecycleEvent {
    pub timestamp: Timestamp,
    pub kind: EventKind,
    pub selection_id: Option<String>,
    pub source_path: Option<SourcePath>,
    pub operation_id: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<ProgressSnapshot>,
}

/// Coalesced progress, separate from ordered terminal activity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectionProgress {
    pub selection_id: String,
    pub operation_id: String,
    pub file: ProgressSnapshot,
    /// Source bytes hashed for this operation, independent of bytes sent.
    pub source_bytes_processed: u64,
    /// Missing chunk payload bytes sent for this operation, excluding local work.
    pub uploaded_file_bytes: u64,
}

/// Bounded lifecycle delivery with independent replaceable progress.
#[derive(Clone)]
pub struct EventSink {
    events: mpsc::Sender<LifecycleEvent>,
    progress: watch::Sender<Option<SelectionProgress>>,
    log: Option<std::sync::Arc<super::logging::ActivityLog>>,
    status: watch::Sender<super::status::SyncSnapshot>,
}

impl EventSink {
    pub fn new(
        events: mpsc::Sender<LifecycleEvent>,
        progress: watch::Sender<Option<SelectionProgress>>,
    ) -> Self {
        let (status, _) = watch::channel(super::status::SyncSnapshot::default());
        Self {
            events,
            progress,
            log: None,
            status,
        }
    }

    pub(super) fn with_log(mut self, directory: &std::path::Path) -> Self {
        self.log = Some(std::sync::Arc::new(super::logging::ActivityLog::new(
            directory,
        )));
        self
    }

    /// Subscribes to the latest connection and root states.
    pub fn subscribe_status(&self) -> watch::Receiver<super::status::SyncSnapshot> {
        self.status.subscribe()
    }

    pub async fn emit(
        &self,
        kind: EventKind,
        selection_id: Option<&str>,
        source_path: Option<&SourcePath>,
        operation_id: &str,
        message: impl Into<String>,
    ) -> Result<(), EngineError> {
        let elapsed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| std::io::Error::other("system clock predates the Unix epoch"))?;
        let seconds = i64::try_from(elapsed.as_secs())
            .map_err(|_| std::io::Error::other("system clock exceeds supported range"))?;
        let timestamp =
            Timestamp::new(seconds, elapsed.subsec_nanos()).map_err(std::io::Error::other)?;
        let event = LifecycleEvent {
            timestamp,
            kind,
            selection_id: selection_id.map(str::to_owned),
            source_path: source_path.cloned(),
            operation_id: operation_id.to_owned(),
            message: message.into(),
            progress: None,
        };
        if let Some(log) = &self.log {
            log.append(&event)?;
        }
        super::status::update(&self.status, &event);
        let _ = self.events.send(event).await;
        Ok(())
    }

    pub fn progress(&self, selection_id: &str, operation_id: &str, file: ProgressSnapshot) {
        let source_bytes_processed = file.source_hashed_bytes;
        let uploaded_file_bytes = file.file_payload_sent;
        self.progress.send_modify(|current| {
            *current = Some(SelectionProgress {
                selection_id: selection_id.to_owned(),
                operation_id: operation_id.to_owned(),
                file,
                source_bytes_processed,
                uploaded_file_bytes,
            });
        });
    }

    pub async fn progress_phase(
        &self,
        selection_id: &str,
        operation_id: &str,
        progress: ProgressSnapshot,
    ) -> Result<(), EngineError> {
        let elapsed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| std::io::Error::other("system clock predates the Unix epoch"))?;
        let seconds = i64::try_from(elapsed.as_secs())
            .map_err(|_| std::io::Error::other("system clock exceeds supported range"))?;
        let timestamp =
            Timestamp::new(seconds, elapsed.subsec_nanos()).map_err(std::io::Error::other)?;
        let kind = match progress.phase {
            super::UploadPhase::Hashing => EventKind::Hashing,
            super::UploadPhase::Chunking => EventKind::Chunking,
            super::UploadPhase::Uploading => EventKind::Uploading,
            super::UploadPhase::Verifying => EventKind::Verifying,
        };
        let event = LifecycleEvent {
            timestamp,
            kind,
            selection_id: Some(selection_id.to_owned()),
            source_path: Some(progress.source_path.clone()),
            operation_id: operation_id.to_owned(),
            message: format!("{:?}", progress.phase),
            progress: Some(progress),
        };
        let _ = self.events.send(event).await;
        Ok(())
    }
}

#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod tests;
