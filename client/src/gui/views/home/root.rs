mod status;

use std::time::Instant;

use drip_client::RootStatus;
use gpui::{Context, IntoElement, Window, div, prelude::*, rgb};

use crate::gui::{model::Shell, views::components};

pub(super) fn row(
    shell: &Shell,
    selection: &drip_client::settings::SavedSelection,
    root_status: Option<&RootStatus>,
    divider: bool,
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    let label = status::label(
        shell.state.connection_at(Instant::now()),
        shell.control.borrow().paused,
        shell.state.quitting,
        root_status,
    );
    let (glyph, color) = status::icon(label);
    let mut status = div().flex().items_center().flex_shrink_0().gap_3().child(
        div()
            .flex()
            .items_center()
            .gap_2()
            .text_color(rgb(color))
            .child(components::icon(glyph, 16.))
            .child(label),
    );
    if matches!(label, "Needs attention" | "File or folder unavailable") {
        status = status.child(components::activity_link(
            cx,
            window,
            &format!("home-{}", selection.id),
            "View Activity",
        ));
    }
    components::watched_row(format!("home-root-{}", selection.id), selection, divider)
        .child(status)
        .child(
            div()
                .absolute()
                .bottom_0()
                .left_0()
                .right_0()
                .child(super::progress::render(
                    &selection.id,
                    label == "Processing",
                )),
        )
}
