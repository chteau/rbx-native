//! A ~24x16 UV sphere, radius 0.5 so a part's `size` becomes its diameter —
//! matching `Part.shape == Ball` and `SpecialMesh.MeshType == Sphere` alike (the
//! two differ only in whether the caller clamps `size` to a uniform diameter
//! before it is used as the scale; see `scene::shape`).

use std::f32::consts::{PI, TAU};

use glam::Vec3;

use super::MeshData;

const SEGMENTS: usize = 24;
const STACKS: usize = 16;
const RADIUS: f32 = 0.5;

pub(crate) fn sphere() -> MeshData {
    let mut mesh = MeshData::default();
    let inside = Vec3::ZERO;

    // A point on the unit sphere is also its own outward normal (direction from
    // the origin), scaled to this mesh's 0.5 radius.
    let point = |stack: usize, segment: usize| -> Vec3 {
        let phi = PI * stack as f32 / STACKS as f32;
        let theta = TAU * segment as f32 / SEGMENTS as f32;
        let (sin_phi, cos_phi) = phi.sin_cos();
        let (sin_theta, cos_theta) = theta.sin_cos();
        Vec3::new(sin_phi * cos_theta, cos_phi, sin_phi * sin_theta) * RADIUS
    };

    for stack in 0..STACKS {
        for segment in 0..SEGMENTS {
            let (a, b, c, d) = (
                point(stack, segment),
                point(stack + 1, segment),
                point(stack + 1, segment + 1),
                point(stack, segment + 1),
            );
            // Degenerate at the poles (a == b, or c == d): a zero-area
            // triangle contributes nothing and draws nothing.
            mesh.push_smooth_triangle(
                inside,
                [a, b, c],
                [a.normalize(), b.normalize(), c.normalize()],
            );
            mesh.push_smooth_triangle(
                inside,
                [a, c, d],
                [a.normalize(), c.normalize(), d.normalize()],
            );
        }
    }

    mesh
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shapes::test_support::assert_well_formed;

    #[test]
    fn the_sphere_is_well_formed_and_faces_outward() {
        assert_well_formed(&sphere());
    }

    #[test]
    fn the_sphere_volume_approximates_a_half_stud_radius_ball() {
        // pi * r^3 * 4/3 with r = 0.5; faceting undershoots the true volume.
        let expected = std::f32::consts::PI * RADIUS.powi(3) * 4.0 / 3.0;
        let volume = sphere().volume();

        assert!(
            (volume - expected).abs() / expected < 0.03,
            "{volume} vs {expected}"
        );
    }

    #[test]
    fn every_vertex_sits_exactly_on_the_sphere() {
        for position in &sphere().positions {
            assert!((Vec3::from(*position).length() - RADIUS).abs() < 1e-5);
        }
    }
}
