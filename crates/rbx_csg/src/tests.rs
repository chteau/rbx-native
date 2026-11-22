use glam::{DVec3, Mat4, Vec3};

use super::{shapes, Failure, Solid};

fn block(at: Vec3, size: Vec3) -> Solid {
    Solid::from_mesh(
        &shapes::block(),
        Mat4::from_translation(at) * Mat4::from_scale(size),
    )
}

#[test]
fn intersecting_two_overlapping_blocks_keeps_their_common_box() {
    let a = block(Vec3::ZERO, Vec3::splat(4.0));
    let b = block(Vec3::new(2.0, 0.0, 0.0), Vec3::splat(4.0));
    let common = a.intersect(b).unwrap().finish(None).unwrap();
    assert!((common.volume() - 32.0).abs() < 1e-6);
    let centre = common.centroid().unwrap();
    assert!((centre - DVec3::new(1.0, 0.0, 0.0)).length() < 1e-6);
    let (lo, hi) = common.bounds().unwrap();
    assert!((lo - DVec3::new(0.0, -2.0, -2.0)).length() < 1e-6);
    assert!((hi - DVec3::new(2.0, 2.0, 2.0)).length() < 1e-6);
}

#[test]
fn disjoint_blocks_intersect_to_nothing() {
    let a = block(Vec3::ZERO, Vec3::ONE);
    let b = block(Vec3::splat(5.0), Vec3::ONE);
    assert_eq!(
        a.intersect(b).unwrap().finish(None).unwrap_err(),
        Failure::Empty
    );
}

#[test]
fn the_centroid_of_an_l_shape_leans_into_its_heavier_arm() {
    let long = block(Vec3::new(1.0, 0.0, 0.0), Vec3::new(4.0, 1.0, 1.0));
    let foot = block(Vec3::new(-0.5, 1.0, 0.0), Vec3::new(1.0, 1.0, 1.0));
    let l = long.union(foot).unwrap().finish(None).unwrap();
    assert!((l.volume() - 5.0).abs() < 1e-6);
    // (4 * (1, 0) + 1 * (-0.5, 1)) / 5
    let centre = l.centroid().unwrap();
    assert!((centre - DVec3::new(0.7, 0.2, 0.0)).length() < 1e-6);
}

#[test]
fn moving_a_solid_moves_its_centroid_and_keeps_its_volume() {
    let a = block(Vec3::ZERO, Vec3::new(1.0, 2.0, 3.0));
    let mirror = glam::DMat4::from_translation(DVec3::X * 3.0)
        * glam::DMat4::from_scale(DVec3::new(-1.0, 1.0, 1.0));
    let moved = a.transformed(mirror);
    assert!((moved.volume() - 6.0).abs() < 1e-9);
    assert!((moved.centroid().unwrap() - DVec3::X * 3.0).length() < 1e-9);
}
