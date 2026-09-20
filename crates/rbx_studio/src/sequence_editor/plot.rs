//! Where a stop sits inside the plot rectangle, and which stop a pointer is
//! over. Pure arithmetic in normalized `[0, 1]` time and `[0, ceiling]`
//! value, so none of it needs a window to test.

use rbx_dom::Color3Data;

use super::{Kind, Stop};

/// The plot area the panel laid out, in window pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Rect {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
}

impl Rect {
    /// The `(time, value)` a pointer at this window position is over.
    /// Deliberately unclamped in time so a drag that leaves the plot still
    /// reads as "past the end" rather than freezing — [`super::Editor::drag_to`]
    /// clamps against the neighbours, which is the rule that actually matters.
    pub(crate) fn at(&self, x: f32, y: f32, ceiling: f32) -> (f32, f32) {
        let time = if self.width > 0.0 {
            (x - self.x) / self.width
        } else {
            0.0
        };
        let value = if self.height > 0.0 {
            1.0 - (y - self.y) / self.height
        } else {
            0.0
        };
        (time.clamp(0.0, 1.0), (value * ceiling).max(0.0))
    }

    /// The inverse: where a stop draws.
    pub(crate) fn of(&self, time: f32, value: f32, ceiling: f32) -> (f32, f32) {
        let fraction = if ceiling > 0.0 { value / ceiling } else { 0.0 };
        (
            self.x + time.clamp(0.0, 1.0) * self.width,
            self.y + (1.0 - fraction.clamp(0.0, 1.0)) * self.height,
        )
    }
}

/// Which grabbable handles a stop has. A colour stop is a marker on a
/// timeline with nothing to drag vertically, so it has exactly one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Handle {
    Point,
    Envelope,
}

pub(crate) fn handles(kind: Kind, _stop: &Stop) -> Vec<Handle> {
    match kind {
        Kind::Color => vec![Handle::Point],
        // The envelope handle is offered even at zero, sitting on top of the
        // stop itself: a band nobody can start dragging is a band nobody
        // discovers. `distance` breaks the tie toward whichever is closer,
        // and `grab`'s scan prefers the nearer of the two.
        Kind::Number => vec![Handle::Point, Handle::Envelope],
    }
}

pub(crate) fn handle_value(handle: Handle, stop: &Stop) -> f32 {
    match handle {
        Handle::Point => stop.value,
        Handle::Envelope => stop.value + stop.envelope,
    }
}

/// Distance in plot space, with the value axis normalized by `ceiling` so a
/// tall sequence is no harder to grab than a short one.
pub(crate) fn distance(time: f32, value: f32, at_time: f32, at_value: f32, ceiling: f32) -> f32 {
    let dt = time - at_time;
    let dv = if ceiling > 0.0 {
        (value - at_value) / ceiling
    } else {
        0.0
    };
    (dt * dt + dv * dv).sqrt()
}

/// The sequence at `t`, linearly between the stops either side — the same
/// rule `rbx_viewer`'s `eval_number`/`eval_color` apply, including two stops
/// at one time reading as a hard step.
pub(crate) fn sample(stops: &[Stop], t: f32) -> Stop {
    let t = t.clamp(0.0, 1.0);
    let Some(first) = stops.first() else {
        return Stop {
            time: t,
            value: 0.0,
            envelope: 0.0,
            color: Color3Data {
                r: 0.0,
                g: 0.0,
                b: 0.0,
            },
        };
    };
    if stops.len() == 1 || t <= first.time {
        return *first;
    }
    for pair in stops.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if t <= b.time {
            let span = b.time - a.time;
            let f = if span > 0.0 { (t - a.time) / span } else { 0.0 };
            return Stop {
                time: t,
                value: mix(a.value, b.value, f),
                envelope: mix(a.envelope, b.envelope, f),
                color: Color3Data {
                    r: mix(a.color.r, b.color.r, f),
                    g: mix(a.color.g, b.color.g, f),
                    b: mix(a.color.b, b.color.b, f),
                },
            };
        }
    }
    *stops.last().expect("checked non-empty above")
}

fn mix(from: f32, to: f32, f: f32) -> f32 {
    from + (to - from) * f
}
