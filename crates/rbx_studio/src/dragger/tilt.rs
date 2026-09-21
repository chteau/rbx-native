//! How a free drag turns what it carries (`DragHelper.getDragTargetNew`,
//! `snapRotationToPrimaryDirection`, `DragHelper.updateTiltRotate`).
//!
//! Nothing here is incremental. Every step starts again from the
//! selection's orientation at the grab, `basis`: seen from the target's
//! frame, and with Align Dragged Objects on, squared onto the frame's axes
//! by the smallest turn that does it — whichever of the selection's axes
//! lies nearest the face's normal goes onto it, and the others onto the
//! face's edges. The quarter turns `R` and `T` have added (`tilt`) come after
//! that, so they keep meaning "this many turns from lying on the face"
//! across every face the drag crosses.

use std::f32::consts::FRAC_PI_2;

use glam::{Mat3, Vec3};

use super::surface::SurfaceFrame;

/// Studio's `DIRS`, in its order: a tie keeps the earlier one.
const DIRECTIONS: [Vec3; 6] = [
    Vec3::X,
    Vec3::NEG_X,
    Vec3::Y,
    Vec3::NEG_Y,
    Vec3::Z,
    Vec3::NEG_Z,
];

/// The first of [`DIRECTIONS`] `score` rates highest.
fn best(score: impl Fn(Vec3) -> f32) -> Vec3 {
    let mut best = (f32::NEG_INFINITY, Vec3::Y);
    for direction in DIRECTIONS {
        let value = score(direction);
        if value > best.0 {
            best = (value, direction);
        }
    }
    best.1
}

/// `snapRotationToPrimaryDirection`: `rotation` snapped to the nearest of
/// the 24 turns that map axes onto axes. The column nearest an axis snaps
/// first, then the next nearest; the third follows by a cross product.
pub(crate) fn snap_to_primary(rotation: Mat3) -> Mat3 {
    let largest = |v: Vec3| v.abs().max_element();
    let closest = |v: Vec3| best(|d| v.dot(d));
    let cross = |a: Vec3, b: Vec3| a.cross(b).normalize_or_zero();
    let (mut r, mut u, mut b) = (rotation.x_axis, rotation.y_axis, rotation.z_axis);
    let (mr, mu, mb) = (largest(r), largest(u), largest(b));
    if mu < mr && mb < mr {
        r = closest(r);
        if mb < mu {
            u = closest(u);
        } else {
            b = closest(b);
            u = cross(b, r);
        }
    } else if mb < mu {
        u = closest(u);
        if mb < mr {
            r = closest(r);
        } else {
            b = closest(b);
            r = cross(u, b);
        }
    } else {
        b = closest(b);
        if mu < mr {
            r = closest(r);
            u = cross(b, r);
        } else {
            u = closest(u);
            r = cross(u, b);
        }
    }
    Mat3::from_cols(r, u, r.cross(u))
}

/// A target frame's axes, as the rotation from its own space to the world's.
pub(crate) fn frame_rotation(frame: &SurfaceFrame) -> Mat3 {
    Mat3::from_cols(frame.x, frame.y, frame.z)
}

/// Studio's `rot`: the grab-time orientation `basis` in `frame`'s space,
/// squared onto its axes when `align` — Align Dragged Objects on and `Alt`
/// up.
pub(crate) fn in_frame(frame: &SurfaceFrame, basis: Mat3, align: bool) -> Mat3 {
    let rotation = frame_rotation(frame).transpose() * basis;
    if align {
        snap_to_primary(rotation)
    } else {
        rotation
    }
}

/// One `R` or `T` press: a quarter turn, right-handed, about whichever axis
/// of the selection as it lies on `frame` (before any earlier turn) points
/// most nearly along `axis` — the face's normal for `R`, the camera's right
/// for `T` — added in front of the turns made so far.
pub(crate) fn turned(
    frame: &SurfaceFrame,
    basis: Mat3,
    tilt: Mat3,
    align: bool,
    axis: Vec3,
) -> Mat3 {
    let lying = frame_rotation(frame) * in_frame(frame, basis, align);
    let about = best(|d| (lying * d).dot(axis));
    let quarter = Mat3::from_axis_angle(about, FRAC_PI_2);
    // `roundRotation`: exact zeros and ones, so turns compose without drift.
    Mat3::from_cols_array(&quarter.to_cols_array().map(|v| (v + 0.5).floor())) * tilt
}

