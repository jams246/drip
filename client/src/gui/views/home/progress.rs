use std::sync::OnceLock;
use std::time::Instant;

use gpui::{Bounds, IntoElement, Pixels, div, prelude::*, px, rgb};

use crate::gui::theme;

static ANIMATION_ORIGIN: OnceLock<Instant> = OnceLock::new();

fn animation_visible(bounds: Bounds<Pixels>, mask: Bounds<Pixels>) -> bool {
    bounds.intersects(&mask)
}

fn indeterminate_segment() -> impl IntoElement {
    let origin = *ANIMATION_ORIGIN.get_or_init(Instant::now);
    gpui::canvas(
        |bounds, window, _| {
            let visible = animation_visible(bounds, window.content_mask().bounds);
            if visible {
                window.request_animation_frame();
            }
            visible
        },
        move |bounds, visible, window, _| {
            if !visible {
                return;
            }
            let phase = origin.elapsed().as_secs_f32() / 1.2 % 1.0;
            let width = bounds.size.width * 0.25;
            let travel = bounds.size.width - width;
            let segment = Bounds::new(
                gpui::point(bounds.origin.x + travel * phase, bounds.origin.y),
                gpui::size(width, bounds.size.height),
            );
            window.paint_quad(gpui::fill(segment, rgb(theme::ACCENT)).corner_radii(px(4.)));
        },
    )
    .absolute()
    .top_0()
    .bottom_0()
    .left_0()
    .right_0()
}

pub(super) fn render(selection_id: &str, processing: bool) -> impl IntoElement {
    let slot = div()
        .id(format!("home-progress-{selection_id}"))
        .w_full()
        .h(px(4.))
        .flex_shrink_0();
    if processing {
        slot.child(
            div()
                .id(format!("home-progress-track-{selection_id}"))
                .relative()
                .size_full()
                .rounded_full()
                .bg(rgb(theme::TRACK))
                .child(indeterminate_segment()),
        )
    } else {
        slot
    }
}

#[cfg(test)]
#[path = "progress_tests.rs"]
mod tests;
