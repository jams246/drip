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
        Some(SelectionState::Offline) => return "Waiting for connection",
        Some(SelectionState::UnavailableRoot) => return "File or folder unavailable",
        Some(SelectionState::Cancelled) => return "Stopped",
        Some(SelectionState::Error) => return "Needs attention",
        _ => {}
    }
    match connection {
        ConnectionState::Pausing => return "Pausing...",
        ConnectionState::Paused => return "Paused",
        ConnectionState::Disconnected => return "Waiting for connection",
        ConnectionState::Connected => {}
    }
    match status.map(|root| root.state) {
        Some(SelectionState::Paused) => return "Paused",
        Some(SelectionState::Synced) => return "Up to date",
        _ => {}
    }
    if let Some(current) = current {
        return progress::stage(current.file.phase).label;
    }
    match status.map(|root| root.state) {
        Some(SelectionState::Scanning) => return "Checking for changes",
        Some(SelectionState::Uploading)
            if last_file.is_some_and(|event| {
                matches!(
                    event.kind,
                    EventKind::UploadCompleted | EventKind::FileChecked
                )
            }) =>
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

#[cfg(test)]
#[path = "status_tests.rs"]
mod tests;
