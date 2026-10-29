mod details;
mod entry;

use gpui::{Context, Window, div, prelude::*, rgb};

use super::super::model::Shell;
use super::components;
use super::theme;
use details::{connection_details, current_transfer, selection_details};
use entry::log_row;

pub(super) fn render(
    shell: &Shell,
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    let mut page = div()
        .id("drip-logs")
        .w_full()
        .flex()
        .flex_col()
        .gap_6()
        .child(connection_details(shell));
    if let Some(feedback) = &shell.state.diagnostic {
        page = page.child(components::technical_feedback(
            "drip-diagnostic",
            "Application diagnostics",
            feedback,
        ));
    }
    if let Some(feedback) = &shell.state.settings_feedback {
        page = page.child(components::technical_feedback(
            "activity-settings-feedback",
            "Settings feedback",
            feedback,
        ));
    }
    if let Some(feedback) = &shell.state.watch_feedback {
        page = page.child(components::technical_feedback(
            "activity-watch-feedback",
            "Watch feedback",
            feedback,
        ));
    }
    if let Some(progress) = &shell.state.progress {
        page = page.child(current_transfer(progress));
    }
    if !shell.state.settings.selections.is_empty() {
        page = page.child(selection_details(shell));
    }
    let mut list = div()
        .id("drip-log-list")
        .tab_group()
        .flex()
        .flex_col()
        .gap_3();
    if shell.state.logs.is_empty() {
        list = list.child(
            div()
                .id("logs-empty")
                .text_sm()
                .text_color(rgb(theme::MUTED))
                .child("No activity recorded."),
        );
    } else {
        for row in shell.state.logs.iter().rev() {
            list = list.child(log_row(row, window, cx));
        }
    }
    page.child(components::card_heading("Recent activity"))
        .child(list)
}
