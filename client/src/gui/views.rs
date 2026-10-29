mod components;
mod heading;
mod home;
mod logs;
mod settings;
mod watch;

use std::time::Instant;

use gpui::{Context, Render, Window, div, prelude::*, px, relative, rgb};

use super::model::{Shell, Tab};
use super::{NextFocus, Pause, PreviousFocus, Quit, theme};

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tab = self.tab;
        let mut content = match tab {
            Tab::Home => home::render(self, window, cx).into_any_element(),
            Tab::Watch => watch::render(self, window, cx).into_any_element(),
            Tab::Activity => logs::render(self, window, cx).into_any_element(),
            Tab::Settings => settings::render(self, window, cx).into_any_element(),
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
                    window,
                    "drip-diagnostic-summary",
                    diagnostic,
                ))
                .into_any_element();
        }
        let scroll = self.tab_scroll[tab.index()].clone();
        let panel_scroll = scroll.clone();
        let scrollbar =
            super::scrollbar::render(scroll.clone(), tab.index(), window, cx).into_any_element();
        let shell = cx.entity().downgrade();
        let panel = div()
            .on_children_prepainted(move |_, window, _cx| {
                let Some(target) = window.take_autoscroll() else {
                    return;
                };
                let mut offset = scroll.offset();
                let next = super::focus::scroll_offset(
                    offset.y,
                    scroll.max_offset().y,
                    scroll.bounds(),
                    target,
                );
                if next != offset.y {
                    offset.y = next;
                    let scroll = scroll.clone();
                    let shell = shell.clone();
                    window.on_next_frame(move |_, cx| {
                        scroll.set_offset(offset);
                        let _ = shell.update(cx, |_, cx| cx.notify());
                    });
                }
            })
            .id("drip-tab-panel")
            .track_focus(&self.focus)
            .tab_index(0)
            .flex()
            .flex_col()
            .flex_1()
            .h_full()
            .min_w_0()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&panel_scroll)
            .bg(rgb(theme::PAGE))
            .px_8()
            .py_8()
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
            .text_size(px(14.))
            .line_height(relative(1.4))
            .bg(rgb(theme::PAGE))
            .text_color(rgb(theme::TEXT))
            .child(heading::render())
            .child(navigation(self, cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .h(px(0.))
                    .min_h_0()
                    .child(panel)
                    .child(scrollbar),
            )
            .child(components::footer(self.footer_label(Instant::now())))
    }
}

fn navigation(shell: &mut Shell, cx: &mut Context<Shell>) -> impl IntoElement {
    let tabs = Tab::ALL.into_iter().map(|tab| {
        let selected = shell.tab == tab;
        let index = tab.index();
        let id = tab.id();
        shell.tab_focus[index] = shell.tab_focus[index].clone().tab_stop(selected);
        let button = div()
            .id(format!("drip-tab-{id}"))
            .track_focus(&shell.tab_focus[index])
            .tab_index(0)
            .tab_stop(selected)
            .flex()
            .items_center()
            .min_h(px(44.))
            .px_5()
            .rounded_t(px(8.))
            .border_b_1()
            .border_color(rgb(theme::BORDER))
            .when(selected, |tab| {
                tab.border_t_1()
                    .border_l_1()
                    .border_r_1()
                    .border_b_0()
                    .px(px(19.))
            })
            .bg(rgb(if selected { theme::PAGE } else { theme::PANEL }))
            .text_color(rgb(if selected { theme::TEXT } else { theme::MUTED }))
            .font_weight(if selected {
                gpui::FontWeight::BOLD
            } else {
                gpui::FontWeight::NORMAL
            })
            .focus_visible(|style| style.border_color(rgb(theme::ACCENT)))
            .cursor_pointer()
            .hover(|style| style.bg(rgb(if selected { theme::PAGE } else { 0x282c34 })))
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(move |shell, _, window, cx| {
                    shell.select_tab(tab, window, cx);
                }),
            )
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
            .child(tab.label());
        div()
            .flex()
            .items_end()
            .child(button)
            .child(div().w(px(4.)).h(px(1.)).bg(rgb(theme::BORDER)))
    });
    div()
        .id("drip-tabs")
        .flex()
        .flex_shrink_0()
        .items_end()
        .bg(rgb(theme::PANEL))
        .child(div().w(px(32.)).h(px(1.)).bg(rgb(theme::BORDER)))
        .children(tabs)
        .child(
            div()
                .flex_1()
                .min_w(px(32.))
                .h(px(1.))
                .bg(rgb(theme::BORDER)),
        )
}
