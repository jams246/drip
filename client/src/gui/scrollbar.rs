mod geometry;

use std::cell::Cell;

use gpui::{
    Bounds, Context, HitboxBehavior, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, ScrollEvent, ScrollHandle, Window, canvas, fill, point, prelude::*, px,
    rgb, size,
};

use super::{model::Shell, theme};
use geometry::{Geometry, clamp};

pub(super) fn render(
    scroll: ScrollHandle,
    tab: usize,
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    let drag = window.use_keyed_state(("tab-scrollbar-drag", tab), cx, |_, _| {
        Cell::new(None::<Pixels>)
    });
    canvas(
        |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
        move |bounds, hitbox, window, cx| {
            let mut offset = scroll.offset();
            let next = clamp(offset.y, scroll.max_offset().y);
            if offset.y != next {
                offset.y = next;
                scroll.set_offset(offset);
                window.refresh();
            }
            let Some(geometry) = Geometry::new(bounds.size.height, scroll.max_offset().y, next)
            else {
                drag.read(cx).set(None);
                return;
            };
            window.paint_quad(fill(bounds, rgb(theme::PANEL)));
            let thumb = Bounds::new(
                point(bounds.left() + px(2.), bounds.top() + geometry.top),
                size(px(8.), geometry.height),
            );
            window.paint_quad(fill(thumb, rgb(theme::MUTED)).corner_radii(px(4.)));
            window.on_mouse_event({
                let scroll = scroll.clone();
                let drag = drag.clone();
                let hitbox = hitbox.clone();
                move |event: &MouseDownEvent, phase, window, cx| {
                    if !phase.bubble()
                        || event.button != MouseButton::Left
                        || !hitbox.is_hovered(window)
                    {
                        return;
                    }
                    if thumb.contains(&event.position) {
                        drag.read(cx).set(Some(event.position.y - thumb.top()));
                    } else {
                        set_offset(
                            &scroll,
                            geometry.page(event.position.y - bounds.top()),
                            window,
                        );
                    }
                    cx.stop_propagation();
                }
            });
            window.on_mouse_event({
                let scroll = scroll.clone();
                let drag = drag.clone();
                move |event: &MouseMoveEvent, phase, window, cx| {
                    if !phase.capture() {
                        return;
                    }
                    if event.pressed_button != Some(MouseButton::Left) {
                        drag.read(cx).set(None);
                    } else if let Some(grab) = drag.read(cx).get() {
                        set_offset(
                            &scroll,
                            geometry.drag(event.position.y - bounds.top(), grab),
                            window,
                        );
                        cx.stop_propagation();
                    }
                }
            });
            window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                if phase.capture() && event.button == MouseButton::Left {
                    drag.read(cx).set(None);
                }
            });
            window.on_mouse_event(move |event: &ScrollEvent, phase, window, cx| {
                if phase.bubble() && hitbox.is_hovered(window) {
                    let next = scroll.offset().y + event.delta.pixel_delta(px(20.)).y;
                    set_offset(&scroll, clamp(next, scroll.max_offset().y), window);
                    cx.stop_propagation();
                }
            });
        },
    )
    .w(px(12.))
    .h_full()
    .flex_shrink_0()
}

fn set_offset(scroll: &ScrollHandle, y: Pixels, window: &mut Window) {
    let mut offset = scroll.offset();
    offset.y = y;
    scroll.set_offset(offset);
    window.refresh();
}
