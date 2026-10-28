use drip_core::SourcePath;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tokio::sync::watch;

use super::lifecycle::{EventKind, LifecycleEvent};

/// Current engine connection state, independent of per-path activity.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub enum ConnectionState {
    Connected,
    Pausing,
    Paused,
    #[default]
    Disconnected,
}

/// Current activity of a selected root.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum SelectionState {
    Scanning,
    Uploading,
    Synced,
    Paused,
    Offline,
    UnavailableRoot,
    Cancelled,
    Error,
}

/// Path context retained for diagnostics and later Home rendering.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct RootStatus {
    pub selection_id: String,
    pub source_path: SourcePath,
    pub state: SelectionState,
    pub message: String,
}

/// Replaceable status snapshot, separate from ordered lifecycle records.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct SyncSnapshot {
    pub connection: ConnectionState,
    pub roots: Vec<RootStatus>,
    #[serde(skip)]
    pub(super) last_authenticated: Option<Instant>,
    #[serde(skip)]
    pub(super) authentication_failed: bool,
    #[serde(skip)]
    pub(super) failure_epoch: u64,
}

impl SyncSnapshot {
    /// Footer reachability is independent of source activity and pending pause intent.
    pub fn footer_label(&self, now: Instant) -> &'static str {
        if !self.authenticated_at(now) {
            "Disconnected"
        } else if self.connection == ConnectionState::Paused {
            "Paused"
        } else {
            "Connected"
        }
    }

    pub(super) fn authenticated_at(&self, now: Instant) -> bool {
        !self.authentication_failed
            && self
                .last_authenticated
                .is_some_and(|last| now.saturating_duration_since(last) <= Duration::from_secs(45))
    }
}

pub(super) fn update(sender: &watch::Sender<SyncSnapshot>, event: &LifecycleEvent) {
    sender.send_modify(|snapshot| apply(snapshot, event));
}

pub(super) fn apply(snapshot: &mut SyncSnapshot, event: &LifecycleEvent) {
    match event.kind {
        EventKind::Connected => {
            if !matches!(
                snapshot.connection,
                ConnectionState::Paused | ConnectionState::Pausing
            ) {
                snapshot.connection = ConnectionState::Connected;
            }
        }
        EventKind::Resumed => snapshot.connection = ConnectionState::Connected,
        EventKind::Disconnected => {
            snapshot.connection = if matches!(
                snapshot.connection,
                ConnectionState::Paused | ConnectionState::Pausing
            ) {
                ConnectionState::Pausing
            } else {
                ConnectionState::Disconnected
            };
            for root in &mut snapshot.roots {
                root.state = SelectionState::Offline;
            }
        }
        EventKind::Pausing => snapshot.connection = ConnectionState::Pausing,
        EventKind::Paused => {
            snapshot.connection = ConnectionState::Paused;
            for root in &mut snapshot.roots {
                root.state = SelectionState::Paused;
            }
        }
        _ => {}
    }
    let state = match event.kind {
        EventKind::Scanning => SelectionState::Scanning,
        EventKind::UploadStarted => SelectionState::Uploading,
        EventKind::Synced => SelectionState::Synced,
        EventKind::UnavailableRoot => SelectionState::UnavailableRoot,
        EventKind::Cancelled => SelectionState::Cancelled,
        EventKind::Error | EventKind::Retry => SelectionState::Error,
        _ => return,
    };
    let (Some(selection_id), Some(source_path)) = (&event.selection_id, &event.source_path) else {
        return;
    };
    let root = RootStatus {
        selection_id: selection_id.clone(),
        source_path: source_path.clone(),
        state,
        message: event.message.clone(),
    };
    if let Some(existing) = snapshot
        .roots
        .iter_mut()
        .find(|root| root.selection_id == *selection_id)
    {
        *existing = root;
    } else {
        snapshot.roots.push(root);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_retry_preserves_processing_root_and_connection() {
        let path = SourcePath::new('C', vec!["file.bin".into()]).unwrap();
        let mut snapshot = SyncSnapshot {
            connection: ConnectionState::Connected,
            roots: vec![RootStatus {
                selection_id: "root".into(),
                source_path: path.clone(),
                state: SelectionState::Uploading,
                message: "upload in progress".into(),
            }],
            ..SyncSnapshot::default()
        };
        let before = snapshot.clone();
        let event = LifecycleEvent {
            timestamp: drip_core::Timestamp::new(1, 0).unwrap(),
            kind: EventKind::ControlRetry,
            selection_id: Some("root".into()),
            source_path: Some(path),
            operation_id: "upload".into(),
            message: "HTTP 503; upload plan retry 1 of 3".into(),
            progress: None,
        };

        apply(&mut snapshot, &event);

        assert_eq!(snapshot, before);
    }
}
