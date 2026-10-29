use drip_client::{ConnectionState, RootStatus, SelectionState};

use crate::gui::{theme, views::components::Glyph};

pub(super) fn label(
    connection: ConnectionState,
    pause_requested: bool,
    stopping: bool,
    status: Option<&RootStatus>,
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
        Some(SelectionState::Paused) => "Paused",
        Some(SelectionState::Synced) => "Up to date",
        Some(SelectionState::Scanning | SelectionState::Uploading) => "Processing",
        _ => "Waiting",
    }
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
