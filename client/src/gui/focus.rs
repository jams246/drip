use std::cell::Cell;

use gpui::{App, Bounds, Div, FocusHandle, Pixels, Window, px};

pub(super) fn handle(id: &str, enabled: bool, window: &mut Window, cx: &mut App) -> FocusHandle {
    window
        .use_keyed_state(format!("focus-{id}"), cx, |_, cx| cx.focus_handle())
        .read(cx)
        .clone()
        .tab_stop(enabled)
}

pub(super) fn reveal(
    element: Div,
    id: &str,
    focus: FocusHandle,
    window: &mut Window,
    cx: &mut App,
) -> Div {
    let previously_focused =
        window.use_keyed_state(format!("reveal-{id}"), cx, |_, _| Cell::new(false));
    element.on_children_prepainted(move |bounds, window, cx| {
        let focused = focus.is_focused(window);
        if focused && !previously_focused.read(cx).replace(focused) {
            if let Some(bounds) = bounds.into_iter().reduce(|a, b| a.union(&b)) {
                // Include control padding around the prepainted children.
                window.request_autoscroll(bounds.dilate(px(24.)));
            }
        } else if !focused {
            previously_focused.read(cx).set(false);
        }
    })
}

pub(super) fn scroll_offset(
    offset: Pixels,
    maximum: Pixels,
    viewport: Bounds<Pixels>,
    target: Bounds<Pixels>,
) -> Pixels {
    if viewport.size.height <= px(16.) {
        return offset;
    }
    let top = viewport.top() + px(8.);
    let bottom = viewport.bottom() - px(8.);
    let adjustment = if target.top() < top || target.size.height > bottom - top {
        top - target.top()
    } else if target.bottom() > bottom {
        bottom - target.bottom()
    } else {
        px(0.)
    };
    (offset + adjustment).clamp(-maximum, px(0.))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{point, size};

    #[test]
    fn focused_controls_scroll_only_as_far_as_needed() {
        let viewport = Bounds::new(point(px(0.), px(100.)), size(px(500.), px(300.)));
        let target = |y, height| Bounds::new(point(px(20.), px(y)), size(px(200.), px(height)));
        let offset = |target| scroll_offset(px(-100.), px(1000.), viewport, target);
        assert_eq!(offset(target(150., 36.)), px(-100.));
        assert_eq!(offset(target(90., 36.)), px(-82.));
        assert_eq!(offset(target(390., 36.)), px(-134.));
        assert_eq!(offset(target(150., 500.)), px(-142.));
        assert_eq!(offset(target(-1000., 36.)), px(0.));
        assert_eq!(offset(target(2000., 36.)), px(-1000.));
        assert_eq!(
            scroll_offset(px(-100.), px(1000.), Bounds::default(), target(150., 36.)),
            px(-100.)
        );
    }
}
