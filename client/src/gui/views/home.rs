pub(super) mod progress;
mod root;

use gpui::{Context, Window, div, prelude::*, rgb};

use super::super::{
    model::{Shell, Tab},
    theme,
};
use super::components;

pub(super) fn render(
    shell: &mut Shell,
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    let mut page = div().id("drip-home").w_full().flex().flex_col().gap_6();
    if shell.control.borrow().paused
        && shell.state.status.connection != drip_client::ConnectionState::Paused
    {
        page = page.child(components::message("home-pausing", "Pausing..."));
    }
    if shell.state.quitting {
        page = page.child(components::message("home-stopping", "Stopped"));
    }
    let mut has_rows = false;
    let mut list = components::watched_list("home-watched-list");
    for (index, selection) in shell.state.active_selections().enumerate() {
        has_rows = true;
        let status = shell
            .state
            .status
            .roots
            .iter()
            .find(|root| root.selection_id == selection.id);
        list = list.child(root::row(shell, selection, status, index > 0, window, cx));
    }
    if has_rows {
        page = page.child(list);
    } else {
        page = page.child(
            components::card("home-empty-card")
                .child(
                    div()
                        .text_color(rgb(theme::MUTED))
                        .child("Nothing is being watched yet."),
                )
                .child(components::button(
                    cx,
                    window,
                    "home-choose-selections",
                    "Choose files or folders",
                    components::ButtonKind::Primary,
                    !shell.state.loading && !shell.state.quitting,
                    |shell, _, window, cx| shell.select_tab(Tab::Watch, window, cx),
                )),
        );
    }
    page
}
