use drip_client::settings::SavedSelection;
use gpui::{Context, ElementId, Render, Stateful, Window, div, prelude::*, px, rgb};

use super::{Glyph, breakable_path, icon};
use crate::gui::theme;

pub(in crate::gui::views) fn list(id: &'static str) -> Stateful<gpui::Div> {
    div()
        .id(id)
        .tab_group()
        .w_full()
        .flex()
        .flex_col()
        .flex_shrink_0()
        .rounded(px(8.))
        .border_1()
        .border_color(rgb(theme::BORDER))
        .bg(rgb(theme::PANEL))
        .overflow_hidden()
}

pub(in crate::gui::views) fn row(
    id: impl Into<ElementId>,
    selection: &SavedSelection,
    divider: bool,
) -> Stateful<gpui::Div> {
    let path = selection.source.to_windows_path();
    let name = selection
        .source
        .components()
        .last()
        .cloned()
        .unwrap_or_else(|| format!("{}:", selection.source.drive()));
    let tooltip = PathTooltip {
        name: name.clone(),
        path: path.clone(),
    };
    div()
        .id(id)
        .relative()
        .w_full()
        .h(px(64.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap_3()
        .px_3()
        .when(divider, |row| {
            row.border_t_1().border_color(rgb(theme::BORDER))
        })
        .child(div().flex_shrink_0().child(icon(
            if selection.root_kind == drip_core::protocol::EntryKind::Directory {
                Glyph::Folder
            } else {
                Glyph::File
            },
            24.,
        )))
        .child(
            div()
                .id("watched-path")
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .tooltip(move |_, cx| cx.new(|_| tooltip.clone()).into())
                .child(
                    div()
                        .text_size(px(14.))
                        .line_height(px(20.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .truncate()
                        .child(name),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .line_height(px(18.))
                        .text_color(rgb(theme::MUTED))
                        .truncate()
                        .child(path),
                ),
        )
}

#[derive(Clone)]
struct PathTooltip {
    name: String,
    path: String,
}

impl Render for PathTooltip {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .max_w((window.viewport_size().width - px(32.)).min(px(600.)))
            .p_3()
            .rounded(px(6.))
            .border_1()
            .border_color(rgb(theme::BORDER))
            .bg(rgb(theme::PANEL))
            .text_color(rgb(theme::TEXT))
            .font_family("Segoe UI")
            .text_size(px(14.))
            .child(div().child(self.name.clone()))
            .child(
                div()
                    .text_color(rgb(theme::MUTED))
                    .child(breakable_path(&self.path)),
            )
    }
}
