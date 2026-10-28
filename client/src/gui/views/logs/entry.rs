use gpui::{Context, IntoElement, Window, div, prelude::*, px, rgb};

use super::super::components;
use super::super::theme;
use crate::gui::{
    focus,
    model::{LogRow, Shell},
};

pub(super) fn log_row(
    row: &LogRow,
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    let event = &row.event;
    let timestamp = event.timestamp;
    let kind = event_label(event.kind);
    let path = event
        .source_path
        .as_ref()
        .map(|path| path.to_windows_path());

    let id = format!(
        "log-{}-{}-{}",
        timestamp.unix_seconds(),
        timestamp.nanos(),
        event.operation_id
    );
    let focus = focus::handle(&id, true, window, cx);
    let mut entry = focus::reveal(div(), &id, focus.clone(), window, cx)
        .track_focus(&focus)
        .id(format!(
            "log-{}-{}-{}",
            timestamp.unix_seconds(),
            timestamp.nanos(),
            event.operation_id
        ))
        .tab_index(0)
        .w_full()
        .min_w_0()
        .flex()
        .flex_col()
        .gap_2()
        .p_5()
        .rounded(px(12.))
        .border_1()
        .border_color(rgb(theme::BORDER))
        .bg(rgb(theme::PANEL))
        .focus(|style| style.border_color(rgb(theme::ACCENT)))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .items_start()
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(theme::MUTED))
                        .child(row.local_time.clone()),
                )
                .child(
                    div()
                        .text_size(px(18.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(kind),
                ),
        );
    if let Some(path) = path {
        entry = entry.child(div().text_sm().child(components::breakable_path(&path)));
    }
    entry
        .child(div().text_sm().text_color(rgb(theme::MUTED)).child(format!(
            "Selection ID: {}. Operation ID: {}",
            event.selection_id.as_deref().unwrap_or("unknown"),
            event.operation_id
        )))
        .child(
            div()
                .text_sm()
                .child(components::breakable_path(&event.message)),
        )
}

fn event_label(kind: drip_client::EventKind) -> &'static str {
    use drip_client::EventKind::*;
    match kind {
        SourceChanged => "Changes detected",
        Queued => "Waiting",
        Scanning => "Checking for changes",
        UploadStarted => "Sending changes",
        UploadCompleted => "File uploaded",
        FileChecked => "File checked",
        Hashing => "Preparing file",
        Chunking => "Preparing changes",
        Uploading => "Sending changes",
        Verifying => "Checking upload",
        DirectoryCreated => "Folder created",
        Deleted => "Deleted",
        Retry | ControlRetry => "Trying again",
        Error => "Needs attention",
        Cancelled => "Stopped",
        SelectionChanged => "Watch list changed",
        Pausing => "Pausing",
        Paused => "Paused",
        Resumed => "Resumed",
        Connected => "Connected",
        Disconnected => "Disconnected",
        Synced => "Up to date",
        UnavailableRoot => "File or folder unavailable",
    }
}
