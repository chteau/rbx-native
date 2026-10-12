//! Strokes a timeline (or any plot over time) is drawn with: dots, lines,
//! polylines, key diamonds, the ruler's ticks and the playhead. Shared by the
//! dope sheet and the sequence editor's plot, so the two draw alike.

use gpui_kit::*;

use super::{Tick, Timeline};
use crate::tokens;

pub(crate) fn dot(window: &mut Window, x: f32, y: f32, radius: f32, colour: Rgba) {
    window.paint_quad(fill(
        Bounds {
            origin: point(px(x - radius), px(y - radius)),
            size: size(px(radius * 2.0), px(radius * 2.0)),
        },
        colour,
    ));
}

/// A stroked run of points, open at both ends. `PathBuilder::stroke` rather
/// than a rotated quad: a curve is a run of short segments at every angle,
/// and a quad cannot carry the ones that are not axis-aligned.
pub(crate) fn polyline(window: &mut Window, points: &[Point<Pixels>], colour: Rgba, width: f32) {
    if points.len() < 2 {
        return;
    }
    let mut builder = PathBuilder::stroke(px(width));
    builder.add_polygon(points, false);
    if let Ok(path) = builder.build() {
        window.paint_path(path, colour);
    }
}

pub(crate) fn line(
    window: &mut Window,
    from: (f32, f32),
    to: (f32, f32),
    colour: Rgba,
    width: f32,
) {
    polyline(
        window,
        &[point(px(from.0), px(from.1)), point(px(to.0), px(to.1))],
        colour,
        width,
    );
}

/// A key: a diamond centred on `(x, y)`, `half` from centre to each tip,
/// with a darker rim so neighbouring keys stay told apart.
pub(crate) fn diamond(window: &mut Window, x: f32, y: f32, half: f32, fill: Rgba, rim: Rgba) {
    for (h, colour) in [(half + 1.0, rim), (half, fill)] {
        let mut builder = PathBuilder::fill();
        builder.add_polygon(
            &[
                point(px(x), px(y - h)),
                point(px(x + h), px(y)),
                point(px(x), px(y + h)),
                point(px(x - h), px(y)),
            ],
            true,
        );
        if let Ok(path) = builder.build() {
            window.paint_path(path, colour);
        }
    }
}

/// A filled, axis-aligned box.
pub(crate) fn rect(window: &mut Window, x: f32, y: f32, w: f32, h: f32, colour: Rgba) {
    window.paint_quad(fill(
        Bounds {
            origin: point(px(x), px(y)),
            size: size(px(w.max(0.0)), px(h.max(0.0))),
        },
        colour,
    ));
}

/// The ruler's tick marks along the bottom of a strip `height` tall, and a
/// faint gridline down through `below` more pixels under each major one.
pub(crate) fn ticks(
    window: &mut Window,
    origin: Point<Pixels>,
    ticks: &[Tick],
    height: f32,
    below: f32,
) {
    let (ox, oy) = (f32::from(origin.x), f32::from(origin.y));
    for tick in ticks {
        let x = ox + tick.x;
        let long = if tick.major {
            height * 0.45
        } else {
            height * 0.2
        };
        rect(
            window,
            x,
            oy + height - long,
            1.0,
            long,
            tokens::text_disabled(),
        );
        if tick.major && below > 0.0 {
            rect(window, x, oy + height, 1.0, below, tokens::border());
        }
    }
}

/// The playhead: a line down the whole timeline at `time`.
pub(crate) fn playhead(
    window: &mut Window,
    origin: Point<Pixels>,
    timeline: &Timeline,
    time: f32,
    height: f32,
) {
    let x = f32::from(origin.x) + timeline.x_of(time);
    rect(
        window,
        x - 0.5,
        f32::from(origin.y),
        2.0,
        height,
        tokens::check_on(),
    );
}
