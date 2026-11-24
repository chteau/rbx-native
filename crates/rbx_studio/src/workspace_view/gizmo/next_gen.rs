//! The gestures Studio's "Next-Gen Draggers" beta adds to Move and Rotate
//! (see `rbx_viewer::gizmo::Skin`): a plane square slides the selection in
//! its plane, the view ring turns it about the line from the camera, and
//! the trackball inside the rings turns it freely. The view ring is an
//! ordinary [`Drag::Ring`] in its own frame; the other two are drags of
//! their own.

use glam::{Mat3, Quat, Vec2, Vec3};
use rbx_viewer::gizmo::{self, Handles};
use rbx_viewer::pick::{self, Ray};
use rbx_viewer::snap;

use super::{Change, Drag, Landing};
use crate::transform::Target;

/// What a press on a plane square opens.
pub(super) fn grab_plane(handles: &Handles, anchor: Target, ray: Ray) -> Option<Drag> {
    let axis = handles.grab_plane(ray)?;
    let (a, b, _) = handles.plane(axis);
    let normal = handles.direction(axis);
    let origin = anchor.position();
    let hit = pick::ray_hits_plane(ray, origin, normal)?;
    Some(Drag::Slide {
        origin,
        axes: [a, b],
        normal,
        grabbed: Vec2::new((hit - origin).dot(a), (hit - origin).dot(b)),
    })
}

/// What a press on Rotate opens once its axis rings came up empty: the
/// view ring, else the trackball.
pub(super) fn grab_rotate(handles: &Handles, anchor: Target, ray: Ray) -> Option<Drag> {
    if handles.grab_view_ring(ray) {
        let frame = handles.view_frame();
        let (angle, ..) = gizmo::ring_crossing(handles.origin(), frame, ray)?;
        return Some(Drag::Ring {
            origin: handles.origin(),
            frame,
            orientation: anchor.orientation(),
            last: angle,
            turned: 0.0,
        });
    }
    let from = handles.grab_trackball(ray)?;
    Some(Drag::Ball {
        origin: handles.origin(),
        radius: handles.trackball_radius(),
        from,
        orientation: anchor.orientation(),
    })
}

/// One step of a [`Drag::Slide`] or [`Drag::Ball`]. The slide's travel
/// along each axis lands on the grid like a Move arrow's; a trackball turn
/// is free, as Studio's is.
pub(super) fn advance(drag: Drag, ray: Ray, landing: Landing) -> Option<(Drag, Change)> {
    match drag {
        Drag::Slide {
            origin,
            axes: [a, b],
            normal,
            grabbed,
        } => {
            let offset = pick::ray_hits_plane(ray, origin, normal)? - origin;
            let travel =
                |axis: Vec3, from: f32| snap::round_to(offset.dot(axis) - from, landing.grid);
            let position = origin + a * travel(a, grabbed.x) + b * travel(b, grabbed.y);
            Some((drag, Change::Position(position)))
        }
        Drag::Ball {
            origin,
            radius,
            from,
            orientation,
        } => {
            let to = gizmo::trackball_point(origin, radius, ray);
            let turn = Mat3::from_quat(Quat::from_rotation_arc(from, to));
            Some((drag, Change::Orientation(turn * orientation)))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ray(from: Vec3, to: Vec3) -> Ray {
        Ray {
            origin: from,
            direction: (to - from).normalize(),
        }
    }

    fn landing(grid: f32) -> Landing<'static> {
        Landing {
            grid,
            angle: 0.0,
            snaps: &[],
        }
    }

    #[test]
    fn a_plane_slide_follows_the_cursor_in_its_plane_on_the_grid() {
        let drag = Drag::Slide {
            origin: Vec3::ZERO,
            axes: [Vec3::X, Vec3::Y],
            normal: Vec3::Z,
            grabbed: Vec2::new(0.5, 0.5),
        };
        let eye = Vec3::new(0.0, 0.0, 20.0);
        let (_, change) = advance(drag, ray(eye, Vec3::new(3.7, -1.2, 0.0)), landing(1.0)).unwrap();
        assert_eq!(change, Change::Position(Vec3::new(3.0, -2.0, 0.0)));
        let (_, free) = advance(drag, ray(eye, Vec3::new(3.7, -1.2, 0.0)), landing(0.0)).unwrap();
        let Change::Position(free) = free else {
            panic!("{free:?}")
        };
        assert!((free - Vec3::new(3.2, -1.7, 0.0)).length() < 1e-4);
    }

    #[test]
    fn the_trackball_turns_the_grabbed_point_to_the_cursor() {
        let drag = Drag::Ball {
            origin: Vec3::ZERO,
            radius: 1.0,
            from: Vec3::Z,
            orientation: Mat3::IDENTITY,
        };
        let eye = Vec3::new(0.0, 0.0, 20.0);
        let target = Vec3::new(0.6, 0.0, 0.8);
        let (_, change) = advance(drag, ray(eye, target), landing(1.0)).unwrap();
        let Change::Orientation(turn) = change else {
            panic!("{change:?}")
        };
        let to = gizmo::trackball_point(Vec3::ZERO, 1.0, ray(eye, target));
        assert!((turn * Vec3::Z - to).length() < 1e-4);
        assert!(to.x > 0.0);
    }
}
