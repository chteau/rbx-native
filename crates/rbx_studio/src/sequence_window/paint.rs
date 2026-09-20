//! Everything the sequence editor draws rather than lays out: the gradient
//! ramp, the curve and its envelope band, the grid behind them and the
//! handles on top.
//!
//! The same two painters serve the panel and the row's own preview strip —
//! a preview is this drawing with the grid and the handles turned off, so a
//! row and the graph it opens can never disagree about the shape of a
//! sequence.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::*;

use crate::sequence_editor::{Kind, Rect, Stop};
use crate::tokens;

/// Steps across a ramp or a curve. 128 is under half a pixel per step at
/// the panel's width, which is finer than the seam a linear ramp could show
/// and cheap enough to repaint every frame.
const STEPS: usize = 128;

/// Gridlines each way behind a curve, chosen so the top, the bottom and the
/// quarters all land on one.
const GRID: usize = 4;

/// Breathing room above and below a curve, as a fraction of the plot's
/// height and capped in pixels. Without it a keypoint at value 0 — which
/// every transparency curve has — draws half outside the box, and the axis
/// ceiling is unreachable by pointer because it is the top edge itself. A
/// ramp takes none: a gradient is meant to bleed to its own edges.
const CURVE_PAD: f32 = 0.1;
const CURVE_PAD_MAX: f32 = 8.0;

const CURVE_WIDTH: f32 = 1.5;
const HANDLE: f32 = 4.0;
const ENVELOPE_HANDLE: f32 = 3.0;

/// How the plot is dressed: a preview turns both off, the panel turns both
/// on and names the stop under the cursor's attention.
#[derive(Clone, Copy)]
pub(crate) struct Look {
    pub(crate) grid: bool,
    pub(crate) handles: Option<usize>,
}

impl Look {
    pub(crate) fn preview() -> Self {
        Look {
            grid: false,
            handles: None,
        }
    }
}

/// The plot element. `report` — when the caller wants it — is handed the
/// laid-out rectangle every frame, which is what turns a later pointer
/// position into a `(time, value)` (see `sequence_editor::Rect`).
pub(crate) fn plot(
    kind: Kind,
    stops: Vec<Stop>,
    ceiling: f32,
    look: Look,
    report: Option<Rc<Cell<Option<Rect>>>>,
) -> impl IntoElement {
    canvas(
        move |bounds, _, _| {
            if let Some(cell) = &report {
                // The *padded* rect, the same one the paint below draws
                // into: a pointer and a keypoint have to agree about where
                // value 0 is.
                cell.set(Some(plot_rect(kind, bounds)));
            }
        },
        move |bounds, _, window, _| {
            let rect = plot_rect(kind, bounds);
            match kind {
                Kind::Color => ramp(&stops, rect, look, window),
                Kind::Number => curve(&stops, ceiling, rect, look, window),
            }
        },
    )
    .size_full()
}

fn plot_rect(kind: Kind, bounds: Bounds<Pixels>) -> Rect {
    let height = f32::from(bounds.size.height);
    let pad = match kind {
        Kind::Number => (height * CURVE_PAD).min(CURVE_PAD_MAX),
        Kind::Color => 0.0,
    };
    Rect {
        x: f32::from(bounds.origin.x),
        y: f32::from(bounds.origin.y) + pad,
        width: f32::from(bounds.size.width),
        height: (height - pad * 2.0).max(1.0),
    }
}

/// A gradient, as one thin quad per step. A quad per step rather than a
/// background gradient because a `ColorSequence` is a multi-stop ramp that
/// can also hard-step (two stops at one time), which a two-colour linear
/// gradient cannot express.
fn ramp(stops: &[Stop], rect: Rect, look: Look, window: &mut Window) {
    let width = rect.width / STEPS as f32;
    for step in 0..STEPS {
        let t = step as f32 / (STEPS - 1) as f32;
        let colour = crate::sequence_editor::sample(stops, t).color;
        window.paint_quad(fill(
            Bounds {
                // A half-step of overlap, so neighbouring quads cannot
                // leave a hairline of background between them after
                // rounding to device pixels.
                origin: point(px(rect.x + t * (rect.width - width)), px(rect.y)),
                size: size(px(width + 1.0), px(rect.height)),
            },
            rgb(rgb_hex(colour.r, colour.g, colour.b)),
        ));
    }
    let Some(selected) = look.handles else {
        return;
    };
    for (index, stop) in stops.iter().enumerate() {
        marker(
            window,
            rect.x + stop.time * rect.width,
            rect.y + rect.height,
            index == selected,
        );
    }
}

