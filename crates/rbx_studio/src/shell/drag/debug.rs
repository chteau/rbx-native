//! `RBX_STUDIO_DRAG` and `RBX_STUDIO_RESIZE`: one drag step each, applied
//! once at startup through the exact `Shell::move_parts`/`Shell::resize_part`
//! a real gizmo gesture ends with — debugging aids for a screenshot, or for
//! an `RBX_STUDIO_UNDO=1` run that reverts one, since nothing can send the
//! viewport a real mouse drag on the editor's behalf (see `AGENTS.md`'s
//! safety rules). Both are no-ops with nothing selected, or a target-less
//! selection (a `Folder`, a `Model` — nothing with a placement to change).

use glam::{Mat3, Vec3};
use gpui_kit::Context;

use crate::transform::Targets;

use super::Shell;

/// `RBX_STUDIO_DRAG=<dx>,<dy>,<dz>`: moves every selected part by this
/// world-space offset, preserving their layout relative to each other and to
/// the gizmo's anchor, through the exact same [`Targets::translate`] and
/// `Shell::move_parts` a real gizmo or cursor drag ends a mouse gesture with
/// — the aid for a group drag.
pub(crate) const DRAG_VARIABLE: &str = "RBX_STUDIO_DRAG";

/// `RBX_STUDIO_RESIZE=<dx>,<dy>,<dz>`: grows the selected part's size by this
/// much along its own axes, holding the face opposite each grown one still
/// the way a Scale handle does, through the same `Shell::resize_part` a real
/// Scale drag ends with — the aid for the Scale tool. Only the anchor part,
/// exactly as a Scale drag over a multi-part selection resizes only that
/// (see `Targets::set_anchor`).
pub(crate) const RESIZE_VARIABLE: &str = "RBX_STUDIO_RESIZE";

impl Shell {
    /// [`DRAG_VARIABLE`]: documented on its own doc comment.
    pub(crate) fn apply_debug_drag(&mut self, cx: &mut Context<Self>) {
        let Ok(spec) = std::env::var(DRAG_VARIABLE) else {
            return;
        };
        let Some(delta) = parse_delta(&spec) else {
            eprintln!("rbxstudio: {DRAG_VARIABLE}: expected <dx>,<dy>,<dz>, got {spec:?}");
            return;
        };

        let mut targets = Targets::read(&self.dom, &self.database, self.selected_all());
        let moves = targets.translate(delta);
        if !moves.is_empty() {
            self.move_parts(&moves, true, None, cx);
        }
    }

    /// [`RESIZE_VARIABLE`]: documented on its own doc comment.
    pub(crate) fn apply_debug_resize(&mut self, cx: &mut Context<Self>) {
        let Ok(spec) = std::env::var(RESIZE_VARIABLE) else {
            return;
        };
        let Some(growth) = parse_delta(&spec) else {
            eprintln!("rbxstudio: {RESIZE_VARIABLE}: expected <dx>,<dy>,<dz>, got {spec:?}");
            return;
        };

        let Some(anchor) = Targets::read(&self.dom, &self.database, self.selected_all()).anchor() else {
            return;
        };
        let Some((size, position)) = grown(
            anchor.size(),
            anchor.position(),
            anchor.orientation(),
            growth,
        ) else {
            eprintln!(
                "rbxstudio: {RESIZE_VARIABLE}: {spec:?} leaves the part no size on some axis"
            );
            return;
        };
        self.resize_part(anchor.referent, size, position, true, cx);
    }
}

/// Parses `"<x>,<y>,<z>"` into a three-axis offset, or `None` for anything
/// else — the only format either variable takes.
fn parse_delta(spec: &str) -> Option<Vec3> {
    let mut fields = spec.split(',').map(str::trim);
    let x = fields.next()?.parse().ok()?;
    let y = fields.next()?.parse().ok()?;
    let z = fields.next()?.parse().ok()?;
    fields.next().is_none().then_some(Vec3::new(x, y, z))
}

/// Where a part of `size`, centred at `position` and facing `orientation`,
/// stands once each of its axes has grown by `growth`: the centre moves by
/// half the growth along each of the part's own axes, so the face opposite
/// each grown one holds still — the rule a Scale handle applies to the one
/// axis it drags (see `workspace_view::gizmo`'s `Drag::Size`). `None` when
/// the growth would leave no size on some axis, the one thing a handle's own
/// clamping never lets through.
fn grown(size: Vec3, position: Vec3, orientation: Mat3, growth: Vec3) -> Option<(Vec3, Vec3)> {
    let size = size + growth;
    (size.min_element() > 0.0).then(|| (size, position + orientation * (growth * 0.5)))
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;

    use glam::{Mat3, Vec3};

    use super::{grown, parse_delta};

    #[test]
    fn three_comma_separated_numbers_parse_as_an_offset() {
        assert_eq!(parse_delta("1, -2.5, 0"), Some(Vec3::new(1.0, -2.5, 0.0)));
    }

    #[test]
    fn anything_else_is_rejected_rather_than_guessed_at() {
        assert_eq!(parse_delta(""), None);
        assert_eq!(parse_delta("1,2"), None, "too few fields");
        assert_eq!(parse_delta("1,2,3,4"), None, "too many fields");
        assert_eq!(parse_delta("x,2,3"), None, "not a number");
    }

    #[test]
    fn growing_one_axis_moves_the_centre_half_way_along_it() {
        let (size, position) = grown(
            Vec3::new(4.0, 1.0, 2.0),
            Vec3::ZERO,
            Mat3::IDENTITY,
            Vec3::new(2.0, 0.0, 0.0),
        )
        .expect("a positive size");

        assert_eq!(size, Vec3::new(6.0, 1.0, 2.0));
        assert_eq!(
            position,
            Vec3::new(1.0, 0.0, 0.0),
            "the -X face stays put, so the centre follows the +X face half way"
        );
    }

    #[test]
    fn the_centre_moves_along_the_parts_own_axis_not_the_worlds() {
        // Turned a quarter around Y, the part's own +X points down world -Z.
        let orientation = Mat3::from_rotation_y(FRAC_PI_2);
        let (_, position) = grown(Vec3::ONE, Vec3::ZERO, orientation, Vec3::new(2.0, 0.0, 0.0))
            .expect("a positive size");

        assert!(
            position.abs_diff_eq(Vec3::new(0.0, 0.0, -1.0), 1e-5),
            "got {position}"
        );
    }

    #[test]
    fn shrinking_an_axis_to_nothing_is_rejected() {
        assert_eq!(
            grown(
                Vec3::ONE,
                Vec3::ZERO,
                Mat3::IDENTITY,
                Vec3::new(0.0, -1.0, 0.0)
            ),
            None,
            "a zero-thick part is not a size a handle could ever produce"
        );
        assert!(grown(
            Vec3::ONE,
            Vec3::ZERO,
            Mat3::IDENTITY,
            Vec3::new(0.0, -0.5, 0.0)
        )
        .is_some());
    }
}
