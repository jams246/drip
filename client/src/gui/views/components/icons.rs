use gpui::{Bounds, Hsla, IntoElement, Path, PathBuilder, Pixels, point, prelude::*, px, rgb};

use crate::gui::theme;

#[derive(Clone, Copy)]
pub(in crate::gui::views) enum Glyph {
    Brand,
    Folder,
    File,
    Check,
    Clock,
    Attention,
}

pub(in crate::gui::views) fn icon(glyph: Glyph, dimension: f32) -> impl IntoElement {
    gpui::canvas(
        move |bounds, _, _| paths(glyph, bounds),
        |_, paths, window, _| {
            for (path, color) in paths {
                window.paint_path(path, color);
            }
        },
    )
    .w(px(dimension))
    .h(px(dimension))
}

fn paths(glyph: Glyph, bounds: Bounds<Pixels>) -> Vec<(Path<Pixels>, Hsla)> {
    let width = bounds.size.width.as_f32();
    let height = bounds.size.height.as_f32();
    let scale = (width.min(height) / 24.).min(2.);
    let pad_x = (width - 24. * scale) / 2.;
    let pad_y = (height - 24. * scale) / 2.;
    let at = |x: f32, y: f32| {
        point(
            bounds.left() + px(pad_x + x * scale),
            bounds.top() + px(pad_y + y * scale),
        )
    };
    let mut paths = Vec::new();
    match glyph {
        Glyph::Brand => {
            let mut drop = PathBuilder::fill();
            drop.move_to(at(12., 2.));
            drop.cubic_bezier_to(at(20.5, 14.5), at(13.5, 6.), at(20.5, 10.));
            drop.cubic_bezier_to(at(12., 23.), at(20.5, 19.2), at(16.7, 23.));
            drop.cubic_bezier_to(at(3.5, 14.5), at(7.3, 23.), at(3.5, 19.2));
            drop.cubic_bezier_to(at(12., 2.), at(3.5, 10.), at(10.5, 6.));
            drop.close();
            push(&mut paths, drop, rgb(theme::ACCENT).into());

            let mut arrow = PathBuilder::fill();
            arrow.move_to(at(11., 19.));
            arrow.line_to(at(13., 19.));
            arrow.line_to(at(13., 12.5));
            arrow.line_to(at(16., 12.5));
            arrow.line_to(at(12., 8.5));
            arrow.line_to(at(8., 12.5));
            arrow.line_to(at(11., 12.5));
            arrow.close();
            push(&mut paths, arrow, rgb(theme::PAGE).into());
        }
        Glyph::Folder => {
            let mut folder = PathBuilder::stroke(px(1.7));
            folder.move_to(at(2.5, 6.));
            folder.line_to(at(9., 6.));
            folder.line_to(at(11., 8.));
            folder.line_to(at(21.5, 8.));
            folder.line_to(at(21.5, 19.5));
            folder.line_to(at(2.5, 19.5));
            folder.close();
            push(&mut paths, folder, rgb(theme::MUTED).into());
        }
        Glyph::File => {
            let mut file = PathBuilder::stroke(px(1.7));
            file.move_to(at(5., 2.));
            file.line_to(at(14., 2.));
            file.line_to(at(20., 8.));
            file.line_to(at(20., 22.));
            file.line_to(at(5., 22.));
            file.close();
            file.move_to(at(14., 2.));
            file.line_to(at(14., 8.));
            file.line_to(at(20., 8.));
            push(&mut paths, file, rgb(theme::MUTED).into());
        }
        Glyph::Check => {
            let mut check = PathBuilder::stroke(px(1.8));
            circle(&mut check, &at, 12., 12., 9.);
            check.move_to(at(7.5, 12.));
            check.line_to(at(10.5, 15.));
            check.line_to(at(16.5, 9.));
            push(&mut paths, check, rgb(theme::SUCCESS).into());
        }
        Glyph::Clock => {
            let mut clock = PathBuilder::stroke(px(1.8));
            circle(&mut clock, &at, 12., 12., 9.);
            clock.move_to(at(12., 7.));
            clock.line_to(at(12., 12.));
            clock.line_to(at(16., 14.));
            push(&mut paths, clock, rgb(theme::MUTED).into());
        }
        Glyph::Attention => {
            let mut alert = PathBuilder::stroke(px(1.8));
            alert.move_to(at(12., 2.5));
            alert.line_to(at(22., 20.5));
            alert.line_to(at(2., 20.5));
            alert.close();
            alert.move_to(at(12., 8.));
            alert.line_to(at(12., 14.));
            alert.move_to(at(12., 17.));
            alert.line_to(at(12., 17.1));
            push(&mut paths, alert, rgb(theme::ERROR).into());
        }
    }
    paths
}

fn circle(
    builder: &mut PathBuilder,
    at: &impl Fn(f32, f32) -> gpui::Point<Pixels>,
    x: f32,
    y: f32,
    radius: f32,
) {
    builder.move_to(at(x, y - radius));
    builder.cubic_bezier_to(
        at(x + radius, y),
        at(x + radius, y - radius),
        at(x + radius, y - radius),
    );
    builder.cubic_bezier_to(
        at(x, y + radius),
        at(x + radius, y + radius),
        at(x, y + radius),
    );
    builder.cubic_bezier_to(
        at(x - radius, y),
        at(x - radius, y + radius),
        at(x - radius, y + radius),
    );
    builder.cubic_bezier_to(
        at(x, y - radius),
        at(x - radius, y - radius),
        at(x, y - radius),
    );
}

fn push(paths: &mut Vec<(Path<Pixels>, Hsla)>, builder: PathBuilder, color: Hsla) {
    if let Ok(path) = builder.build() {
        paths.push((path, color));
    }
}
