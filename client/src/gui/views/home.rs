pub(super) mod progress;
mod root;

use gpui::{Context, div, prelude::*, rgb};

use super::super::{
    model::{Shell, Tab},
    theme,
};
use super::components;

pub(super) fn render(shell: &mut Shell, cx: &mut Context<Shell>) -> impl IntoElement {
    let active: Vec<_> = shell
        .state
        .settings
        .selections
        .iter()
        .filter(|item| item.active)
        .collect();
    shell
        .progress_views
        .retain(|id, _| active.iter().any(|selection| selection.id == *id));
    for selection in &active {
        shell
            .progress_views
            .entry(selection.id.clone())
            .or_insert_with(|| cx.new(|_| progress::ProgressView::default()));
    }
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
        let progress = shell.state.progress.as_ref();
        let current = progress.filter(|_| {
            root::visible_progress(
                shell.state.status.connection,
                shell.control.borrow().paused,
                shell.state.quitting,
                progress,
                &selection.id,
                status,
            )
        });
        let progress_view = shell.progress_views.get(&selection.id).unwrap().clone();
        progress_view.update(cx, |view, cx| view.set_progress(current.cloned(), cx));
        let last_file = shell.state.last_file.get(&selection.id);
        page = page.child(root::card(
            shell,
            selection,
            status,
            current,
            last_file,
            progress_view,
            cx,
        ));
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
