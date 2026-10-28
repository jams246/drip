use gpui::{HighlightStyle, StyledText, UnderlineStyle, div, prelude::*, px, rgb};

use super::{components, theme};

pub(super) fn render() -> impl IntoElement {
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
        .flex_shrink_0()
        .items_center()
        .gap_2()
        .px_8()
        .py_5()
        .border_b_1()
        .border_color(rgb(theme::BORDER))
        .bg(rgb(theme::PANEL))
        .child(components::icon(components::Glyph::Brand, 36.))
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(
                    div()
                        .id("drip-title")
                        .text_size(px(24.))
                        .font_weight(gpui::FontWeight::BOLD)
                        .child("DRIP"),
                )
                .child(
                    div()
                        .text_size(px(14.))
                        .text_color(rgb(theme::MUTED))
                        .child(subtitle),
                ),
        )
}
