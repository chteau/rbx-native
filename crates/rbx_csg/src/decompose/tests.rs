use glam::{DMat4, DVec3, Mat4, Vec3};

use super::decompose;
use crate::hull::Hull;
use crate::{shapes, solidmesh, Solid};

fn block(at: Vec3, size: Vec3) -> Solid {
    Solid::from_mesh(
        &shapes::block(),
        Mat4::from_translation(at) * Mat4::from_scale(size),
    )
}

#[test]
fn a_box_is_one_piece() {
    let pieces = decompose(&block(Vec3::ZERO, Vec3::new(4.0, 2.0, 2.0)), 0.02, 16);
    assert_eq!(pieces.len(), 1);
    assert!((pieces[0].volume() - 16.0).abs() < 1e-9);
}

#[test]
fn an_l_splits_into_its_two_blocks() {
    let foot = block(Vec3::new(0.0, 1.0, 0.0), Vec3::new(8.0, 2.0, 2.0));
    let leg = block(Vec3::new(-3.0, 5.0, 0.0), Vec3::new(2.0, 6.0, 2.0));
    let l = foot.union(leg).unwrap().finish(None).unwrap();
    let pieces = decompose(&l, 0.02, 16);
    assert_eq!(pieces.len(), 2);
    let total: f64 = pieces.iter().map(|p| p.volume()).sum();
    assert!((total - 56.0).abs() < 1e-6, "{total}");
}

#[test]
fn the_budget_caps_the_pieces() {
    let base = block(Vec3::ZERO, Vec3::new(8.0, 2.0, 8.0));
    let comb = (0..4).fold(base, |solid, i| {
        let tooth = block(
            Vec3::new(-3.0 + 2.0 * i as f32, 3.0, 0.0),
            Vec3::new(1.0, 4.0, 8.0),
        );
        solid.union(tooth).unwrap()
    });
    let comb = comb.finish(None).unwrap();
    assert_eq!(decompose(&comb, 0.0, 3).len(), 3);
    assert!(decompose(&comb, 0.001, 32).len() >= 5);
}

/// A finished boolean's welded polygons sit a little off their planes; the
/// cuts must still end (a block and a ball: one convex piece when they
/// intersect, several when they unite).
#[test]
fn a_welded_curved_result_decomposes() {
    for intersect in [true, false] {
        let a = block(Vec3::ZERO, Vec3::splat(4.0));
        let ball = Solid::from_mesh(
            &shapes::sphere(),
            Mat4::from_translation(Vec3::X) * Mat4::from_scale(Vec3::splat(5.0)),
        );
        let solid = if intersect {
            a.intersect(ball)
        } else {
            a.union(ball)
        };
        let solid = solid.unwrap().finish(None).unwrap();
        let pieces = decompose(&solid, 0.02, 16);
        assert_eq!(pieces.len() == 1, intersect, "{} pieces", pieces.len());
    }
}

/// Our own drilled block, read back from the `SolidMeshHolder` we save
/// (f32 corners, one triangle per face), at both decomposition settings:
/// nothing covers the hole, every piece is a sound hull.
#[test]
fn a_drilled_block_keeps_its_hole_open() {
    let block = block(Vec3::ZERO, Vec3::new(8.0, 4.0, 8.0));
    let drill = Solid::from_mesh(
        &shapes::cylinder_x(),
        Mat4::from_rotation_z(std::f32::consts::FRAC_PI_2)
            * Mat4::from_scale(Vec3::new(6.0, 3.0, 3.0)),
    );
    let drilled = block.subtract(drill).unwrap().finish(None).unwrap();
    let saved = solidmesh::decode(&solidmesh::encode(&drilled)).unwrap();
    let drilled = saved.solid(DMat4::IDENTITY);
    let inside = |hull: &Hull, p: DVec3| {
        hull.triangles.iter().all(|t| {
            let [a, b, c] = t.map(|i| hull.points[i as usize]);
            (b - a).cross(c - a).dot(p - a) <= 1e-6
        })
    };
    for (tolerance, budget) in [(0.02, 16), (0.002, 32)] {
        let pieces = decompose(&drilled, tolerance, budget);
        assert!(pieces.len() >= 4);
        for p in [
            DVec3::ZERO,
            DVec3::new(0.0, 1.9, 0.0),
            DVec3::new(0.5, -1.9, -0.5),
        ] {
            assert!(!pieces.iter().any(|h| inside(h, p)), "{p} covered");
        }
        let total: f64 = pieces.iter().map(Hull::volume).sum();
        assert!(total >= drilled.volume() - 1e-3 && total < 256.0, "{total}");
    }
}
