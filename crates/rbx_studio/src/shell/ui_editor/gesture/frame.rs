//! Where a selection's frame stands and what a gesture in flight shows,
//! ahead of the canvas catching up with its writes.

use rbx_dom::Ref;

use super::{Gesture, Held};
use crate::ui_canvas::carry;
use crate::ui_canvas::Rect;

/// The frame a selection's handles stand on: one element's own box, turned
/// as it is; several elements' boxes, bounded square to the screen, the way
/// a Figma selection box is.
pub(in crate::shell::ui_editor) fn frame_of(
    boxes: impl IntoIterator<Item = (Rect, f32)>,
) -> Option<(Rect, f32)> {
    let mut boxes = boxes.into_iter();
    let first = boxes.next()?;
    let Some(second) = boxes.next() else {
        return Some(first);
    };
    let bounds = [second]
        .into_iter()
        .chain(boxes)
        .fold(first.0.turned_bounds(first.1), |bounds, (rect, turn)| {
            bounds.union(&rect.turned_bounds(turn))
        });
    Some((bounds, 0.0))
}

/// Where a gesture in flight has the selection's frame, ahead of the
/// canvas catching up: `None` for one that does not move the frame itself.
pub(in crate::shell::ui_editor) fn frame(gesture: &Gesture) -> Option<(Rect, f32)> {
    match gesture {
        Gesture::Resize { shown, .. } => Some(*shown),
        Gesture::Rotate {
            frame: (rect, turn),
            delta,
            ..
        } => Some((*rect, turn + delta)),
        _ => None,
    }
}

/// What a gesture in flight shows for what it carries, ahead of the canvas
/// catching up with the writes: each element's box and turn.
pub(in crate::shell::ui_editor) fn preview(gesture: &Gesture) -> Vec<(Ref, Rect, f32)> {
    match gesture {
        Gesture::Move { held, shift, .. } => held
            .iter()
            .map(|h| (h.referent, h.rect.shifted(*shift), h.rotation))
            .collect(),
        Gesture::Resize { preview, .. } => preview.clone(),
        Gesture::Rotate {
            held, frame, delta, ..
        } => {
            let carried: Vec<_> = held.iter().map(Held::carried).collect();
            let moved = carry::turn(frame.0.centre(), *delta, &carried);
            held.iter()
                .zip(moved)
                .map(|(h, shift)| (h.referent, h.rect.shifted(shift), h.rotation + delta))
                .collect()
        }
        _ => Vec::new(),
    }
}
