use gpui::{div, prelude::*, px, rgb};

use super::super::model::{LogRow, Shell};
use super::components;

pub(super) fn render(shell: &Shell) -> impl IntoElement {
    let mut page = components::section("Logs").id("drip-logs").gap_3();
    if shell.state.logs.is_empty() {
        return page.child(components::message("logs-empty", "No activity recorded."));
    }
    let mut list = div()
        .id("drip-log-list")
        .tab_group()
        .flex()
        .flex_col()
        .gap_2();
    for row in shell.state.logs.iter().rev() {
        list = list.child(log_row(row));
    }
    page = page.child(list);
    page
}

fn log_row(row: &LogRow) -> impl IntoElement {
    let event = &row.event;
    let timestamp = event.timestamp;
    let kind = event_label(event.kind);
    let path = event
        .source_path
        .as_ref()
        .map(|path| path.to_windows_path());
    let path_text = path
        .as_ref()
        .map_or_else(String::new, |path| format!("Path: {path}"));

    let mut entry = div()
        .id(format!(
            "log-{}-{}-{}",
            timestamp.unix_seconds(),
            timestamp.nanos(),
            event.operation_id
        ))
        .tab_index(0)
        .flex()
        .flex_col()
        .gap_1()
        .p_3()
        .rounded(px(4.))
        .border_1()
        .border_color(rgb(0x354452))
        .bg(rgb(0x18212b))
        .focus(|style| style.border_color(rgb(0x8be0d3)))
        .child(
            div()
                .text_sm()
                .text_color(rgb(0xaebcc8))
                .child(format!("{}  {kind}", row.local_time)),
        );
    if let Some(path) = path {
        entry = entry.child(div().text_sm().child(path));
    }
    entry.child(
        div()
            .text_sm()
            .text_color(rgb(0xc5d2dc))
            .child(event.message.clone()),
    )
}

fn event_label(kind: drip_client::EventKind) -> &'static str {
    use drip_client::EventKind::*;
    match kind {
        SourceChanged => "Source changed",
        Queued => "Queued",
        Scanning => "Scanning",
        UploadStarted => "Upload started",
        UploadCompleted => "Upload completed",
        FileChecked => "File checked",
        Hashing => "Hashing",
        Chunking => "Chunking / hashing",
        Uploading => "Uploading",
        Verifying => "Verifying",
        DirectoryCreated => "Directory created",
        Deleted => "Deleted",
        Retry => "Retry",
        Error => "Error",
        Cancelled => "Cancelled",
        SelectionChanged => "Selection changed",
        Pausing => "Pausing",
        Paused => "Paused",
        Resumed => "Resumed",
        Connected => "Connected",
        Disconnected => "Disconnected",
        Synced => "Synced",
        UnavailableRoot => "Unavailable source",
    }
}
