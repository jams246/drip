use gpui::{Context, Window, div, prelude::*, rgb};

use crate::gui::{
    focus,
    model::{Feedback, FeedbackSeverity, Shell, Tab},
    theme,
};

pub(in crate::gui::views) fn technical(
    id: &'static str,
    heading: &'static str,
    feedback: &Feedback,
) -> impl IntoElement {
    let color = color(feedback.severity);
    let mut view = div()
        .id(id)
        .flex()
        .flex_col()
        .gap_1()
        .p_4()
        .rounded(gpui::px(8.))
        .border_1()
        .border_color(rgb(theme::BORDER))
        .bg(rgb(theme::PANEL))
        .text_color(rgb(color))
        .child(div().text_sm().child(heading))
        .child(feedback.summary.clone());
    if let Some(detail) = &feedback.detail {
        view = view.child(
            div()
                .text_sm()
                .text_color(rgb(theme::MUTED))
                .child(super::breakable_path(detail)),
        );
    }
    view
}

pub(in crate::gui::views) fn activity_link(
    cx: &mut Context<Shell>,
    window: &mut Window,
    id: &str,
    label: &'static str,
) -> impl IntoElement {
    let focus = focus::handle(id, true, window, cx);
    focus::reveal(div(), id, focus.clone(), window, cx)
        .track_focus(&focus)
        .id("drip-view-activity")
        .tab_index(0)
        .tab_stop(true)
        .border_1()
        .border_color(rgb(theme::ACCENT))
        .text_color(rgb(theme::ACCENT))
        .focus(|style| style.border_color(rgb(theme::TEXT)))
        .on_click(cx.listener(|shell, _, window, cx| {
            shell.select_tab(Tab::Activity, window, cx);
        }))
        .on_key_down(
            cx.listener(|shell, event: &gpui::KeyDownEvent, window, cx| {
                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                    cx.stop_propagation();
                    shell.select_tab(Tab::Activity, window, cx);
                }
            }),
        )
        .child(label)
}

fn color(severity: FeedbackSeverity) -> u32 {
    match severity {
        FeedbackSeverity::Info => theme::MUTED,
        FeedbackSeverity::Warning => theme::WARNING,
        FeedbackSeverity::Error => theme::ERROR,
    }
}
