use std::path::PathBuf;

use gpui::{Context, Window, div, prelude::*, px, rgb};

use super::super::model::Shell;
use super::components::{self, ButtonKind, Glyph};
use super::theme;

pub(super) fn render(
    shell: &Shell,
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    let enabled = !shell.state.loading && !shell.state.busy && !shell.state.quitting;
    let mut page = div()
        .id("drip-watch")
        .w_full()
        .flex()
        .flex_col()
        .gap_6()
        .child(components::page_header(
            "Watch",
            Some("Choose the files and folders to send to your server."),
        ))
        .child(
            div()
                .flex()
                .gap_3()
                .child(components::button(
                    cx,
                    window,
                    "watch-add-folder",
                    "Add folder",
                    ButtonKind::Primary,
                    enabled,
                    |shell, _, window, cx| shell.add_folder(window, cx),
                ))
                .child(components::button(
                    cx,
                    window,
                    "watch-add-file",
                    "Add file",
                    ButtonKind::Secondary,
                    enabled,
                    |shell, _, window, cx| shell.add_file(window, cx),
                )),
        );
    if let Some(feedback) = &shell.state.watch_feedback {
        page = page.child(components::feedback_summary(
            cx,
            window,
            "watch-message",
            feedback,
        ));
    }
    for selection in &shell.state.settings.selections {
        let path = selection.source.to_windows_path();
        let name = selection
            .source
            .components()
            .last()
            .cloned()
            .unwrap_or_else(|| format!("{}:", selection.source.drive()));
        let directory = selection.root_kind == drip_core::protocol::EntryKind::Directory;

        let active = selection.active;
        let id = format!("watch-selection-{}", selection.id);

        let mut row = components::card(id)
            .flex_row()
            .items_center()
            .gap_4()
            .child(components::icon(
                if directory {
                    Glyph::FolderFilled
                } else {
                    Glyph::File
                },
                36.,
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_size(px(18.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(name),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(theme::MUTED))
                            .child(components::breakable_path(&path)),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_sm()
                            .child(div().w(px(8.)).h(px(8.)).rounded_full().bg(rgb(if active {
                                theme::SUCCESS
                            } else {
                                theme::MUTED
                            })))
                            .child(if active {
                                "Watching"
                            } else {
                                "Stopped watching"
                            }),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(theme::MUTED))
                            .child(if active {
                                "The copy on your server will stay."
                            } else {
                                "Your server copy is kept."
                            }),
                    ),
            );
        if active {
            let path = PathBuf::from(path);
            row = row.child(components::button(
                cx,
                window,
                format!("watch-remove-{}", selection.id),
                "Stop watching",
                ButtonKind::Secondary,
                enabled,
                move |shell, _, window, cx| shell.remove(path.clone(), window, cx),
            ));
        }
        page = page.child(row);
    }
    if shell.state.settings.selections.is_empty() {
        page = page.child(
            components::card("watch-empty-card").child(
                div()
                    .text_color(rgb(theme::MUTED))
                    .child("Add a file or folder to get started."),
            ),
        );
    }
    page
}
