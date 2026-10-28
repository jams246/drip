mod status;

use drip_client::{RootStatus, SelectionProgress};
use gpui::{Context, IntoElement, div, prelude::*, px, rgb};

use super::progress;
use crate::gui::{
    model::Shell,
    theme,
    views::components::{self, Glyph},
};

pub(super) use status::{is_indeterminate, visible_progress};

pub(super) fn card(
    shell: &Shell,
    selection: &drip_client::settings::SavedSelection,
    root_status: Option<&RootStatus>,
    current: Option<&SelectionProgress>,
    last_file: Option<&drip_client::LifecycleEvent>,
    animation_offset: Option<f32>,
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
        shell.state.status.connection,
        shell.control.borrow().paused,
        shell.state.quitting,
        root_status,
        current,
        last_file,
    );

    let (glyph, color) = status::icon(label);
    let mut card = components::card(format!("home-root-{}", selection.id))
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
        card = card.child(progress::render(current, animation_offset));
    } else if let Some(message) = last_file.and_then(|event| status::last_file_label(event.kind)) {
        card = card.child(
            div()
                .id(format!("home-last-file-{}", selection.id))
                .text_sm()
                .text_color(rgb(theme::MUTED))
                .child(message),
        );
    }
    if matches!(label, "Needs attention" | "File or folder unavailable") {
        card = card.child(components::activity_link(cx, "View Activity"));
    }
    card
}
