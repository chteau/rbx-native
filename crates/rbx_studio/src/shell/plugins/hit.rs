//! Which element of a plugin's rendered GUI the pointer is over.

use std::collections::HashSet;

use rbx_dom::Ref;
use rbx_viewer::GuiBox;

fn contains(b: &GuiBox, x: f32, y: f32) -> bool {
    let [bx, by, bw, bh] = b.rect;
    x >= bx && y >= by && x < bx + bw && y < by + bh
}

/// The topmost interactive element (a button, text box, scrolling frame or
/// `Active` object — `render::Drawn::sinks`) under the point, as in Roblox,
/// where a button's own label lets the press through to it; otherwise the
/// topmost box. Skips the root's own frame (`boxes[0]`); rotation is
/// ignored.
pub(super) fn hit(boxes: &[GuiBox], sinks: &HashSet<Ref>, x: f32, y: f32) -> Option<Ref> {
    let mut under = boxes.iter().skip(1).rev().filter(|b| contains(b, x, y));
    let top = under.clone().next().map(|b| b.referent);
    under
        .find(|b| sinks.contains(&b.referent))
        .map(|b| b.referent)
        .or(top)
}

/// Whether any interactive element is under the point — what keeps a click
/// on an overlay's button from also reaching the 3D view below it.
pub(super) fn over_sink(boxes: &[GuiBox], sinks: &HashSet<Ref>, x: f32, y: f32) -> bool {
    boxes
        .iter()
        .skip(1)
        .any(|b| sinks.contains(&b.referent) && contains(b, x, y))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boxed(referent: Ref, rect: [f32; 4]) -> GuiBox {
        GuiBox {
            referent,
            rect,
            rotation: 0.0,
            content: None,
            size_scale: 1.0,
            aspect: false,
        }
    }

    #[test]
    fn a_label_on_a_button_lets_the_press_through_to_it() {
        let (root, button, label, beside) = (Ref::new(1), Ref::new(2), Ref::new(3), Ref::new(4));
        let boxes = [
            boxed(root, [0.0, 0.0, 100.0, 100.0]),
            boxed(button, [10.0, 10.0, 50.0, 20.0]),
            boxed(label, [10.0, 10.0, 50.0, 20.0]),
            boxed(beside, [70.0, 10.0, 20.0, 20.0]),
        ];
        let sinks = HashSet::from([button]);
        assert_eq!(hit(&boxes, &sinks, 20.0, 15.0), Some(button));
        assert_eq!(hit(&boxes, &sinks, 75.0, 15.0), Some(beside));
        assert_eq!(hit(&boxes, &sinks, 5.0, 90.0), None);
        assert!(over_sink(&boxes, &sinks, 20.0, 15.0));
        assert!(!over_sink(&boxes, &sinks, 75.0, 15.0));
    }
}
