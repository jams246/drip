use gpui::{ClickEvent, Context, IntoElement, Window, div, prelude::*, px, rgb};

use crate::gui::{focus, input::Input, model::Shell, theme};

#[derive(Clone, Copy)]
pub(in crate::gui::views) enum ButtonKind {
    Primary,
    Secondary,
}

pub(in crate::gui::views) fn button(
    cx: &mut Context<Shell>,
    window: &mut Window,
    id: impl Into<gpui::SharedString>,
    label: impl Into<gpui::SharedString>,
    kind: ButtonKind,
    enabled: bool,
    on_click: impl Fn(&mut Shell, &ClickEvent, &mut Window, &mut Context<Shell>) + 'static,
) -> impl IntoElement {
    let id = id.into();
    let label = label.into();

    let primary = matches!(kind, ButtonKind::Primary);
    let focus = focus::handle(&id, enabled, window, cx);
    let button = focus::reveal(div(), &id, focus.clone(), window, cx)
        .track_focus(&focus)
        .id(id.clone())
        .tab_index(0)
        .tab_stop(enabled)
        .flex()
        .items_center()
        .justify_center()
        .min_h(px(36.))
        .px_4()
        .rounded(px(6.))
        .border_1()
        .border_color(rgb(if primary {
            theme::ACCENT
        } else {
            theme::BORDER
        }))
        .bg(rgb(if primary { theme::ACCENT } else { theme::PANEL }))
        .text_color(rgb(if primary { theme::PAGE } else { theme::TEXT }))
        .focus(|style| style.border_color(rgb(theme::TEXT)))
        .hover(|style| style.bg(rgb(if primary { 0xb5c6f7 } else { 0x282c34 })))
        .child(label);
    if enabled {
        button.on_click(cx.listener(on_click)).cursor_pointer()
    } else {
        button
    }
}

pub(in crate::gui::views) fn startup_checkbox(
    cx: &mut Context<Shell>,
    window: &mut Window,
    checked: bool,
    enabled: bool,
) -> impl IntoElement {
    let focus = focus::handle("startup", enabled, window, cx);
    let checkbox = focus::reveal(div(), "startup", focus.clone(), window, cx)
        .track_focus(&focus)
        .id("drip-startup-setting")
        .tab_index(0)
        .tab_stop(enabled)
        .flex()
        .items_center()
        .gap_2()
        .min_h(px(36.))
        .text_color(rgb(theme::TEXT))
        .border_1()
        .border_color(gpui::transparent_black())
        .rounded(px(6.))
        .focus(|style| style.border_color(rgb(theme::ACCENT)))
        .child(
            div()
                .w(px(20.))
                .h(px(20.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.))
                .border_1()
                .border_color(rgb(if checked { theme::ACCENT } else { theme::MUTED }))
                .text_color(rgb(theme::ACCENT))
                .child(if checked { "✓" } else { "" }),
        )
        .child("Start at sign-in");
    if enabled {
        checkbox
            .on_click(cx.listener(|shell, _, window, cx| {
                shell.toggle_startup(window, cx);
            }))
            .on_key_down(
                cx.listener(|shell, event: &gpui::KeyDownEvent, window, cx| {
                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                        cx.stop_propagation();
                        shell.toggle_startup(window, cx);
                    }
                }),
            )
    } else {
        checkbox
    }
}

pub(in crate::gui::views) fn field(
    label: &'static str,
    stable_id: &'static str,
    input: gpui::Entity<Input>,
) -> impl IntoElement {
    div()
        .id(format!("field-{stable_id}"))
        .flex()
        .flex_col()
        .gap_2()
        .child(div().text_sm().text_color(rgb(theme::TEXT)).child(label))
        .child(input)
}
