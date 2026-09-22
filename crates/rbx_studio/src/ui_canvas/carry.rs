//! Resizing and turning a whole selection at once, the way a Figma
//! selection box does: a frame round everything selected is what the
//! handles move, and each element follows it — its centre carried where
//! the frame takes that point, its own size stretched by however much the
//! frame stretched along the element's own axes.
//!
//! One element is the same thing with a frame that is its own box, so a
//! single element and a selection go through the one path.

use super::{rotate, Rect, Resize};

/// One carried element as drawn: its centre, its unturned size, and its
/// `AbsoluteRotation`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Carried {
    pub(crate) centre: [f32; 2],
    pub(crate) size: [f32; 2],
    pub(crate) rotation: f32,
}

/// What resizing `frame` — turned `turn` degrees about its centre — by
/// `step` does to each of `carried`: how far its centre travels on screen,
/// and how much it grows along its own axes.
///
/// The frame's stretch is a scale along its own two axes; an element turned
/// against the frame has its axes stretched by the length that scale gives
/// them, which is exact for the square-on case and keeps a turned element a
/// rectangle (not the parallelogram a true stretch would make) otherwise.
pub(crate) fn scale(frame: &Rect, turn: f32, step: &Resize, carried: &[Carried]) -> Vec<Moved> {
    let factor = [0, 1].map(|axis| {
        let (_, length) = frame.along(axis);
        match length > 0.0 {
            true => (length + step.grow[axis]) / length,
            false => 1.0,
        }
    });
    let pivot = frame.centre();
    carried
        .iter()
        .map(|element| {
            let local = rotate(minus(element.centre, pivot), -turn);
            let moved = [0, 1].map(|axis| local[axis] * factor[axis] + step.centre[axis]);
            let centre = plus(pivot, rotate(moved, turn));
            let (sin, cos) = (element.rotation - turn).to_radians().sin_cos();
            let along = [
                (factor[0] * cos).hypot(factor[1] * sin),
                (factor[0] * sin).hypot(factor[1] * cos),
            ];
            Moved {
                centre: minus(centre, element.centre),
                grow: [0, 1].map(|axis| element.size[axis] * (along[axis] - 1.0)),
            }
        })
        .collect()
}

/// How far each of `carried` travels when the selection turns `degrees`
/// about `pivot`; each also turns by `degrees` about its own centre.
pub(crate) fn turn(pivot: [f32; 2], degrees: f32, carried: &[Carried]) -> Vec<[f32; 2]> {
    carried
        .iter()
        .map(|element| {
            let turned = plus(pivot, rotate(minus(element.centre, pivot), degrees));
            minus(turned, element.centre)
        })
        .collect()
}

/// One element's share of a resize: see [`scale`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Moved {
    pub(crate) centre: [f32; 2],
    pub(crate) grow: [f32; 2],
}

fn minus(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn plus(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] + b[0], a[1] + b[1]]
}
