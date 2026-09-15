//! The unit cube as a [`MeshData`], for code that needs a `Part` block as
//! plain triangles rather than the renderer's own GPU cube
//! (`renderer::mesh::cube`) — the union CSG carves it.

use glam::Vec3;

use super::MeshData;

const HALF: f32 = 0.5;

pub(crate) fn block() -> MeshData {
    let mut mesh = MeshData::default();
    let inside = Vec3::ZERO;
    let corner = |x: f32, y: f32, z: f32| Vec3::new(x, y, z) * HALF;
    // Each face's four corners in ring order; `push_flat_quad` fixes winding.
    for [a, b, c, d] in [
        [
            (-1., -1., -1.),
            (1., -1., -1.),
            (1., 1., -1.),
            (-1., 1., -1.),
        ],
        [(-1., -1., 1.), (1., -1., 1.), (1., 1., 1.), (-1., 1., 1.)],
        [
            (-1., -1., -1.),
            (-1., -1., 1.),
            (-1., 1., 1.),
            (-1., 1., -1.),
        ],
        [(1., -1., -1.), (1., -1., 1.), (1., 1., 1.), (1., 1., -1.)],
        [
            (-1., -1., -1.),
            (1., -1., -1.),
            (1., -1., 1.),
            (-1., -1., 1.),
        ],
        [(-1., 1., -1.), (1., 1., -1.), (1., 1., 1.), (-1., 1., 1.)],
    ] {
        mesh.push_flat_quad(
            inside,
            corner(a.0, a.1, a.2),
            corner(b.0, b.1, b.2),
            corner(c.0, c.1, c.2),
            corner(d.0, d.1, d.2),
        );
    }
    mesh
}

#[cfg(test)]
mod tests {
    use super::super::test_support::assert_well_formed;
    use super::block;

    #[test]
    fn the_block_is_a_well_formed_unit_cube() {
        let mesh = block();
        assert_well_formed(&mesh);
        assert_eq!(mesh.indices.len(), 12 * 3);
        assert!(
            (mesh.volume() - 1.0).abs() < 1e-6,
            "volume {}",
            mesh.volume()
        );
    }
}
