//! The camera's view frustum, in world space — what the main render pass
//! culls a drawable's bounding sphere against before it costs the GPU
//! anything (see `renderer::cull::MainCull`).
//!
//! Built from [`super::Camera::frustum_corners`], which already unprojects
//! the exact matrix a frame is drawn with — reusing it here (rather than
//! re-deriving planes from the projection matrix by hand) keeps the cull test
//! in lock-step with the actual projection, and it is already covered by
//! `super::tests::every_corner_stays_inside_the_frustum`.

use glam::Vec3;

use super::{Camera, Viewpoint};

/// Where the far corners are placed to build the near/left/right/top/bottom
/// planes below. Those planes all pass through the eye, so their orientation
/// does not depend on this distance — it only has to be finite and far enough
/// for the cross products that define them to stay numerically well
/// conditioned. Distance culling is a wholly separate, spherical test (see
/// `renderer::cull::MainCull`), so this never gates what is actually drawn.
const PLANE_CONSTRUCTION_DISTANCE: f32 = 10_000.0;

/// One half-space, `normal`-side positive: `distance` is how far inside it a
/// point sits, negative once the point has crossed to the outside.
#[derive(Debug, Clone, Copy)]
struct Plane {
    normal: Vec3,
    d: f32,
}

impl Plane {
    fn distance(&self, point: Vec3) -> f32 {
        self.normal.dot(point) + self.d
    }
}

/// The camera's view frustum for one frame, near/left/right/top/bottom only.
///
/// No far plane: the render-distance cutoff this pairs with is a spherical
/// test against the eye (see `renderer::cull::MainCull`), matching the
/// distance the shader's own edge-of-view fade already measures, rather than
/// a planar cut that would fade corners of the screen sooner than the centre.
pub(crate) struct Frustum {
    planes: [Plane; 5],
}

impl Frustum {
    pub(crate) fn new(camera: &Camera, from: Viewpoint, aspect: f32) -> Self {
        let corners = camera.frustum_corners(from, aspect, PLANE_CONSTRUCTION_DISTANCE);
        // Any point strictly inside every plane works as the "inside" side to
        // orient them toward; the corners' own centroid always qualifies.
        let centroid = corners.iter().fold(Vec3::ZERO, |sum, &corner| sum + corner) / 8.0;

        let plane_through = |a: usize, b: usize, c: usize| -> Plane {
            let (p0, p1, p2) = (corners[a], corners[b], corners[c]);
            let normal = (p1 - p0).cross(p2 - p0).normalize_or_zero();
            let d = -normal.dot(p0);
            if normal.dot(centroid) + d < 0.0 {
                Plane {
                    normal: -normal,
                    d: -d,
                }
            } else {
                Plane { normal, d }
            }
        };

        // Corner indices follow `frustum_corners`: bit 0 is +x, bit 1 is +y,
        // bit 2 is the far depth rather than the near one.
        Frustum {
            planes: [
                plane_through(0, 1, 2), // near
                plane_through(0, 2, 4), // left  (x = -1)
                plane_through(1, 3, 5), // right (x = +1)
                plane_through(0, 4, 1), // bottom (y = -1)
                plane_through(2, 3, 6), // top   (y = +1)
            ],
        }
    }

    /// Whether a bounding sphere could still show on screen. The boundary
    /// counts as visible — a sphere merely touching a plane is kept, not
    /// culled — with a small epsilon so float rounding in the plane
    /// construction itself can only ever err toward drawing something that
    /// turns out fully hidden, never the other way round.
    pub(crate) fn visible(&self, center: Vec3, radius: f32) -> bool {
        const TOUCHING_EPSILON: f32 = 1e-3;
        self.planes
            .iter()
            .all(|plane| plane.distance(center) >= -radius - TOUCHING_EPSILON)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::Pose;
    use crate::scene::tests_support::bounds_from;

    fn frustum() -> Frustum {
        let camera = Camera::framing(&bounds_from(Vec3::splat(-10.0), Vec3::splat(10.0)));
        let pose = Pose {
            position: Vec3::new(0.0, 0.0, 20.0),
            yaw: 0.0,
            pitch: 0.0,
            fov_degrees: 70.0,
        };
        Frustum::new(&camera, Viewpoint::Free(pose), 1.0)
    }

    // Looking down -Z from (0, 0, 20): dead ahead, well inside every plane.
    #[test]
    fn a_sphere_dead_ahead_is_visible() {
        assert!(frustum().visible(Vec3::new(0.0, 0.0, 0.0), 1.0));
    }

    #[test]
    fn a_sphere_behind_the_eye_is_culled() {
        assert!(!frustum().visible(Vec3::new(0.0, 0.0, 25.0), 1.0));
    }

    #[test]
    fn a_sphere_far_to_the_side_is_culled_by_the_left_or_right_plane() {
        assert!(!frustum().visible(Vec3::new(500.0, 0.0, 0.0), 1.0));
        assert!(!frustum().visible(Vec3::new(-500.0, 0.0, 0.0), 1.0));
    }

    #[test]
    fn a_sphere_far_above_or_below_is_culled_by_the_top_or_bottom_plane() {
        assert!(!frustum().visible(Vec3::new(0.0, 500.0, 0.0), 1.0));
        assert!(!frustum().visible(Vec3::new(0.0, -500.0, 0.0), 1.0));
    }

    // A big enough radius drags the sphere back into every plane's reach even
    // though its center sits well outside the frustum on every side at once.
    #[test]
    fn a_huge_sphere_still_touching_the_frustum_is_visible() {
        assert!(frustum().visible(Vec3::new(400.0, 400.0, 0.0), 1000.0));
    }

    // A sphere exactly tangent to a plane must not be culled by float
    // rounding alone — the boundary belongs to "visible".
    #[test]
    fn a_sphere_exactly_touching_a_plane_stays_visible() {
        let frustum = frustum();
        let plane = frustum.planes[1];
        let radius = 5.0;
        // A point exactly on the plane, then pushed `radius` further outside
        // along its normal: `distance(center) == -radius`, the tangent case.
        let on_plane = -plane.normal * plane.d;
        let center = on_plane - plane.normal * radius;

        assert!((plane.distance(center) + radius).abs() < 1e-4);
        assert!(frustum.visible(center, radius));
    }
}
