use drip_core::SourcePath;
use serde::{Deserialize, Serialize};
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
}

pub(super) fn update(sender: &watch::Sender<SyncSnapshot>, event: &LifecycleEvent) {
    sender.send_modify(|snapshot| apply(snapshot, event));
}

pub(super) fn apply(snapshot: &mut SyncSnapshot, event: &LifecycleEvent) {
    match event.kind {
        EventKind::Connected | EventKind::Resumed => {
            snapshot.connection = ConnectionState::Connected
        }
        EventKind::Disconnected => {
            snapshot.connection = ConnectionState::Disconnected;
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
