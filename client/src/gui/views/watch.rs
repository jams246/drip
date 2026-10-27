use std::path::PathBuf;

use gpui::{Context, div, prelude::*, px, rgb};

use super::super::model::Shell;
use super::components;

pub(super) fn render(shell: &Shell, cx: &mut Context<Shell>) -> impl IntoElement {
    let enabled = !shell.state.loading && !shell.state.busy && !shell.state.quitting;
    let mut page = components::section("Watch").id("drip-watch").gap_4().child(
        div()
            .flex()
            .gap_2()
            .child(components::button(
                cx,
                "watch-add-file",
                "Add file",
                enabled,
                |shell, _, window, cx| {
                    shell.add_file(window, cx);
                },
            ))
            .child(components::button(
                cx,
                "watch-add-folder",
                "Add folder",
                enabled,
                |shell, _, window, cx| {
                    shell.add_folder(window, cx);
                },
            )),
    );
    if !shell.state.watch_message.is_empty() {
        page = page.child(components::message(
            "watch-message",
            &shell.state.watch_message,
        ));
    }
    for selection in &shell.state.settings.selections {
        let source = selection.source.to_windows_path();
        let mirror = mirror_destination(shell, &selection.source);

        let active = selection.active;
        let mut row = div()
            .id(format!("watch-selection-{}", selection.id))
            .flex()
            .flex_col()
            .gap_2()
            .p_4()
            .rounded(px(6.))
            .border_1()
            .border_color(rgb(0x354452))
            .bg(rgb(0x18212b))
            .child(div().text_sm().text_color(rgb(0xaebcc8)).child(if active {
                "Active"
            } else {
                "Retained copy"
            }))
            .child(div().child(format!("Source: {source}")))
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(0xc5d2dc))
                    .child(format!("Mirror: {mirror}")),
            );
        if active {
            let path = PathBuf::from(source);
            row = row.child(components::button(
                cx,
                format!("watch-remove-{}", selection.id),
                "Remove selection",
                enabled,
                move |shell, _, window, cx| shell.remove(path.clone(), window, cx),
            ));
        }
        page = page.child(row);
    }
    if shell.state.settings.selections.is_empty() {
        page = page.child(components::message(
            "watch-empty",
            "No files or folders selected.",
        ));
    }
    page
}

fn mirror_destination(shell: &Shell, source: &drip_core::SourcePath) -> String {
    let Some(binding) = shell.state.binding.as_ref() else {
        return "Unavailable until paired".into();
    };
    mirror_path(&binding.mirror_directory, source)
}

fn mirror_path(mirror_directory: &str, source: &drip_core::SourcePath) -> String {
    let root = format!("/mirror/{}/{}", mirror_directory, source.drive());
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
