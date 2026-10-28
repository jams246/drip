use gpui::{IntoElement, div, prelude::*, px, rgb};

use super::super::super::model::Shell;
use super::super::components;
use super::super::theme;

pub(super) fn connection_details(shell: &Shell) -> impl IntoElement {
    let mut card =
        components::card("settings-binding").child(components::card_heading("Connection details"));
    if let Some(binding) = &shell.state.binding {
        for (label, value) in [
            ("Server", binding.server.clone()),
            ("Computer ID", binding.pc_id.clone()),
            (
                "Server folder",
                format!("/mirror/{}", binding.mirror_directory),
            ),
        ] {
            card = card.child(detail_row(label, &value));
        }
    } else {
        card = card.child(
            div()
                .text_sm()
                .text_color(rgb(theme::MUTED))
                .child("No saved computer registration."),
        );
    }
    card
}

pub(super) fn current_transfer(progress: &drip_client::SelectionProgress) -> impl IntoElement {
    let file = &progress.file;
    let total = file.total_bytes.map_or_else(
        || "total unknown".to_owned(),
        |total| format!("{total} bytes"),
    );
    components::card("activity-current-transfer")
        .child(components::card_heading("Current transfer"))
        .child(detail_row("Selection ID", &progress.selection_id))
        .child(detail_row("Operation ID", &progress.operation_id))
        .child(detail_row("File", &file.source_path.to_windows_path()))
        .child(detail_row("Stage", &format!("{:?}", file.phase)))
        .child(detail_row(
            "Stage bytes",
            &format!("{} processed; {total}", file.processed_bytes),
        ))
        .child(detail_row(
            "BLAKE3",
            &format!("{} source bytes hashed", progress.source_bytes_processed),
        ))
        .child(detail_row(
            "FastCDC",
            &format!(
                "{} bytes, {} chunks",
                file.source_chunked_bytes, file.chunk_count
            ),
        ))
        .child(detail_row(
            "Payload",
            &format!(
                "{} bytes planned; {} bytes sent",
                file.planned_payload_bytes, progress.uploaded_file_bytes
            ),
        ))
}

pub(super) fn selection_details(shell: &Shell) -> impl IntoElement {
    let mut card = components::card("activity-selections")
        .child(components::card_heading("Watched locations"));
    if let Some(binding) = &shell.state.binding {
        for selection in &shell.state.settings.selections {
            let mirror = mirror_path(&binding.mirror_directory, &selection.source);
            card = card.child(
                div()
                    .id(format!("activity-selection-{}", selection.id))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(detail_row("Selection ID", &selection.id))
                    .child(detail_row("Server copy", &mirror))
                    .child(detail_row(
                        "Status",
                        if selection.active {
                            "Watching"
                        } else {
                            "Stopped watching"
                        },
                    )),
            );
        }
    }
    card
}

fn detail_row(label: &str, value: &str) -> impl IntoElement {
    div()
        .id(format!("detail-{label}"))
        .flex()
        .items_start()
        .gap_6()
        .child(
            div()
                .w(px(240.))
                .flex_shrink_0()
                .text_sm()
                .text_color(rgb(theme::MUTED))
                .child(label.to_owned()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_sm()
                .child(components::breakable_path(value)),
        )
}

fn mirror_path(mirror_directory: &str, source: &drip_core::SourcePath) -> String {
    let root = format!("/mirror/{mirror_directory}/{}", source.drive());
    if source.components().is_empty() {
        root
    } else {
        format!("{root}/{}", source.components().join("/"))
    }
}

#[cfg(test)]
mod tests {
    use super::mirror_path;

    #[test]
    fn mirror_path_preserves_drive_and_source_components() {
        let source = drip_core::SourcePath::parse_absolute(r"C:\Users\Ada\file.txt").unwrap();
        assert_eq!(
            mirror_path("drip-ada-123", &source),
            "/mirror/drip-ada-123/C/Users/Ada/file.txt"
        );
    }
}
