use gpui::{Context, CursorStyle, MouseButton, Window, div, prelude::*, px, rgb};

use super::{element::TextElement, model::Input, text};

impl Render for Input {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.pending_native_change {
            _window.notify_text_input_changed(&self.focus_handle);
            _window.notify_text_selection_changed(&self.focus_handle);
            self.pending_native_change = false;
        }
        div()
            .id(format!("drip-input-{}", self.label))
            .key_context("DripInput")
            .track_focus(&self.focus_handle)
            .focus_visible(|style| style.border_color(rgb(0x38bdf8)))
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .w_full()
            .px_3()
            .py_2()
            .bg(rgb(0x18212b))
            .border_2()
            .border_color(rgb(0x516070))
            .rounded(px(4.))
            .overflow_hidden()
            .child(TextElement { input: cx.entity() })
    }
}

pub(super) fn display_range(input: &Input) -> std::ops::Range<usize> {
    text::display_offset(&input.content, input.selected_range.start, input.secret)
        ..text::display_offset(&input.content, input.selected_range.end, input.secret)
}
