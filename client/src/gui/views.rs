mod components;
mod home;
mod logs;
mod settings;
mod watch;

use std::time::Instant;

use gpui::{
    Context, HighlightStyle, Render, StyledText, UnderlineStyle, Window, div, prelude::*, px, rgb,
};

use super::model::{Shell, Tab};
use super::{NextFocus, Pause, PreviousFocus, Quit, theme};

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tab = self.tab;
        let mut content = match tab {
            Tab::Home => home::render(self, cx).into_any_element(),
            Tab::Watch => watch::render(self, cx).into_any_element(),
            Tab::Activity => logs::render(self).into_any_element(),
            Tab::Settings => settings::render(self, cx).into_any_element(),
        };
        if tab != Tab::Activity
            && let Some(diagnostic) = &self.state.diagnostic
        {
            content = div()
                .w_full()
                .flex()
                .flex_col()
                .gap_4()
                .child(content)
                .child(components::feedback_summary(
                    cx,
                    "drip-diagnostic-summary",
                    diagnostic,
                ))
                .into_any_element();
        }
        let panel = div()
            .id("drip-tab-panel")
            .track_focus(&self.focus)
            .tab_index(0)
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .bg(rgb(theme::PAGE))
            .px_8()
            .py_7()
            .child(content);
        div()
            .id("drip-shell")
            .on_action(cx.listener(|_, _: &NextFocus, window, cx| window.focus_next(cx)))
            .on_action(cx.listener(|_, _: &PreviousFocus, window, cx| window.focus_prev(cx)))
            .on_action(cx.listener(|shell, _: &Quit, window, cx| shell.quit(window, cx)))
            .on_action(cx.listener(|shell, _: &Pause, window, cx| shell.pause(window, cx)))
            .size_full()
            .flex()
            .flex_col()
            .font_family("Segoe UI")
            .bg(rgb(theme::PAGE))
            .text_color(rgb(theme::TEXT))
            .child(heading())
            .child(navigation(self, cx))
            .child(panel)
            .child(components::footer(self.footer_label(Instant::now())))
    }
}

fn heading() -> impl IntoElement {
    let underline = HighlightStyle {
        underline: Some(UnderlineStyle {
            color: Some(rgb(theme::ACCENT).into()),
            thickness: px(1.),
            wavy: false,
        }),
        ..Default::default()
    };
    let subtitle = StyledText::new("Directed Remote Incremental Push").with_highlights([
        (0..1, underline),
        (9..10, underline),
        (16..17, underline),
        (28..29, underline),
    ]);
    div()
        .id("drip-heading")
        .flex()
        .items_center()
        .gap_4()
        .px_8()
        .py_5()
        .border_b_1()
        .border_color(rgb(theme::BORDER))
        .bg(rgb(theme::PANEL))
        .child(components::icon(components::Glyph::Brand, 44.))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .id("drip-title")
                        .text_size(px(32.))
                        .font_weight(gpui::FontWeight::BOLD)
                        .child("DRIP"),
                )
                .child(
                    div()
                        .text_size(px(16.))
                        .text_color(rgb(theme::MUTED))
                        .child(subtitle),
                ),
        )
}

fn navigation(shell: &mut Shell, cx: &mut Context<Shell>) -> impl IntoElement {
    let tabs = Tab::ALL.into_iter().map(|tab| {
        let selected = shell.tab == tab;
        let index = tab.index();
        let id = tab.id();
        shell.tab_focus[index] = shell.tab_focus[index].clone().tab_stop(selected);
        div()
            .id(format!("drip-tab-{id}"))
            .track_focus(&shell.tab_focus[index])
            .tab_index(0)
            .tab_stop(selected)
            .flex()
            .items_center()
            .min_h(px(44.))
            .px_5()
            .rounded_t(px(8.))
            .border_1()
            .when(selected, |tab| tab.border_b_0().mb(px(-1.)))
            .border_color(rgb(theme::BORDER))
            .bg(rgb(if selected { theme::PAGE } else { theme::PANEL }))
            .text_color(rgb(if selected { theme::TEXT } else { theme::MUTED }))
            .font_weight(if selected {
                gpui::FontWeight::BOLD
            } else {
                gpui::FontWeight::NORMAL
            })
            .focus(|style| style.border_color(rgb(theme::ACCENT)))
            .hover(|style| style.bg(rgb(if selected { theme::PAGE } else { 0x282c34 })))
            .on_click(cx.listener(move |shell, _, window, cx| {
                shell.select_tab(tab, window, cx);
            }))
            .on_key_down(
                cx.listener(move |shell, event: &gpui::KeyDownEvent, window, cx| {
                    let next = match event.keystroke.key.as_str() {
                        "left" => Some(
                            Tab::ALL[(shell.tab.index() + Tab::ALL.len() - 1) % Tab::ALL.len()],
                        ),
                        "right" => Some(Tab::ALL[(shell.tab.index() + 1) % Tab::ALL.len()]),
                        _ => None,
                    };
                    if let Some(next) = next {
                        cx.stop_propagation();
                        shell.select_tab(next, window, cx);
                    }
                }),
            )
            .child(tab.label())
    });
    div()
        .id("drip-tabs")
        .relative()
        .flex()
        .items_end()
        .gap_1()
        .px_8()
        .bg(rgb(theme::PANEL))
        .child(
            div()
                .absolute()
                .bottom_0()
                .left_0()
                .right_0()
                .h(px(1.))
                .bg(rgb(theme::BORDER)),
        )
        .children(tabs)
}
