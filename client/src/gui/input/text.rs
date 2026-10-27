use std::borrow::Cow;
use std::ops::Range;

use gpui::{Pixels, px};
use unicode_segmentation::UnicodeSegmentation;

const BULLET: &str = "•";

pub(super) fn horizontal_offset(
    caret_x: Pixels,
    line_width: Pixels,
    viewport_width: Pixels,
    current: Pixels,
) -> Pixels {
    let mut offset = if caret_x < current {
        caret_x
    } else if caret_x + px(2.) > current + viewport_width {
        caret_x + px(2.) - viewport_width
    } else {
        current
    };
    let max_offset = line_width + px(2.) - viewport_width;
    let max_offset = if max_offset > px(0.) {
        max_offset
    } else {
        px(0.)
    };
    if offset > max_offset {
        offset = max_offset;
    }
    offset
}

pub(super) fn single_line(text: &str) -> Cow<'_, str> {
    if text.contains('\r') || text.contains('\n') {
        Cow::Owned(text.replace(['\r', '\n'], " "))
    } else {
        Cow::Borrowed(text)
    }
}

pub(super) fn utf16_to_byte(text: &str, offset: usize) -> usize {
    let mut utf16_offset = 0;
    for (byte_offset, character) in text.char_indices() {
        if utf16_offset >= offset {
            return byte_offset;
        }
        utf16_offset += character.len_utf16();
        if utf16_offset > offset {
            return byte_offset + character.len_utf8();
        }
    }
    text.len()
}

pub(super) fn byte_to_utf16(text: &str, offset: usize) -> usize {
    let mut byte_offset = 0;
    let mut utf16_offset = 0;
    for character in text.chars() {
        if byte_offset >= offset {
            break;
        }
        byte_offset += character.len_utf8();
        utf16_offset += character.len_utf16();
    }
    utf16_offset
}

pub(super) fn range_from_utf16(text: &str, range: &Range<usize>) -> Range<usize> {
    utf16_to_byte(text, range.start)..utf16_to_byte(text, range.end)
}

pub(super) fn range_to_utf16(text: &str, range: &Range<usize>) -> Range<usize> {
    byte_to_utf16(text, range.start)..byte_to_utf16(text, range.end)
}

pub(super) fn floor_boundary(text: &str, offset: usize) -> usize {
    text.grapheme_indices(true)
        .map(|(index, _)| index)
        .chain(std::iter::once(text.len()))
        .take_while(|index| *index <= offset)
        .last()
        .unwrap_or(0)
}

pub(super) fn ceil_boundary(text: &str, offset: usize) -> usize {
    text.grapheme_indices(true)
        .map(|(index, _)| index)
        .find(|index| *index >= offset)
        .unwrap_or(text.len())
}

pub(super) fn nearest_boundary(text: &str, offset: usize) -> usize {
    let offset = offset.min(text.len());
    let before = floor_boundary(text, offset);
    let after = ceil_boundary(text, offset);
    if offset - before <= after - offset {
        before
    } else {
        after
    }
}

pub(super) fn normalize_range(text: &str, range: Range<usize>) -> Range<usize> {
    let start = range.start.min(range.end);
    let end = range.end.max(range.start);
    if start == end {
        let cursor = nearest_boundary(text, start);
        return cursor..cursor;
    }
    let start = floor_boundary(text, start);
    let end = ceil_boundary(text, end);
    start..end
}

pub(super) fn previous_boundary(text: &str, offset: usize) -> usize {
    text.grapheme_indices(true)
        .rev()
        .find_map(|(index, _)| (index < offset).then_some(index))
        .unwrap_or(0)
}

pub(super) fn next_boundary(text: &str, offset: usize) -> usize {
    text.grapheme_indices(true)
        .find_map(|(index, _)| (index > offset).then_some(index))
        .unwrap_or(text.len())
}

pub(super) fn mask_graphemes(text: &str) -> String {
    BULLET.repeat(text.graphemes(true).count())
}

pub(super) fn mask_utf16(text: &str) -> String {
    BULLET.repeat(text.encode_utf16().count())
}

pub(super) fn display_offset(text: &str, offset: usize, secret: bool) -> usize {
    if secret {
        text.grapheme_indices(true)
            .take_while(|(index, _)| *index < offset)
            .count()
            * BULLET.len()
    } else {
        offset
    }
}

pub(super) fn source_offset(text: &str, offset: usize, secret: bool) -> usize {
    if secret {
        text.grapheme_indices(true)
            .nth(offset / BULLET.len())
            .map(|(index, _)| index)
            .unwrap_or(text.len())
    } else {
        nearest_boundary(text, offset)
    }
}

pub(super) fn marked_ranges(
    content: &str,
    insertion_start: usize,
    inserted: &str,
    selected_utf16: Option<Range<usize>>,
) -> (Option<Range<usize>>, Range<usize>) {
    let marked = if inserted.is_empty() {
        None
    } else {
        Some(normalize_range(
            content,
            insertion_start..insertion_start + inserted.len(),
        ))
    };
    let selection = selected_utf16
        .map(|selected| {
            let base = byte_to_utf16(content, insertion_start);
            let inserted_length = inserted.encode_utf16().count();
            let range = base + selected.start.min(inserted_length)
                ..base + selected.end.min(inserted_length);
            normalize_range(content, range_from_utf16(content, &range))
        })
        .unwrap_or_else(|| {
            let end = ceil_boundary(content, insertion_start + inserted.len());
            end..end
        });
    (marked, selection)
}
