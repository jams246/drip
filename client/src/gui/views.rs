mod components;
mod home;
mod logs;
mod settings;
mod watch;

use std::time::Instant;

use gpui::{Context, Render, Window, div, prelude::*, px, rgb};

use super::model::{Shell, Tab};
use super::{NextFocus, Pause, PreviousFocus, Quit};

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tab = self.tab;
        let mut panel = div()
            .id("drip-tab-panel")
            .track_focus(&self.focus)
            .tab_index(0)
            .flex_1()
            .overflow_y_scroll()
            .child(match tab {
                Tab::Home => home::render(self).into_any_element(),
                Tab::Watch => watch::render(self, cx).into_any_element(),
                Tab::Logs => logs::render(self).into_any_element(),
                Tab::Settings => settings::render(self, cx).into_any_element(),
            });
        if !self.state.diagnostic.is_empty() {
            panel = panel.child(components::diagnostic(&self.state.diagnostic));
        }
        div()
            .id("drip-shell")
            .on_action(cx.listener(|_, _: &NextFocus, window, cx| window.focus_next(cx)))
            .on_action(cx.listener(|_, _: &PreviousFocus, window, cx| window.focus_prev(cx)))
            .on_action(cx.listener(|shell, _: &Quit, window, cx| shell.quit(window, cx)))
            .on_action(cx.listener(|shell, _: &Pause, window, cx| shell.pause(window, cx)))
            .size_full()
            .flex()
            .flex_col()
            .p_6()
            .gap_4()
            .bg(rgb(0x111820))
            .text_color(rgb(0xe9eef4))
            .child(heading())
            .child(navigation(self, cx))
            .child(panel)
            .child(components::footer(self.footer_label(Instant::now())))
    }
}

fn heading() -> impl IntoElement {
    div()
        .id("drip-heading")
        .text_xl()
        .font_weight(gpui::FontWeight::BOLD)
        .child("DRIP - Directed Remote Incremental Push")
}

fn navigation(shell: &mut Shell, cx: &mut Context<Shell>) -> impl IntoElement {
    div()
        .id("drip-tabs")
        .flex()
        .gap_2()
        .children(Tab::ALL.into_iter().map(|tab| {
            let selected = shell.tab == tab;
            let index = tab.index();
            shell.tab_focus[index] = shell.tab_focus[index].clone().tab_stop(selected);
            div()
                .id(format!("drip-tab-{}", tab.label().to_lowercase()))
                .track_focus(&shell.tab_focus[index])
                .tab_index(0)
                .tab_stop(selected)
                .px_4()
                .py_2()
                .rounded(px(4.))
                .border_1()
                .border_color(if selected {
                    rgb(0x55b7a8)
                } else {
                    rgb(0x354452)
                })
                .bg(if selected {
                    rgb(0x1d3438)
                } else {
                    rgb(0x18212b)
                })
                .text_color(if selected {
                    rgb(0xeaf8f5)
                } else {
                    rgb(0xb9c5cf)
                })
                .focus(|style| style.border_color(rgb(0x8be0d3)))
                .hover(|style| style.bg(rgb(0x263743)))
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
        }))
}
