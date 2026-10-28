mod status;

use std::time::Instant;

use drip_client::{RootStatus, SelectionProgress};
use gpui::{Context, IntoElement, Window, div, prelude::*, px, rgb};

use crate::gui::{
    model::Shell,
    theme,
    views::components::{self, Glyph},
};

pub(super) use status::visible_progress;

pub(super) fn card(
    shell: &Shell,
    selection: &drip_client::settings::SavedSelection,
    root_status: Option<&RootStatus>,
    current: Option<&SelectionProgress>,
    progress_view: impl IntoElement,
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    let path = selection.source.to_windows_path();
    let name = selection
        .source
        .components()
        .last()
        .cloned()
        .unwrap_or_else(|| format!("{}:", selection.source.drive()));
    let directory = selection.root_kind == drip_core::protocol::EntryKind::Directory;

    let label = status::label(
        shell.state.connection_at(Instant::now()),
        shell.control.borrow().paused,
        shell.state.quitting,
        root_status,
        current,
        shell.state.last_file.get(&selection.id),
    );

    let (glyph, color) = status::icon(label);
    let mut card = components::card(format!("home-root-{}", selection.id)).child(
        div()
            .w_full()
            .flex()
            .flex_row()
            .items_start()
            .gap_4()
            .child(components::icon(
                if directory {
                    Glyph::Folder
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
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_color(rgb(color))
                    .child(components::icon(glyph, 20.))
                    .child(label),
            ),
    );
    if let Some(current) = current {
        let current_path = current.file.source_path.to_windows_path();
        card = card.child(
            div()
                .id(format!("home-current-file-{}", selection.id))
                .text_sm()
                .pt_2()
                .child(format!(
                    "Current file: {}",
                    components::breakable_path(&current_path)
                )),
        );
        card = card.child(progress_view);
    }
    if matches!(label, "Needs attention" | "File or folder unavailable") {
        card = card.child(components::activity_link(
            cx,
            window,
            &format!("home-{}", selection.id),
            "View Activity",
        ));
    }
    card
}
