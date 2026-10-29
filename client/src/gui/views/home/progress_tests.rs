use super::*;

#[test]
fn indeterminate_animation_runs_only_when_intersecting_its_clip() {
    let visible = Bounds::new(gpui::point(px(10.), px(20.)), gpui::size(px(20.), px(8.)));
    let clip = Bounds::new(gpui::point(px(0.), px(24.)), gpui::size(px(100.), px(40.)));
    let offscreen = Bounds::new(gpui::point(px(10.), px(70.)), gpui::size(px(20.), px(8.)));

    assert!(animation_visible(visible, clip));
    assert!(!animation_visible(offscreen, clip));
}
