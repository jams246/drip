mod controls;
mod feedback;
mod icons;
mod watched;

use gpui::{ElementId, IntoElement, Stateful, div, prelude::*, px, rgb};

use super::super::theme;

pub(super) use controls::{ButtonKind, button, field, startup_checkbox};
pub(super) use feedback::{
    activity_link, summary as feedback_summary, technical as technical_feedback,
};
pub(super) use icons::{Glyph, icon};
pub(super) use watched::{list as watched_list, row as watched_row};

pub(super) fn footer(connection: &str) -> impl IntoElement {
    let color = match connection {
        "Connected" => theme::SUCCESS,
        "Paused" => theme::WARNING,
        _ => theme::ERROR,
    };
    div()
        .id("drip-footer")
        .w_full()
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_between()
        .border_t_1()
        .border_color(rgb(theme::BORDER))
        .px_8()
        .py_3()
        .bg(rgb(theme::PANEL))
        .child(
            div()
                .flex()
                .items_center()
                .gap_3()
                .h(px(44.))
                .px_4()
                .rounded_full()
                .bg(rgb(color))
                .text_color(rgb(theme::PAGE))
                .text_size(px(16.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(
                    div()
                        .flex_shrink_0()
                        .w(px(10.))
                        .h(px(10.))
                        .rounded_full()
                        .bg(rgb(theme::PAGE)),
                )
                .child(
                    div()
                        .relative()
                        .top(px(-2.))
                        .line_height(px(20.))
                        .child(connection.to_owned()),
                ),
        )
        .child(
            div()
                .flex()
                .gap_2()
                .text_sm()
                .text_color(rgb(theme::MUTED))
                .child("DRIP")
                .child(env!("CARGO_PKG_VERSION")),
        )
}

pub(super) fn message(id: &'static str, text: &str) -> impl IntoElement {
    div()
        .id(id)
        .text_sm()
        .text_color(rgb(theme::MUTED))
        .child(text.to_owned())
}

pub(super) fn card(id: impl Into<ElementId>) -> Stateful<gpui::Div> {
    div()
        .id(id)
        .w_full()
        .min_w_0()
        .flex()
        .flex_col()
        .gap_3()
        .p_6()
        .rounded(px(12.))
        .border_1()
        .border_color(rgb(theme::BORDER))
        .bg(rgb(theme::PANEL))
}

pub(super) fn card_heading(title: &'static str) -> impl IntoElement {
    div()
        .id(format!("card-heading-{}", title.to_lowercase()))
        .text_size(px(18.))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(rgb(theme::TEXT))
        .child(title)
}

pub(super) fn breakable_path(path: &str) -> String {
    path.replace('\\', "\\\u{200b}").replace('/', "/\u{200b}")
}
