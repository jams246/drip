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
    let active: Vec<_> = shell
        .state
        .settings
        .selections
        .iter()
        .filter(|item| item.active)
        .collect();
    let mut page = div()
        .id("drip-home")
        .w_full()
        .flex()
        .flex_col()
        .gap_6()
        .child(components::page_header(
            "Home",
            Some("Your watched files and folders."),
        ));
    if shell.control.borrow().paused
        && shell.state.status.connection != drip_client::ConnectionState::Paused
    {
        page = page.child(components::message("home-pausing", "Pausing..."));
    }
    if shell.state.quitting {
        page = page.child(components::message("home-stopping", "Stopped"));
    }
    let mut has_rows = false;
    for selection in active {
        has_rows = true;
        let status = shell
            .state
            .status
            .roots
            .iter()
            .find(|root| root.selection_id == selection.id);
        page = page.child(root::card(shell, selection, status, window, cx));
    }
    if !has_rows {
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
