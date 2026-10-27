use std::ops::Range;

use gpui::{
    App, Bounds, Context, FocusHandle, Focusable, Pixels, ShapedLine, SharedString, Window,
};

pub(in crate::gui) struct Input {
    pub(super) label: SharedString,
    pub(super) secret: bool,
    pub(super) focus_handle: FocusHandle,
    pub(super) content: String,
    pub(super) selected_range: Range<usize>,
    pub(super) selection_reversed: bool,
    pub(super) marked_range: Option<Range<usize>>,
    pub(super) last_layout: Option<ShapedLine>,
    pub(super) last_bounds: Option<Bounds<Pixels>>,
    pub(super) horizontal_offset: Pixels,
    pub(super) is_selecting: bool,
    pub(super) pending_native_change: bool,
}

impl Input {
    pub(in crate::gui) fn new(
        label: impl Into<SharedString>,
        secret: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            label: label.into(),
            secret,
            focus_handle: cx.focus_handle().tab_stop(true),
            content: String::new(),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_layout: None,
            last_bounds: None,
            horizontal_offset: gpui::px(0.),
            is_selecting: false,
            pending_native_change: false,
        }
    }

    pub(in crate::gui) fn value(&self) -> &str {
        &self.content
    }

    pub(super) fn display_value(&self) -> SharedString {
        if self.secret {
            super::text::mask_graphemes(&self.content).into()
        } else {
            self.content.clone().into()
        }
    }

    pub(in crate::gui) fn set_value(&mut self, value: impl Into<String>, cx: &mut Context<Self>) {
        self.content = value.into();
        let end = self.content.len();
        self.selected_range = end..end;
        self.selection_reversed = false;
        self.marked_range = None;
        self.pending_native_change = true;
        cx.notify();
    }

    pub(in crate::gui) fn clear(&mut self, cx: &mut Context<Self>) {
        self.set_value(String::new(), cx);
    }

    pub(super) fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    pub(super) fn move_to(&mut self, offset: usize, window: &Window, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        window.notify_text_selection_changed(&self.focus_handle);
        cx.notify();
    }

    pub(super) fn select_to(&mut self, offset: usize, window: &Window, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset;
        } else {
            self.selected_range.end = offset;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        window.notify_text_selection_changed(&self.focus_handle);
        cx.notify();
    }

    pub(super) fn previous_boundary(&self, offset: usize) -> usize {
        super::text::previous_boundary(&self.content, offset)
    }

    pub(super) fn next_boundary(&self, offset: usize) -> usize {
        super::text::next_boundary(&self.content, offset)
    }
}

impl Focusable for Input {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}
