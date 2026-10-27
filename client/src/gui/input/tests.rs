use super::text;
use gpui::px;
use unicode_segmentation::UnicodeSegmentation;

#[test]
fn cursor_and_delete_boundaries_preserve_unicode_graphemes() {
    let value = "e\u{301}🙂👩‍👩‍👧‍👦";
    let boundaries: Vec<_> = value
        .grapheme_indices(true)
        .map(|(index, _)| index)
        .chain(std::iter::once(value.len()))
        .collect();

    assert_eq!(boundaries, vec![0, 3, 7, value.len()]);
    assert_eq!(text::previous_boundary(value, 7), 3);
    assert_eq!(text::next_boundary(value, 3), 7);
    assert_eq!(text::normalize_range(value, 4..5), 3..7);
    assert_eq!(text::normalize_range(value, 4..4), 3..3);
}

#[test]
fn native_utf16_ranges_round_trip_supplementary_characters() {
    let value = "e🙂漢";
    let range = text::range_from_utf16(value, &(1..3));

    assert_eq!(&value[range.clone()], "🙂");
    assert_eq!(text::range_to_utf16(value, &range), 1..3);
    assert_eq!(
        text::normalize_range(value, text::range_from_utf16(value, &(1..2))),
        1..5
    );
}

#[test]
fn secret_mask_never_contains_plaintext_and_tracks_graphemes() {
    let value = "e\u{301}🙂";

    assert_eq!(text::mask_graphemes(value), "••");
    assert_eq!(text::mask_utf16(value), "••••");
    assert_eq!(text::display_offset(value, value.len(), true), 6);
    assert_eq!(text::source_offset(value, 3, true), 3);
    assert_eq!(text::source_offset(value, 6, true), value.len());
}

#[test]
fn ime_preedit_selection_uses_inserted_text_utf16_offsets() {
    let inserted = "e\u{301}🙂";
    let content = format!("X{inserted}Y");
    let (marked, selected) = text::marked_ranges(&content, 1, inserted, Some(2..4));

    assert_eq!(marked, Some(1..8));
    assert_eq!(&content[selected.clone()], "🙂");
}

#[test]
fn ime_preedit_cursor_snaps_after_a_grapheme_join() {
    let inserted = "\u{200d}";
    let content = format!("👩{inserted}💻");
    let (marked, selected) = text::marked_ranges(&content, "👩".len(), inserted, None);

    assert_eq!(marked, Some(0..content.len()));
    assert_eq!(selected, content.len()..content.len());
}

#[test]
fn native_paste_stays_single_line_without_shifting_ime_utf16_offsets() {
    let value = text::single_line("a\r\nb");

    assert_eq!(value.as_ref(), "a  b");
    assert_eq!(value.encode_utf16().count(), 4);
}

#[test]
fn horizontal_scroll_keeps_caret_visible_and_clamps_after_resize() {
    assert_eq!(
        text::horizontal_offset(px(500.), px(500.), px(100.), px(0.)),
        px(402.)
    );
    assert_eq!(
        text::horizontal_offset(px(40.), px(500.), px(100.), px(80.)),
        px(40.)
    );
    assert_eq!(
        text::horizontal_offset(px(450.), px(500.), px(300.), px(400.)),
        px(202.)
    );
}
