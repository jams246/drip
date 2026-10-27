use drip_client::{ConnectionState, EventKind, SelectionState, UploadPhase};
use gpui::{div, prelude::*, px, rgb};

use super::super::model::Shell;
use super::components;
use drip_client::settings::SavedSelection;

mod progress;

pub(super) fn render(shell: &Shell) -> impl IntoElement {
    let mut page = div()
        .id("drip-home")
        .flex()
        .flex_col()
        .gap_4()
        .child(components::section("Home"));
    let pause_pending = (shell.control.borrow().paused
        && shell.state.status.connection != ConnectionState::Paused)
        || shell.state.status.connection == ConnectionState::Pausing;
    if pause_pending {
        page = page.child(components::message(
            "home-pausing",
            "Pause requested. Waiting for the server barrier.",
        ));
    }
    if shell.state.quitting {
        page = page.child(components::message(
            "home-stopping",
            "Stopping. Waiting for safe engine shutdown.",
        ));
    }
    let active = shell
        .state
        .settings
        .selections
        .iter()
        .filter(|item| item.active);
    let mut has_rows = false;
    for selection in active {
        has_rows = true;
        let status = shell
            .state
            .status
            .roots
            .iter()
            .find(|root| root.selection_id == selection.id);
        let current = shell
            .state
            .progress
            .as_ref()
            .filter(|progress| progress.selection_id == selection.id);
        let last_file = shell.state.last_file.get(&selection.id);
        let queued = status.is_none()
            && shell
                .state
                .logs
                .iter()
                .rev()
                .find(|row| row.event.selection_id.as_deref() == Some(selection.id.as_str()))
                .is_some_and(|row| row.event.kind == EventKind::Queued);
        page = page.child(root_card(selection, status, current, last_file, queued));
    }
    if !has_rows {
        page = page.child(components::message(
            "home-empty",
            "No watched files or folders.",
        ));
    }
    page
}

fn root_card(
    selection: &SavedSelection,
    status: Option<&drip_client::RootStatus>,
    current: Option<&drip_client::SelectionProgress>,
    last_file: Option<&drip_client::LifecycleEvent>,
    queued: bool,
) -> impl IntoElement {
    let path = selection.source.to_windows_path();
    let kind = if selection.root_kind == drip_core::protocol::EntryKind::Directory {
        "Folder"
    } else {
        "File"
    };
    let current_path = current.map(|progress| progress.file.source_path.to_windows_path());

    let mut card = div()
        .id(format!("home-root-{}", selection.id))
        .flex()
        .flex_col()
        .gap_2()
        .p_4()
        .rounded(px(6.))
        .border_1()
        .border_color(rgb(0x354452))
        .bg(rgb(0x18212b))
        .child(div().text_sm().text_color(rgb(0xaebcc8)).child(kind))
        .child(div().text_base().child(path.clone()))
        .child(root_state(selection, status, current, last_file, queued));
    if let Some(current) = current {
        card = card.child(
            div()
                .id(format!("home-current-file-{}", selection.id))
                .text_sm()
                .text_color(rgb(0xcbd8e2))
                .child(format!(
                    "Current file: {}",
                    current.file.source_path.to_windows_path()
                )),
        );
        card = card.child(progress::render(current));
    }
    card
}

fn root_state(
    selection: &SavedSelection,
    status: Option<&drip_client::RootStatus>,
    current: Option<&drip_client::SelectionProgress>,
    last_file: Option<&drip_client::LifecycleEvent>,
    queued: bool,
) -> impl IntoElement {
    let (label, message) = if let Some(progress) = current {
        (
            match progress.file.phase {
                UploadPhase::Hashing => "Hashing",
                UploadPhase::Chunking => "Chunking / hashing",
                UploadPhase::Uploading => "Uploading",
                UploadPhase::Verifying => "Verifying",
            },
            "",
        )
    } else {
        match status.map(|root| root.state) {
            Some(SelectionState::Scanning) => {
                ("Scanning", status.map_or("", |root| root.message.as_str()))
            }
            Some(SelectionState::Uploading)
                if last_file.is_some_and(|event| event.kind == EventKind::UploadCompleted) =>
            {
                ("Reconciling", "")
            }
            Some(SelectionState::Uploading) => ("Uploading", "Waiting for file progress"),
            Some(SelectionState::Synced) => {
                ("Synced", status.map_or("", |root| root.message.as_str()))
            }
            Some(SelectionState::Paused) => {
                ("Paused", status.map_or("", |root| root.message.as_str()))
            }
            Some(SelectionState::Offline) => {
                ("Offline", status.map_or("", |root| root.message.as_str()))
            }
            Some(SelectionState::UnavailableRoot) => (
                "Unavailable source",
                status.map_or("", |root| root.message.as_str()),
            ),
            Some(SelectionState::Error) => {
                ("Error", status.map_or("", |root| root.message.as_str()))
            }
            None if queued => ("Queued", ""),
            None if selection.active => ("Waiting for engine status", ""),
            None => ("Inactive", ""),
        }
    };
    let mut view = div()
        .id(format!("home-state-{}", selection.id))
        .text_color(state_color(label))
        .child(label);
    if !message.is_empty() {
        view = view.child(div().text_color(rgb(0xc5d2dc)).child(message.to_owned()));
    }
    if current.is_none()
        && let Some(event) = last_file
    {
        view = view.child(last_file_line(event));
    }
    view
}

fn state_color(label: &str) -> impl Into<gpui::Hsla> {
    match label {
        "Error" | "Unavailable source" => rgb(0xffc07a),
        "Synced" => rgb(0x8be0d3),
        _ => rgb(0xe9eef4),
    }
}

fn last_file_line(event: &drip_client::LifecycleEvent) -> impl IntoElement {
    let label = match event.kind {
        EventKind::UploadCompleted => "Last file verified",
        EventKind::Retry => "Last file retry",
        EventKind::Error => "Last file error",
        _ => "File outcome",
    };
    let path = event
        .source_path
        .as_ref()
        .map(|path| path.to_windows_path());
    let text = path.map_or_else(
        || format!("{label}: {}", event.message),
        |path| format!("{label}: {path}. {}", event.message),
    );
    div()
        .id(format!("home-last-file-{}", event.operation_id))
        .text_sm()
        .text_color(rgb(0xc5d2dc))
        .child(text)
}
