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
    /// The window the canvas shows through, not the frame, which a bar
    /// inset makes wider than the window.
    pub(crate) rect: Rect,
    /// The scissor the frame itself is drawn under: a frame scrolled out of
    /// its parent's window is not under the cursor, whatever its rect says.
    pub(crate) clip: Option<Rect>,
    /// How far `CanvasPosition` can go along each axis, `[0, range]`; zero
    /// where the canvas fits the window, `ScrollingDirection` excludes the
    /// axis or `ScrollingEnabled` is off.
    pub(crate) range: [f32; 2],
    /// `CanvasPosition` as drawn, clamped to the canvas.
    pub(crate) position: [f32; 2],
    /// Each axis's scroll bar thumb, where one is drawn.
    pub(crate) thumbs: [Option<Rect>; 2],
}

/// Every `ScrollingFrame` of one [`crate::Headless::render_gui`] canvas, in
/// paint order, for a host that scrolls them itself: by wheel
/// ([`GuiScrolls::wheel_target`]) and by dragging a bar's thumb
/// ([`GuiScrolls::thumb_at`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GuiScrolls(pub(crate) Vec<ScrollWindow>);

/// A press on a scroll bar's thumb: dragging it by one pixel along `axis`
/// moves `CanvasPosition` by `per_pixel`, from `position`, within
/// `[0, range]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThumbGrab {
    pub referent: Ref,
    pub axis: usize,
    pub position: [f32; 2],
    pub range: f32,
    pub per_pixel: f32,
}

impl GuiScrolls {
    /// See [`scroll_target`].
    pub fn wheel_target(&self, point: [f32; 2], axis: usize) -> Option<ScrollTarget> {
        scroll_target(&self.0, point, axis)
    }

    /// The thumb under `point`, topmost first. A thumb is the window's share
    /// of the canvas along a track the window's length, so a pixel of thumb
    /// travel is `slack / (track - thumb)` pixels of canvas.
    pub fn thumb_at(&self, point: [f32; 2]) -> Option<ThumbGrab> {
        self.0.iter().rev().find_map(|window| {
            (0..2).find_map(|axis| {
                let thumb = window.thumbs[axis]?;
                let visible = window.clip.is_none_or(|clip| contains(&clip, point));
                if !visible || !contains(&thumb, point) || window.range[axis] <= 0.0 {
                    return None;
                }
                let track = window.rect.size()[axis];
                let length = thumb.size()[axis];
                Some(ThumbGrab {
                    referent: window.referent,
                    axis,
                    position: window.position,
                    range: window.range[axis],
                    per_pixel: window.range[axis] / (track - length).max(1.0),
                })
            })
        })
    }
}

/// The frame the wheel over `point` scrolls, and how far it can go.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollTarget {
    pub referent: Ref,
    /// `CanvasPosition`'s reach along the wheel's axis: `[0, range]`.
    pub range: f32,
}

/// The innermost of `windows`, the last in paint order, under `point` that
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
            position: [0.0; 2],
            thumbs: [None; 2],
        }
    }

    #[test]
    fn a_thumb_drag_moves_the_canvas_by_its_share_of_the_track() {
        // A 100px window over a 400px canvas: a 25px thumb on a 100px track,
        // so 75px of travel covers the 300px of slack.
        let mut list = window(1, rect(0.0, 0.0, 100.0, 100.0), None, [0.0, 300.0]);
        list.position = [0.0, 30.0];
        list.thumbs[1] = Some(rect(94.0, 7.5, 6.0, 25.0));
        let scrolls = GuiScrolls(vec![list]);

        let grab = scrolls.thumb_at([96.0, 20.0]).unwrap();
        assert_eq!((grab.axis, grab.position[1], grab.range), (1, 30.0, 300.0));
        assert_eq!(grab.per_pixel, 4.0);
        assert!(scrolls.thumb_at([96.0, 50.0]).is_none());
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
