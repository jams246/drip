mod controls;
mod feedback;
mod icons;

use gpui::{ElementId, IntoElement, Stateful, div, prelude::*, px, rgb};

use super::super::theme;

pub(super) use controls::{ButtonKind, button, field, startup_checkbox};
pub(super) use feedback::{
    activity_link, summary as feedback_summary, technical as technical_feedback,
};
pub(super) use icons::{Glyph, icon};

pub(super) fn footer(connection: &str) -> impl IntoElement {
    let color = match connection {
        "Connected" => theme::SUCCESS,
        "Paused" => theme::WARNING,
        _ => theme::ERROR,
    };
    let marker = match connection {
        "Connected" => div()
            .w(px(8.))
            .h(px(8.))
            .rounded_full()
            .bg(rgb(color))
            .into_any_element(),
        "Paused" => div().text_color(rgb(color)).child("Ⅱ").into_any_element(),
        _ => div().text_color(rgb(color)).child("×").into_any_element(),
    };
    div()
        .id("drip-footer")
        .tab_index(0)
        .focus(|style| style.border_color(rgb(theme::ACCENT)))
        .w_full()
        .flex()
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
                .gap_2()
                .px_3()
                .py_2()
                .rounded(px(6.))
                .border_1()
                .border_color(rgb(color))
                .text_color(rgb(color))
                .child(marker)
                .child(connection.to_owned()),
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

pub(super) fn page_header(title: &'static str, subtitle: Option<&'static str>) -> gpui::Div {
    let heading = div()
        .id(format!("page-heading-{}", title.to_lowercase()))
        .text_size(px(32.))
        .font_weight(gpui::FontWeight::BOLD)
        .text_color(rgb(theme::TEXT))
        .child(title);
    let page = div().flex().flex_col().gap_2().child(heading);
    if let Some(subtitle) = subtitle {
        page.child(
            div()
                .text_size(px(16.))
                .text_color(rgb(theme::MUTED))
                .child(subtitle),
        )
    } else {
        page
    }
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
