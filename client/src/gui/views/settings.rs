use gpui::{Context, div, prelude::*, rgb};

use super::super::model::Shell;
use super::components;

pub(super) fn render(shell: &Shell, cx: &mut Context<Shell>) -> impl IntoElement {
    let enabled = !shell.state.loading && !shell.state.busy && !shell.state.quitting;
    let mut page = components::section("Settings")
        .id("drip-settings")
        .gap_4()
        .child(components::field("Server URL", shell.server.clone()))
        .child(components::field("Registration token", shell.token.clone()))
        .child(components::field("PC display name", shell.pc_name.clone()))
        .child(components::button(
            cx,
            "settings-pair",
            "Pair",
            enabled,
            |shell, _, window, cx| {
                shell.pair(window, cx);
            },
        ))
        .child(components::startup_checkbox(
            cx,
            shell.state.settings.start_at_sign_in,
            enabled,
        ));
    if !shell.state.settings_message.is_empty() {
        page = page.child(components::message(
            "settings-message",
            &shell.state.settings_message,
        ));
    }
    if let Some(binding) = &shell.state.binding {
        page = page.child(
            div()
                .id("settings-binding")
                .flex()
                .flex_col()
                .gap_1()
                .pt_3()
                .border_t_1()
                .border_color(rgb(0x354452))
                .child(div().child(format!("Server: {}", binding.server)))
                .child(div().child(format!("PC ID: {}", binding.pc_id)))
                .child(div().child(format!("Mirror directory: {}", binding.mirror_directory))),
        );
    }
    page
}
