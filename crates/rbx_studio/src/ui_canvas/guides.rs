//! Smart alignment guides and the distance readout: pure 2D geometry over
//! axis-aligned boxes, knowing nothing of GUI objects or of the canvas
//! drawing them, so any 2D tool can reuse it.
//!
//! Snapping follows the Figma model the canvas is patterned on: a moving
//! box's left, centre and right (top, middle and bottom) are each pulled
//! onto the nearest of every other box's same three lines when one is
//! within reach, independently per axis, and a guide is drawn along every
//! line that ends up shared.

use super::Rect;

/// A guide line: across (`axis` 0, a vertical line at `x = at`) or down
/// (`axis` 1, a horizontal line at `y = at`), running from `from` to `to`
/// along the other axis.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Guide {
    pub(crate) axis: usize,
    pub(crate) at: f32,
    pub(crate) from: f32,
    pub(crate) to: f32,
}

/// Lines closer than this are one line: what a snapped edge reads as after
/// a `UDim` offset rounds it to a whole pixel.
const SAME_LINE: f32 = 0.5;

/// The smallest shift within `reach` that puts one of `moving` onto one of
/// `targets`, or `None` when nothing is that close.
pub(crate) fn snap_axis(moving: &[f32], targets: &[f32], reach: f32) -> Option<f32> {
    moving
        .iter()
        .flat_map(|&line| targets.iter().map(move |&target| target - line))
        .filter(|shift| shift.abs() <= reach)
        .min_by(|a, b| a.abs().total_cmp(&b.abs()))
}

/// Every min/centre/max line of `others` along `axis`.
pub(crate) fn lines_of(others: &[Rect], axis: usize) -> Vec<f32> {
    others.iter().flat_map(|other| other.lines(axis)).collect()
}

/// How far to shift `moving` so it snaps onto `others`, per axis — zero on
/// an axis with nothing within `reach`.
pub(crate) fn snap_move(moving: &Rect, others: &[Rect], reach: f32) -> [f32; 2] {
    [0, 1].map(|axis| snap_axis(&moving.lines(axis), &lines_of(others, axis), reach).unwrap_or(0.0))
}

/// A guide along every line `moving` now shares with one of `others`,
/// spanning both boxes so the eye can follow it from one to the other.
pub(crate) fn guides(moving: &Rect, others: &[Rect]) -> Vec<Guide> {
    let mut found: Vec<Guide> = Vec::new();
    for axis in [0, 1] {
        let across = 1 - axis;
        for other in others {
            for line in moving.lines(axis) {
                if !other
                    .lines(axis)
                    .iter()
                    .any(|&target| (target - line).abs() < SAME_LINE)
                {
                    continue;
                }
                let (a, b) = (moving.along(across), other.along(across));
                let span = (a.0.min(b.0), (a.0 + a.1).max(b.0 + b.1));
                // One guide per line: two siblings sharing it widen the
                // one already there rather than drawing over it.
                match found
                    .iter_mut()
                    .find(|guide| guide.axis == axis && (guide.at - line).abs() < SAME_LINE)
                {
                    Some(guide) => {
                        guide.from = guide.from.min(span.0);
                        guide.to = guide.to.max(span.1);
                    }
                    None => found.push(Guide {
                        axis,
                        at: line,
                        from: span.0,
                        to: span.1,
                    }),
                }
            }
        }
    }
    found
}

/// One distance of the readout: a run from `a` to `b`, `length` pixels long.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Distance {
    pub(crate) a: [f32; 2],
    pub(crate) b: [f32; 2],
    pub(crate) length: f32,
}

/// The distances between `from` (the selection) and `to` (what the pointer
/// is over): from each side to the matching side of the box that holds the
/// other when one is inside the other, and otherwise the gap between them
/// on each axis where they do not overlap.
pub(crate) fn measure(from: &Rect, to: &Rect) -> Vec<Distance> {
    if to.contains(from) {
        return inset(from, to);
    }
    if from.contains(to) {
        return inset(to, from);
    }
    let mut runs = Vec::new();
    for axis in [0, 1] {
        let across = 1 - axis;
        let (f, t) = (from.along(axis), to.along(axis));
        let (near, far) = match f.0 + f.1 <= t.0 {
            true => (f.0 + f.1, t.0),
            false if t.0 + t.1 <= f.0 => (t.0 + t.1, f.0),
            false => continue,
        };
        // Along the middle of whatever the two share across, or the
        // selection's own middle when they share nothing.
        let (fa, ta) = (from.along(across), to.along(across));
        let lo = fa.0.max(ta.0);
        let hi = (fa.0 + fa.1).min(ta.0 + ta.1);
        let mid = match lo <= hi {
            true => (lo + hi) * 0.5,
            false => fa.0 + fa.1 * 0.5,
        };
        let point = |value: f32| match axis {
            0 => [value, mid],
            _ => [mid, value],
        };
        runs.push(Distance {
            a: point(near),
            b: point(far),
            length: far - near,
        });
    }
    runs
}

/// `inner`'s four sides to `outer`'s, each drawn through `inner`'s middle.
fn inset(inner: &Rect, outer: &Rect) -> Vec<Distance> {
    let [cx, cy] = inner.centre();
    let (right, bottom) = (inner.x + inner.w, inner.y + inner.h);
    let (outer_right, outer_bottom) = (outer.x + outer.w, outer.y + outer.h);
    [
        ([outer.x, cy], [inner.x, cy]),
        ([right, cy], [outer_right, cy]),
        ([cx, outer.y], [cx, inner.y]),
        ([cx, bottom], [cx, outer_bottom]),
    ]
    .into_iter()
    .map(|(a, b)| Distance {
        a,
        b,
        length: (b[0] - a[0]) + (b[1] - a[1]),
    })
    .collect()
}
