//! Unit cylinders (radius 0.5, length 1) along X or Y: `Part.shape == Cylinder`
//! lies along the part's X axis, while the legacy `CylinderMesh` (and
//! `SpecialMesh.MeshType == Cylinder`) stand along Y.

use std::f32::consts::TAU;

use glam::Vec3;

use super::MeshData;

const SEGMENTS: usize = 24;
const RADIUS: f32 = 0.5;
const HALF_LENGTH: f32 = 0.5;

pub(crate) fn cylinder_y() -> MeshData {
    cylinder(|h, cos, sin| Vec3::new(cos * RADIUS, h * HALF_LENGTH, sin * RADIUS))
}

pub(crate) fn cylinder_x() -> MeshData {
    cylinder(|h, cos, sin| Vec3::new(h * HALF_LENGTH, cos * RADIUS, sin * RADIUS))
}

/// Builds a cylinder from an embedding of (signed half-length, cos, sin) into
/// object space.
///
/// The X and Y variants share this one implementation by passing different
/// embeddings; swapping which axis carries height and which two carry radius is
/// an odd permutation and flips handedness, but that is harmless here — every
/// triangle is oriented against a point known to be inside the shape, not by
/// the order its vertices happen to be listed in.
fn cylinder(embed: impl Fn(f32, f32, f32) -> Vec3) -> MeshData {
    let mut mesh = MeshData::default();
    let inside = Vec3::ZERO;

    // `h` is a signed unit (-1 or 1), not a length: `embed` itself scales it by
    // HALF_LENGTH, so multiplying it in twice would halve the cylinder's length.
    //
    // The embedding is linear in each argument, so plugging in zero height
    // yields a vector purely in the radial plane: its direction is exactly the
    // outward side normal at that angle, whichever axis the caller chose.
    let ring_point = |h: f32, segment: usize| -> (Vec3, Vec3) {
        let theta = TAU * segment as f32 / SEGMENTS as f32;
        let (sin, cos) = theta.sin_cos();
        (embed(h, cos, sin), embed(0.0, cos, sin).normalize())
    };

    for segment in 0..SEGMENTS {
        let (a, na) = ring_point(-1.0, segment);
        let (b, nb) = ring_point(1.0, segment);
        let (c, nc) = ring_point(1.0, segment + 1);
        let (d, nd) = ring_point(-1.0, segment + 1);
        mesh.push_smooth_triangle(inside, [a, b, c], [na, nb, nc]);
        mesh.push_smooth_triangle(inside, [a, c, d], [na, nc, nd]);
    }

    for h in [-1.0, 1.0] {
        let center = embed(h, 0.0, 0.0);
        for segment in 0..SEGMENTS {
            let (a, _) = ring_point(h, segment);
            let (b, _) = ring_point(h, segment + 1);
            mesh.push_flat_triangle(inside, center, a, b);
        }
    }

    mesh
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shapes::test_support::assert_well_formed;

    #[test]
    fn both_axes_are_well_formed_and_face_outward() {
        assert_well_formed(&cylinder_x());
        assert_well_formed(&cylinder_y());
    }

    #[test]
    fn both_axes_hold_the_same_volume_as_a_half_stud_radius_cylinder() {
        // pi * r^2 * length, faceted so slightly undershoots the true volume.
        let expected = std::f32::consts::PI * RADIUS.powi(2) * (HALF_LENGTH * 2.0);

        for volume in [cylinder_x().volume(), cylinder_y().volume()] {
            assert!(
                (volume - expected).abs() / expected < 0.02,
                "{volume} vs {expected}"
            );
        }
    }

    #[test]
    fn the_y_cylinder_stands_along_y_and_the_x_cylinder_lies_along_x() {
        let spans = |mesh: &MeshData, axis: usize| {
            mesh.positions
                .iter()
                .map(|p| p[axis])
                .fold(0.0f32, f32::max)
        };

        let y = cylinder_y();
        assert!((spans(&y, 1) - HALF_LENGTH).abs() < 1e-5);
        assert!((spans(&y, 0) - RADIUS).abs() < 1e-5);

        let x = cylinder_x();
        assert!((spans(&x, 0) - HALF_LENGTH).abs() < 1e-5);
        assert!((spans(&x, 1) - RADIUS).abs() < 1e-5);
    }
}
