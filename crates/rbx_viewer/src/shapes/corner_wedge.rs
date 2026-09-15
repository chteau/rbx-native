//! The `CornerWedgePart` corner pyramid: a full-height vertical edge at
//! (+X, -Z), tapering down to nothing at the diagonally opposite base corner
//! (-X, +Z). The apex sits directly above the (+X, -Z) base corner (per
//! FrostDracony/Roblox's `GetCornersOfWedgeAndCornerWedge` corner-finding
//! reference), which is what makes two of its four side faces flat rather than
//! sloped.

use glam::Vec3;

use super::MeshData;

const H: f32 = 0.5;

pub(crate) fn corner_wedge() -> MeshData {
    let front_left = Vec3::new(-H, -H, -H);
    let front_right = Vec3::new(H, -H, -H);
    let back_right = Vec3::new(H, -H, H);
    let back_left = Vec3::new(-H, -H, H);
    let apex = Vec3::new(H, H, -H);

    let mut mesh = MeshData::default();
    let inside = (front_left + front_right + back_right + back_left + apex) / 5.0;

    mesh.push_flat_quad(inside, front_left, front_right, back_right, back_left);
    // A pyramid over a quadrilateral base is always its base plus one triangle
    // per base edge to the apex, regardless of where the apex sits above it.
    mesh.push_flat_triangle(inside, front_left, front_right, apex);
    mesh.push_flat_triangle(inside, front_right, back_right, apex);
    mesh.push_flat_triangle(inside, back_right, back_left, apex);
    mesh.push_flat_triangle(inside, back_left, front_left, apex);

    mesh
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shapes::test_support::assert_well_formed;

    #[test]
    fn the_corner_wedge_is_well_formed_and_faces_outward() {
        assert_well_formed(&corner_wedge());
    }

    #[test]
    fn the_corner_wedge_is_a_third_of_the_unit_cube() {
        // Pyramid volume = base area * height / 3, independent of the apex's
        // horizontal offset (Cavalieri's principle): 1 * 1 / 3.
        assert!((corner_wedge().volume() - 1.0 / 3.0).abs() < 1e-5);
    }
}
