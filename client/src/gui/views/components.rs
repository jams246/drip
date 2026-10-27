use gpui::{ClickEvent, Context, IntoElement, Window, div, prelude::*, px, rgb};

use super::super::model::Shell;

pub(super) fn button(
    cx: &mut Context<Shell>,
    id: impl Into<gpui::ElementId>,
    label: impl Into<gpui::SharedString>,
    enabled: bool,
    on_click: impl Fn(&mut Shell, &ClickEvent, &mut Window, &mut Context<Shell>) + 'static,
) -> impl IntoElement {
    let label = label.into();

    let button = div()
        .id(id)
        .tab_index(0)
        .tab_stop(enabled)
        .px_3()
        .py_2()
        .rounded(px(4.))
        .border_1()
        .border_color(if enabled {
            rgb(0x526578)
        } else {
            rgb(0x343c45)
        })
        .bg(if enabled {
            rgb(0x202b36)
        } else {
            rgb(0x171e25)
        })
        .text_color(if enabled {
            rgb(0xe9eef4)
        } else {
            rgb(0x77828d)
        })
        .focus(|style| style.border_color(rgb(0x8be0d3)))
        .hover(|style| style.bg(rgb(0x283a48)))
        .child(label);
    if enabled {
        button.on_click(cx.listener(on_click)).cursor_pointer()
    } else {
        button
    }
}

pub(super) fn startup_checkbox(
    cx: &mut Context<Shell>,
    checked: bool,
    enabled: bool,
) -> impl IntoElement {
    let label = if checked && enabled {
        "Start at sign-in, checked"
    } else if checked {
        "Start at sign-in, checked, unavailable while Drip is loading or busy"
    } else if enabled {
        "Start at sign-in, not checked"
    } else {
        "Start at sign-in, not checked, unavailable while Drip is loading or busy"
    };
    let checkbox = div()
        .id("drip-startup-setting")
        .tab_index(0)
        .tab_stop(enabled)
        .flex()
        .items_center()
        .gap_2()
        .px_3()
        .py_2()
        .rounded(px(4.))
        .border_1()
        .border_color(if enabled {
            rgb(0x526578)
        } else {
            rgb(0x343c45)
        })
        .bg(if enabled {
            rgb(0x202b36)
        } else {
            rgb(0x171e25)
        })
        .text_color(if enabled {
            rgb(0xe9eef4)
        } else {
            rgb(0x77828d)
        })
        .focus(|style| style.border_color(rgb(0x8be0d3)))
        .child(if checked { "☑" } else { "☐" })
        .child("Start at sign-in");
    if enabled {
        checkbox.on_click(cx.listener(|shell, _, window, cx| {
            shell.toggle_startup(window, cx);
        }))
    } else {
        checkbox
    }
}

pub(super) fn footer(connection: &str) -> impl IntoElement {
    div()
        .id("drip-footer")
        .w_full()
        .flex()
        .justify_between()
        .border_t_1()
        .border_color(rgb(0x354452))
        .pt_3()
        .text_sm()
        .child(connection.to_owned())
        .child(format!("Version {}", env!("CARGO_PKG_VERSION")))
}

pub(super) fn diagnostic(message: &str) -> impl IntoElement {
    div()
        .id("drip-diagnostic")
        .text_color(rgb(0xffc07a))
        .child(message.to_owned())
}

pub(super) fn field(
    label: &'static str,
    input: gpui::Entity<super::super::input::Input>,
) -> impl IntoElement {
    div()
        .id(format!("field-{}", label.to_lowercase().replace(' ', "-")))
        .flex()
        .flex_col()
        .gap_2()
        .child(div().child(label))
        .child(input)
}

pub(super) fn section(title: &'static str) -> gpui::Div {
    div().flex().flex_col().gap_3().child(
        div()
            .id(format!(
                "section-heading-{}",
                title.to_lowercase().replace(' ', "-")
            ))
            .text_lg()
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .child(title),
    )
}

pub(super) fn message(id: &'static str, text: &str) -> impl IntoElement {
    div()
        .id(id)
        .text_sm()
        .text_color(rgb(0xc5d2dc))
        .child(text.to_owned())
}
