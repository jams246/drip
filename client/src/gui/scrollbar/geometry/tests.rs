use super::*;

#[test]
fn fitting_or_zero_height_viewports_hide_the_thumb() {
    for (viewport, maximum) in [(100., 0.), (0., 100.), (0., 0.)] {
        assert!(Geometry::new(px(viewport), px(maximum), px(0.)).is_none());
    }
}

#[test]
fn thumb_is_proportional_and_reaches_both_ends() {
    let start = Geometry::new(px(200.), px(600.), px(0.)).unwrap();
    let end = Geometry::new(px(200.), px(600.), px(-600.)).unwrap();
    assert_eq!(
        (start.height, start.top, end.top),
        (px(50.), px(0.), px(150.))
    );
}

#[test]
fn minimum_thumb_fits_short_tracks() {
    let normal = Geometry::new(px(200.), px(10000.), px(0.)).unwrap();
    let short = Geometry::new(px(12.), px(10000.), px(0.)).unwrap();
    assert_eq!((normal.height, short.height), (px(24.), px(12.)));
}

#[test]
fn track_clicks_page_and_clamp() {
    let middle = Geometry::new(px(200.), px(600.), px(-300.)).unwrap();
    let end = Geometry::new(px(200.), px(600.), px(-550.)).unwrap();
    assert_eq!(
        (
            middle.page(px(0.)),
            middle.page(px(200.)),
            end.page(px(200.))
        ),
        (px(-100.), px(-500.), px(-600.))
    );
    assert_eq!(middle.page(middle.top + px(1.)), middle.offset);
}

#[test]
fn dragging_preserves_grab_position_and_clamps_outside_track() {
    let geometry = Geometry::new(px(200.), px(600.), px(-300.)).unwrap();
    assert_eq!(geometry.drag(geometry.top + px(10.), px(10.)), px(-300.));
    assert_eq!(geometry.drag(px(-100.), px(10.)), px(0.));
    assert_eq!(geometry.drag(px(1000.), px(10.)), px(-600.));
}

#[test]
fn content_shrink_clamps_offset_even_without_overflow() {
    assert_eq!(clamp(px(-600.), px(100.)), px(-100.));
    assert_eq!(clamp(px(-100.), px(0.)), px(0.));
    assert_eq!(clamp(px(20.), px(100.)), px(0.));
}

#[test]
fn short_track_drag_keeps_current_offset() {
    let geometry = Geometry::new(px(12.), px(100.), px(-50.)).unwrap();
    assert_eq!(geometry.drag(px(10.), px(3.)), px(-50.));
}