#[cfg(test)]
mod tests {
    use glam::Vec2;

    use super::*;
    use crate::dragger::surface::TargetKind;

    fn close(a: Mat3, b: Mat3) -> bool {
        a.abs_diff_eq(b, 1e-5)
    }

    fn face(x: Vec3, y: Vec3) -> SurfaceFrame {
        SurfaceFrame {
            corner: Vec3::ZERO,
            x,
            y,
            z: x.cross(y),
            size: Vec2::ZERO,
            kind: TargetKind::Polygon,
            part: None,
        }
    }

    #[test]
    fn a_yaw_under_45_degrees_snaps_back_and_one_over_snaps_on() {
        assert!(close(
            snap_to_primary(Mat3::from_rotation_y(0.7)),
            Mat3::IDENTITY
        ));
        let snapped = snap_to_primary(Mat3::from_rotation_y(0.9));
        assert!(
            close(snapped, Mat3::from_rotation_y(FRAC_PI_2)),
            "{snapped}"
        );
    }

    #[test]
    fn a_shallow_slope_lays_the_part_flat_on_it() {
        // A wedge rising 1 in 2 facing +Y-and-back: the part's own up is the
        // axis nearest the slope's normal, so it goes onto it.
        let normal = Vec3::new(0.0, 2.0, 1.0).normalize();
        let slope = face(Vec3::X, normal);
        let rot = in_frame(&slope, Mat3::IDENTITY, true);
        let world = frame_rotation(&slope) * rot;
        assert!((world.y_axis - normal).length() < 1e-5, "{}", world.y_axis);
        assert!((world.x_axis - Vec3::X).length() < 1e-5);
    }

    #[test]
    fn a_steep_slope_stands_the_part_on_the_face_nearest_it() {
        // Rising 2 in 1: the part's back (+Z) is nearer the normal than its
        // top, so it stands on its back face instead.
        let normal = Vec3::new(0.0, 1.0, 2.0).normalize();
        let slope = face(Vec3::X, normal);
        let world = frame_rotation(&slope) * in_frame(&slope, Mat3::IDENTITY, true);
        assert!((world.z_axis - normal).length() < 1e-5, "{}", world.z_axis);
    }

    #[test]
    fn holding_orientation_keeps_the_grab_time_turn_on_any_face() {
        let basis = Mat3::from_rotation_y(0.3);
        let slope = face(Vec3::X, Vec3::new(0.0, 2.0, 1.0).normalize());
        let world = frame_rotation(&slope) * in_frame(&slope, basis, false);
        assert!(close(world, basis));
    }

    #[test]
    fn r_spins_about_the_faces_normal_and_t_tips_about_the_view_right() {
        let floor = face(Vec3::X, Vec3::Y);
        let spun = turned(&floor, Mat3::IDENTITY, Mat3::IDENTITY, true, Vec3::Y);
        assert!(close(spun, Mat3::from_rotation_y(FRAC_PI_2)));
        // A second press adds another quarter, exactly.
        let twice = turned(&floor, Mat3::IDENTITY, spun, true, Vec3::Y);
        let half_turn = Mat3::from_rotation_y(std::f32::consts::PI).to_cols_array();
        assert_eq!(twice, Mat3::from_cols_array(&half_turn.map(f32::round)));
        // T with the camera's right along world -X turns about -X.
        let tipped = turned(
            &floor,
            Mat3::IDENTITY,
            Mat3::IDENTITY,
            true,
            Vec3::new(-0.9, 0.1, 0.2),
        );
        assert!(close(tipped, Mat3::from_rotation_x(-FRAC_PI_2)));
    }
}