/// A `NumberSequence`: the grid, the envelope band, the curve, the handles.
fn curve(stops: &[Stop], ceiling: f32, rect: Rect, look: Look, window: &mut Window) {
    if look.grid {
        for step in 0..=GRID {
            let f = step as f32 / GRID as f32;
            line(
                window,
                (rect.x, rect.y + f * rect.height),
                (rect.x + rect.width, rect.y + f * rect.height),
                tokens::divider(),
                1.0,
            );
            line(
                window,
                (rect.x + f * rect.width, rect.y),
                (rect.x + f * rect.width, rect.y + rect.height),
                tokens::divider(),
                1.0,
            );
        }
    }

    // The band first, so the curve reads on top of its own spread.
    if stops.iter().any(|stop| stop.envelope > 0.0) {
        let mut band = PathBuilder::fill();
        let mut outline: Vec<Point<Pixels>> = Vec::with_capacity(stops.len() * 2);
        for stop in stops {
            let (x, y) = rect.of(stop.time, stop.value + stop.envelope, ceiling);
            outline.push(point(px(x), px(y)));
        }
        for stop in stops.iter().rev() {
            let (x, y) = rect.of(stop.time, (stop.value - stop.envelope).max(0.0), ceiling);
            outline.push(point(px(x), px(y)));
        }
        band.add_polygon(&outline, true);
        if let Ok(path) = band.build() {
            window.paint_path(path, accent(0x33));
        }
    }

    let points: Vec<Point<Pixels>> = (0..STEPS)
        .map(|step| {
            let t = step as f32 / (STEPS - 1) as f32;
            let (x, y) = rect.of(t, crate::sequence_editor::sample(stops, t).value, ceiling);
            point(px(x), px(y))
        })
        .collect();
    polyline(window, &points, tokens::check_on(), CURVE_WIDTH);

    let Some(selected) = look.handles else {
        return;
    };
    for (index, stop) in stops.iter().enumerate() {
        if stop.envelope > 0.0 {
            let (x, y) = rect.of(stop.time, stop.value + stop.envelope, ceiling);
            dot(window, x, y, ENVELOPE_HANDLE, accent(0xAA));
        }
        let (x, y) = rect.of(stop.time, stop.value, ceiling);
        dot(
            window,
            x,
            y,
            HANDLE,
            if index == selected {
                tokens::text_full()
            } else {
                tokens::check_on()
            },
        );
    }
}

/// A colour stop's triangle, pointing up at the ramp it sits under.
fn marker(window: &mut Window, x: f32, bottom: f32, selected: bool) {
    let mut builder = PathBuilder::fill();
    builder.add_polygon(
        &[
            point(px(x), px(bottom)),
            point(px(x - HANDLE), px(bottom + HANDLE * 1.8)),
            point(px(x + HANDLE), px(bottom + HANDLE * 1.8)),
        ],
        true,
    );
    if let Ok(path) = builder.build() {
        window.paint_path(
            path,
            if selected {
                tokens::text_full()
            } else {
                tokens::text_disabled()
            },
        );
    }
}

fn dot(window: &mut Window, x: f32, y: f32, radius: f32, colour: Rgba) {
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
fn polyline(window: &mut Window, points: &[Point<Pixels>], colour: Rgba, width: f32) {
    if points.len() < 2 {
        return;
    }
    let mut builder = PathBuilder::stroke(px(width));
    builder.add_polygon(points, false);
    if let Ok(path) = builder.build() {
        window.paint_path(path, colour);
    }
}

fn line(window: &mut Window, from: (f32, f32), to: (f32, f32), colour: Rgba, width: f32) {
    polyline(
        window,
        &[point(px(from.0), px(from.1)), point(px(to.0), px(to.1))],
        colour,
        width,
    );
}

fn accent(alpha: u32) -> Rgba {
    let base = tokens::check_on();
    rgba((rgb_hex(base.r, base.g, base.b) << 8) | alpha)
}

fn rgb_hex(r: f32, g: f32, b: f32) -> u32 {
    let byte = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u32;
    (byte(r) << 16) | (byte(g) << 8) | byte(b)
}
