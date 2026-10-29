use std::path::PathBuf;

use gpui::{Context, Window, div, prelude::*, rgb};

use super::super::model::Shell;
use super::components::{self, ButtonKind};
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
        .gap_4()
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
    let mut list = components::watched_list("watch-watched-list");
    let mut has_rows = false;
    for (index, selection) in shell.state.active_selections().enumerate() {
        has_rows = true;
        let path = PathBuf::from(selection.source.to_windows_path());
        list = list.child(
            components::watched_row(
                format!("watch-selection-{}", selection.id),
                selection,
                index > 0,
            )
            .child(components::button(
                cx,
                window,
                format!("watch-remove-{}", selection.id),
                "Remove",
                ButtonKind::Secondary,
                enabled,
                move |shell, _, window, cx| shell.remove(path.clone(), window, cx),
            )),
        );
    }
    if has_rows {
        page = page.child(list);
    } else {
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
