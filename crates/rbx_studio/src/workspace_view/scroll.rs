//! The wheel over a `ScrollingFrame` drawn in the viewport: Studio's edit
//! view scrolls the frame's canvas rather than zooming the camera, so the
//! render thread — the only side that knows where the overlay's frames were
//! laid out (see `rbx_viewer::Headless::gui_scroll_target`) — is asked first,
//! and the camera only gets the notch nothing scrollable was under.
//!
//! `RBX_STUDIO_SCROLL=<x>,<y>,<dx>,<dy>` injects one such wheel event at
//! viewport pixel `(x, y)` with `dx`/`dy` notches (`dy` positive away from
//! the user) once the first frame is up — a screenshot aid, since nothing
//! else can roll the wheel over the viewport on the editor's behalf.

use gpui_kit::{Pixels, Point, ScrollDelta};
use rbx_dom::Ref;
use rbx_viewer::ScrollTarget;

use super::input::{wheel_notches, wheel_scroll, Wheel};
use super::WorkspaceView;

const SCROLL_VARIABLE: &str = "RBX_STUDIO_SCROLL";

/// Canvas pixels one wheel notch scrolls. The docs state no step for a
/// `ScrollingFrame` (see `rbx_viewer`'s `scene::gui::wheel`), so this is a
/// plain desktop list's notch; a whole window per notch would skip rows.
pub(crate) const WHEEL_STEP: f32 = 100.0;

/// One wheel notch that landed on a scrolling frame, as the render thread
/// resolved it: what to scroll, along which axis, by how much, and how far
/// `CanvasPosition` can go along that axis (`[0, range]`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Scroll {
    pub(crate) referent: Ref,
    pub(crate) axis: usize,
    pub(crate) delta: f32,
    pub(crate) range: f32,
}

impl Scroll {
    /// A notch away from the user (positive) shows what is above, which is
    /// the canvas moving *down* the window: `CanvasPosition` shrinks.
    pub(crate) fn of(target: ScrollTarget, wheel: Wheel) -> Self {
        Scroll {
            referent: target.referent,
            axis: wheel.axis,
            delta: -wheel.notches * WHEEL_STEP,
            range: target.range,
        }
    }
}

/// `CanvasPosition` after `scroll`, from `current`: the one axis moves, and
/// never past either end of the canvas.
pub(crate) fn scrolled(current: [f32; 2], scroll: &Scroll) -> [f32; 2] {
    let mut next = current;
    next[scroll.axis] = (current[scroll.axis] + scroll.delta).clamp(0.0, scroll.range.max(0.0));
    next
}

/// The wheel event `RBX_STUDIO_SCROLL` asks for, if set and well-formed.
pub(super) fn debug_wheel() -> Option<([f32; 2], Wheel)> {
    let spec = std::env::var(SCROLL_VARIABLE).ok()?;
    parse(&spec)
}

fn parse(spec: &str) -> Option<([f32; 2], Wheel)> {
    let mut numbers = spec.split(',').map(|part| part.trim().parse::<f32>().ok());
    let [x, y, dx, dy] = [
        numbers.next()??,
        numbers.next()??,
        numbers.next()??,
        numbers.next()??,
    ];
    let wheel = match dy != 0.0 {
        true => Wheel {
            axis: 1,
            notches: dy,
        },
        false => Wheel {
            axis: 0,
            notches: dx,
        },
    };
    Some(([x, y], wheel))
}

impl WorkspaceView {
    /// A wheel event over the panel: handed to the render thread with where
    /// it landed in the frame's own pixels, to scroll a frame there or,
    /// failing that, to step the camera as `wheel_notches` reads it.
    pub(super) fn wheel(
        &self,
        position: Point<Pixels>,
        delta: ScrollDelta,
        shift: bool,
        scale: f32,
    ) {
        let viewport = self.viewport.get();
        // Same logical-to-physical step a click takes (see `cursor_ray`).
        let at = [
            f32::from(position.x) * scale - viewport.origin.0 as f32,
            f32::from(position.y) * scale - viewport.origin.1 as f32,
        ];
        self.pump
            .wheel(at, wheel_scroll(delta, shift), wheel_notches(delta));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scroll(axis: usize, delta: f32, range: f32) -> Scroll {
        Scroll {
            referent: Ref::new(1),
            axis,
            delta,
            range,
        }
    }

    #[test]
    fn a_notch_towards_the_user_moves_the_canvas_up_by_one_step() {
        let target = ScrollTarget {
            referent: Ref::new(1),
            range: 500.0,
        };
        let down = Scroll::of(
            target,
            Wheel {
                axis: 1,
                notches: -1.0,
            },
        );
        assert_eq!(down.delta, WHEEL_STEP);
        assert_eq!(scrolled([0.0, 40.0], &down), [0.0, 40.0 + WHEEL_STEP]);
    }

    #[test]
    fn the_canvas_stops_at_either_end() {
        assert_eq!(scrolled([0.0, 30.0], &scroll(1, -100.0, 500.0)), [0.0, 0.0]);
        assert_eq!(
            scrolled([0.0, 450.0], &scroll(1, 100.0, 500.0)),
            [0.0, 500.0]
        );
        assert_eq!(scrolled([10.0, 0.0], &scroll(0, 100.0, 50.0)), [50.0, 0.0]);
    }

    #[test]
    fn the_other_axis_is_left_alone() {
        assert_eq!(scrolled([70.0, 20.0], &scroll(1, 5.0, 500.0)), [70.0, 25.0]);
    }

    #[test]
    fn the_debug_spec_names_a_point_and_an_axis() {
        let (at, wheel) = parse("120, 300, 0, -2").unwrap();
        assert_eq!(at, [120.0, 300.0]);
        assert_eq!((wheel.axis, wheel.notches), (1, -2.0));
        let (_, across) = parse("1,2,3,0").unwrap();
        assert_eq!((across.axis, across.notches), (0, 3.0));
        assert!(parse("1,2,3").is_none());
        assert!(parse("a,b,c,d").is_none());
    }
}
