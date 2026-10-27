use std::ops::Range;

use gpui::{Bounds, Context, EntityInputHandler, Pixels, UTF16Selection, Window};

use super::{model::Input, text};

impl EntityInputHandler for Input {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = text::normalize_range(
            &self.content,
            text::range_from_utf16(&self.content, &range_utf16),
        );
        actual_range.replace(text::range_to_utf16(&self.content, &range));
        let value = &self.content[range];
        Some(if self.secret {
            text::mask_utf16(value)
        } else {
            value.to_owned()
        })
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: text::range_to_utf16(&self.content, &self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn set_selected_text_range(
        &mut self,
        range_utf16: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selected_range = text::normalize_range(
            &self.content,
            text::range_from_utf16(&self.content, &range_utf16),
        );
        self.selection_reversed = false;
        window.notify_text_selection_changed(&self.focus_handle);
        cx.notify();
    }

    fn text_length_utf16(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.content.encode_utf16().count())
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| text::range_to_utf16(&self.content, range))
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.marked_range = None;
        window.notify_text_input_changed(&self.focus_handle);
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let new_text = text::single_line(new_text);
        let range = range_utf16
            .as_ref()
            .map(|range| text::range_from_utf16(&self.content, range))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.selected_range.clone());
        self.replace(range, &new_text);
        window.notify_text_input_changed(&self.focus_handle);
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let new_text = text::single_line(new_text);
        let range = range_utf16
            .as_ref()
            .map(|range| text::range_from_utf16(&self.content, range))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.selected_range.clone());
        let range = text::normalize_range(&self.content, range);
        let insertion_start = range.start;
        self.content.replace_range(range, &new_text);
        (self.marked_range, self.selected_range) = text::marked_ranges(
            &self.content,
            insertion_start,
            &new_text,
            new_selected_range_utf16,
        );
        self.selection_reversed = false;
        window.notify_text_input_changed(&self.focus_handle);
        window.notify_text_selection_changed(&self.focus_handle);
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let line = self.last_layout.as_ref()?;
        if line.text != self.display_value() {
            return None;
        }
        let source_range = text::normalize_range(
            &self.content,
            text::range_from_utf16(&self.content, &range_utf16),
        );
        let start = text::display_offset(&self.content, source_range.start, self.secret);
        let end = text::display_offset(&self.content, source_range.end, self.secret);
        let start_x = clamp_x(
            bounds.left() + line.x_for_index(start) - self.horizontal_offset,
            &bounds,
        );
        let end_x = clamp_x(
            bounds.left() + line.x_for_index(end) - self.horizontal_offset,
            &bounds,
        );
        Some(Bounds::from_corners(
            gpui::point(start_x, bounds.top()),
            gpui::point(end_x, bounds.bottom()),
        ))
    }

    fn selection_bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Bounds<Pixels>> {
        self.bounds_for_range(range_utf16, bounds, window, cx)
            .into_iter()
            .collect()
    }

    fn character_index_for_point(
        &mut self,
        point: gpui::Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let line = self.last_layout.as_ref()?;
        let line_point = self.last_bounds?.localize(&point)?;
        if line.text != self.display_value() {
            return None;
        }
        let x = line_point.x + self.horizontal_offset;
        if self.secret {
            let offset = line.closest_index_for_x(x);
            let source = text::source_offset(&self.content, offset, true);
            Some(text::byte_to_utf16(&self.content, source))
        } else {
            let offset = line.closest_index_for_x(x);
            let source = text::nearest_boundary(&self.content, offset);
            Some(text::byte_to_utf16(&self.content, source))
        }
    }
}

fn clamp_x(x: Pixels, bounds: &Bounds<Pixels>) -> Pixels {
    if x < bounds.left() {
        bounds.left()
    } else if x > bounds.right() {
        bounds.right()
    } else {
        x
    }
}

impl Input {
    fn replace(&mut self, range: Range<usize>, new_text: &str) {
        let range = text::normalize_range(&self.content, range);
        self.content.replace_range(range.clone(), new_text);
        let end = text::ceil_boundary(&self.content, range.start + new_text.len());
        self.selected_range = end..end;
        self.selection_reversed = false;
        self.marked_range = None;
    }
}
