mod progress;
mod root;

use std::time::{Duration, Instant};

use gpui::{Context, div, prelude::*, rgb};

use super::super::{
    model::{Shell, Tab},
    theme,
};
use super::components;

pub(super) fn render(shell: &mut Shell, cx: &mut Context<Shell>) -> impl IntoElement {
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
    let active = shell
        .state
        .settings
        .selections
        .iter()
        .filter(|item| item.active);
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
        let last_file = shell.state.last_file.get(&selection.id);
        let offset = shell
            .progress_animation_started
            .map(|start| progress::animation_offset(start.elapsed()));
        page = page.child(root::card(
            shell, selection, status, current, last_file, offset, cx,
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
    if should_animate(shell) {
        start_animation(shell, cx);
    }
    page
}

fn should_animate(shell: &Shell) -> bool {
    if shell.tab != Tab::Home || shell.state.quitting {
        return false;
    }
    let Some(progress) = shell.state.progress.as_ref() else {
        return false;
    };
    if !root::is_indeterminate(progress) {
        return false;
    }
    let status = shell
        .state
        .status
        .roots
        .iter()
        .find(|root| root.selection_id == progress.selection_id);
    root::visible_progress(
        shell.state.status.connection,
        shell.control.borrow().paused,
        shell.state.quitting,
        Some(progress),
        &progress.selection_id,
        status,
    )
}

fn start_animation(shell: &mut Shell, cx: &mut Context<Shell>) {
    if shell.progress_animation_active {
        return;
    }
    shell.progress_animation_active = true;
    shell.progress_animation_started = Some(Instant::now());
    cx.spawn(async move |this, cx| {
        loop {
            cx.background_executor()
                .timer(Duration::from_millis(80))
                .await;
            let running = this
                .update(cx, |shell, cx| {
                    if should_animate(shell) {
                        cx.notify();
                        true
                    } else {
                        shell.progress_animation_active = false;
                        shell.progress_animation_started = None;
                        false
                    }
                })
                .unwrap_or(false);
            if !running {
                break;
            }
        }
    })
    .detach();
}
