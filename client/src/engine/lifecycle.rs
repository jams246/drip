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
}

/// Coalesced progress, separate from ordered terminal activity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectionProgress {
    pub selection_id: String,
    pub operation_id: String,
    pub file: ProgressSnapshot,
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
        };
        if let Some(log) = &self.log {
            log.append(&event)?;
        }
        super::status::update(&self.status, &event);
        let _ = self.events.send(event).await;
        Ok(())
    }

    pub fn progress(&self, selection_id: &str, operation_id: &str, file: ProgressSnapshot) {
        self.progress.send_replace(Some(SelectionProgress {
            selection_id: selection_id.to_owned(),
            operation_id: operation_id.to_owned(),
            file,
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
