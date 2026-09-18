//! The live stud-count readout shown near the cursor while a Move or Scale
//! drag is in progress: how far the selection has travelled, or how much it
//! has grown, since the handle was grabbed.
//!
//! An rbx-native addition, not a Roblox Studio one — the roadmap item this
//! implements says so explicitly. `gizmo.rs`'s own drag math already knows
//! the distance or growth by the time `WorkspaceView::drag_to` runs; this
//! module only formats that number and places it on screen, the same split
//! `label.rs` keeps between computing a number and laying it out.

use gpui_kit::{px, Pixels, Point, SharedString};

/// Fixed screen offset from the cursor, up and to the right of it — clear of
/// the pointer itself, with no attempt at collision-avoidance against other
/// UI (see the roadmap item's own "keep this lazy" scope note).
const OFFSET_X: Pixels = px(16.0);
const OFFSET_Y: Pixels = px(24.0);

/// The straight-line distance moved so far, in studs, to Studio's own
/// numeric-field precision (two decimal places).
pub(super) fn moved(distance: f32) -> SharedString {
    SharedString::from(format!("{distance:.2} studs"))
}

/// How much the dragged axis has grown (or, negative, shrunk) so far, in
/// studs — signed, since a scale drag can go either way and a shrink should
/// read distinctly from a move's always-positive distance.
pub(super) fn grown(delta: f32) -> SharedString {
    SharedString::from(format!("{delta:+.2} studs"))
}

/// Where the readout sits: a fixed offset from the cursor, converted from
/// the window-relative position `MouseMoveEvent` reports into the
/// panel-relative coordinates `.absolute()` positions a child against — the
/// same origin subtraction `WorkspaceView::cursor_ray` already does to
/// unproject a click, kept in logical pixels here instead of device ones
/// since this places a GPUI element rather than casting a ray.
pub(super) fn position(
    cursor: Point<Pixels>,
    viewport_origin: (u32, u32),
    scale: f32,
) -> Point<Pixels> {
    Point::new(
        cursor.x - px(viewport_origin.0 as f32 / scale) + OFFSET_X,
        cursor.y - px(viewport_origin.1 as f32 / scale) - OFFSET_Y,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_move_reads_as_a_plain_distance() {
        assert_eq!(moved(3.0_f32.hypot(4.0)), "5.00 studs");
    }

    #[test]
    fn a_grow_reads_with_an_explicit_sign() {
        assert_eq!(grown(2.5), "+2.50 studs");
    }

    #[test]
    fn a_shrink_reads_with_a_minus_sign() {
        assert_eq!(grown(-1.1), "-1.10 studs");
    }

    #[test]
    fn the_readout_lands_up_and_right_of_a_cursor_flush_with_the_panel() {
        let point = position(Point::new(px(100.0), px(80.0)), (0, 0), 1.0);
        assert_eq!(point.x, px(116.0));
        assert_eq!(point.y, px(56.0));
    }

    // A HiDPI display (scale 2.0) whose panel starts 40 physical pixels in
    // from the window's own left edge — 20 logical pixels the cursor's
    // window-relative position has to be corrected for before the fixed
    // offset is added.
    #[test]
    fn a_panels_own_offset_in_the_window_is_subtracted_first() {
        let point = position(Point::new(px(120.0), px(80.0)), (40, 0), 2.0);
        assert_eq!(point.x, px(120.0 - 20.0 + 16.0));
    }
}
