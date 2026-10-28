use gpui::{Context, div, prelude::*, rgb};

use super::super::model::Shell;
use super::components::{self, ButtonKind};
use super::theme;

pub(super) fn render(shell: &Shell, cx: &mut Context<Shell>) -> impl IntoElement {
    let enabled = !shell.state.loading && !shell.state.busy && !shell.state.quitting;
    let mut server = components::card(
        "settings-server-card")
    .child(components::card_heading("Your server"))
    .child(
        div()
            .text_sm()
            .text_color(rgb(theme::MUTED))
            .child("Files travel one way, from this computer to your server."),
    )
    .child(components::field(
        "Server address",
        "server-url",
        shell.server.clone(),
    ))
    .child(components::field(
        "Connection token",
        "registration-token",
        shell.token.clone(),
    ))
    .child(
        div()
            .text_sm()
            .text_color(rgb(theme::MUTED))
            .child("Use a one-time token from your server."),
    )
    .child(components::field(
        "Computer name",
        "pc-display-name",
        shell.pc_name.clone(),
    ))
    .child(div().text_sm().text_color(rgb(theme::MUTED)).child(
        "Connecting again creates a new computer registration and keeps existing server copies.",
    ))
    .child(div().flex().justify_start().child(components::button(
        cx,
        "settings-pair",
        if shell.state.connecting {
            "Connecting..."
        } else {
            "Connect"
        },
        ButtonKind::Primary,
        enabled,
        |shell, _, window, cx| shell.pair(window, cx),
    )));
    if let Some(feedback) = &shell.state.settings_feedback {
        server = server.child(components::feedback_summary(
            cx,
            "settings-message",
            feedback,
        ));
    }
    let this_computer = components::card("settings-computer-card")
        .child(components::card_heading("This computer"))
        .child(components::startup_checkbox(
            cx,
            shell.state.settings.start_at_sign_in,
            enabled,
        ));
    div()
        .id("drip-settings")
        .w_full()
        .flex()
        .flex_col()
        .gap_6()
        .child(components::page_header("Settings", None))
        .child(server)
        .child(this_computer)
}
