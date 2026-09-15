//! The `WedgePart` prism: a vertical face at +Z, sloping down to the
//! front-bottom edge at (-Z, -Y). Vertex layout cross-checked against
//! FrostDracony/Roblox's `GetCornersOfWedgeAndCornerWedge` corner-finding
//! reference, which derives its six corners straight from the engine's own
//! bounding logic.

use glam::Vec3;

use super::MeshData;

const H: f32 = 0.5;

pub(crate) fn wedge() -> MeshData {
    let back_top_left = Vec3::new(-H, H, H);
    let back_bottom_left = Vec3::new(-H, -H, H);
    let back_top_right = Vec3::new(H, H, H);
    let back_bottom_right = Vec3::new(H, -H, H);
    let front_bottom_left = Vec3::new(-H, -H, -H);
    let front_bottom_right = Vec3::new(H, -H, -H);

    let mut mesh = MeshData::default();
    // The average of the six corners of a convex prism always lies in its
    // interior, so it is a safe "outward" reference for every face below.
    let inside = (back_top_left
        + back_bottom_left
        + back_top_right
        + back_bottom_right
        + front_bottom_left
        + front_bottom_right)
        / 6.0;

    mesh.push_flat_quad(
        inside,
        back_bottom_left,
        back_bottom_right,
        back_top_right,
        back_top_left,
    );
    mesh.push_flat_quad(
        inside,
        front_bottom_left,
        front_bottom_right,
        back_bottom_right,
        back_bottom_left,
    );
    // The ramp: rises from the front-bottom edge to the back-top edge.
    mesh.push_flat_quad(
        inside,
        front_bottom_left,
        front_bottom_right,
        back_top_right,
        back_top_left,
    );
    mesh.push_flat_triangle(inside, front_bottom_left, back_bottom_left, back_top_left);
    mesh.push_flat_triangle(
        inside,
        front_bottom_right,
        back_top_right,
        back_bottom_right,
    );

    mesh
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shapes::test_support::assert_well_formed;

    #[test]
    fn the_wedge_is_well_formed_and_faces_outward() {
        assert_well_formed(&wedge());
    }

    #[test]
    fn the_wedge_is_exactly_half_the_unit_cube() {
        // A right-triangle cross-section of area 0.5, extruded one stud along X.
        assert!((wedge().volume() - 0.5).abs() < 1e-5);
    }

    // Which way round the prism sits decides where a `Face = Front` decal lands
    // and which side a builder's ramp climbs, so it is pinned here rather than
    // left to the corner names above.
    #[test]
    fn the_slope_climbs_toward_the_back_and_the_vertical_face_is_at_plus_z() {
        let mesh = wedge();
        let mut slope_vertices = 0;

        for (position, normal) in mesh.positions.iter().zip(&mesh.normals) {
            let (position, normal) = (Vec3::from(*position), Vec3::from(*normal));
            if normal.y > 0.0 {
                // The slope is the only upward-facing surface a wedge has: it
                // leans toward -Z, the direction Roblox calls Front.
                assert!(normal.z < 0.0, "the slope faces {normal} instead of -Z");
                slope_vertices += 1;
            }
            if normal.z > 0.5 {
                assert_eq!(position.z, H, "the vertical face is not at +Z");
            }
        }

        assert!(slope_vertices > 0, "the wedge has no slope at all");
    }
}
