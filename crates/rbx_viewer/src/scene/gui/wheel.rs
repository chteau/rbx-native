//! What the mouse wheel over a point of the screen overlay would scroll: the
//! innermost `ScrollingFrame` whose window holds the point and whose canvas
//! actually overflows it along the wheel's axis.
//!
//! The docs describe the wheel's effect only in passing (`UIPageLayout.
//! ScrollWheelInputEnabled`: "scrolling down moves to the next page"); how
//! far a notch scrolls a `ScrollingFrame` is stated nowhere, so the step is
//! the host's call. Nothing here is drawn: the layout leaves one
//! [`ScrollWindow`] per frame behind, in paint order, and [`scroll_target`]
//! reads them back.

use rbx_dom::Ref;

use super::layout::Rect;

/// A `ScrollingFrame`'s window as laid out, with what the wheel needs to
/// know about it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ScrollWindow {
    pub(crate) referent: Ref,
    /// The window the canvas shows through — not the frame, which a bar
    /// inset makes wider than the window.
    pub(crate) rect: Rect,
    /// The scissor the frame itself is drawn under: a frame scrolled out of
    /// its parent's window is not under the cursor, whatever its rect says.
    pub(crate) clip: Option<Rect>,
    /// How far `CanvasPosition` can go along each axis — `[0, range]`; zero
    /// where the canvas fits the window, `ScrollingDirection` excludes the
    /// axis or `ScrollingEnabled` is off.
    pub(crate) range: [f32; 2],
}

/// The frame the wheel over `point` scrolls, and how far it can go.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollTarget {
    pub referent: Ref,
    /// `CanvasPosition`'s reach along the wheel's axis: `[0, range]`.
    pub range: f32,
}

/// The innermost of `windows` — the last in paint order — under `point` that
/// can scroll along `axis` (0 across, 1 down). A frame that cannot is looked
/// through, so the wheel over a fitted list still scrolls the list around it.
///
/// ponytail: a rotated frame is hit against its unrotated box; the layout
/// carries no rotated geometry to test against, and a wheel over a turned
/// list is rare enough to wait for a complaint.
pub(crate) fn scroll_target(
    windows: &[ScrollWindow],
    point: [f32; 2],
    axis: usize,
) -> Option<ScrollTarget> {
    windows
        .iter()
        .rev()
        .find(|window| {
            window.range[axis] > 0.0
                && contains(&window.rect, point)
                && window.clip.is_none_or(|clip| contains(&clip, point))
        })
        .map(|window| ScrollTarget {
            referent: window.referent,
            range: window.range[axis],
        })
}

fn contains(rect: &Rect, [x, y]: [f32; 2]) -> bool {
    x >= rect.x && y >= rect.y && x < rect.x + rect.width && y < rect.y + rect.height
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    fn window(id: u32, rect: Rect, clip: Option<Rect>, range: [f32; 2]) -> ScrollWindow {
        ScrollWindow {
            referent: Ref::new(id),
            rect,
            clip,
            range,
        }
    }

    #[test]
    fn the_innermost_frame_under_the_point_wins() {
        let outer = window(1, rect(0.0, 0.0, 400.0, 400.0), None, [0.0, 600.0]);
        let inner = window(2, rect(50.0, 50.0, 100.0, 100.0), None, [0.0, 300.0]);
        let windows = [outer, inner];

        let hit = scroll_target(&windows, [60.0, 60.0], 1).unwrap();
        assert_eq!(hit.referent, inner.referent);
        assert_eq!(hit.range, 300.0);
        let hit = scroll_target(&windows, [300.0, 300.0], 1).unwrap();
        assert_eq!(hit.referent, outer.referent);
        assert!(scroll_target(&windows, [500.0, 60.0], 1).is_none());
    }

    #[test]
    fn a_frame_scrolled_out_of_its_parents_window_is_not_hit() {
        let clipped = window(
            1,
            rect(0.0, 150.0, 100.0, 100.0),
            Some(rect(0.0, 0.0, 100.0, 100.0)),
            [0.0, 50.0],
        );
        assert!(scroll_target(&[clipped], [10.0, 160.0], 1).is_none());
    }

    #[test]
    fn a_frame_that_cannot_scroll_along_the_axis_is_looked_through() {
        let outer = window(1, rect(0.0, 0.0, 400.0, 400.0), None, [0.0, 600.0]);
        let sideways = window(2, rect(0.0, 0.0, 100.0, 100.0), None, [200.0, 0.0]);
        let disabled = window(3, rect(0.0, 0.0, 100.0, 100.0), None, [0.0, 0.0]);

        let hit = scroll_target(&[outer, sideways], [10.0, 10.0], 1).unwrap();
        assert_eq!(hit.referent, outer.referent);
        let hit = scroll_target(&[outer, sideways], [10.0, 10.0], 0).unwrap();
        assert_eq!(hit.referent, sideways.referent);
        assert!(scroll_target(&[disabled], [10.0, 10.0], 1).is_none());
    }
}
