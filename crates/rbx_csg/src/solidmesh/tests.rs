use glam::{DMat4, DVec3, Mat4, Vec3};

use super::*;
use crate::shapes;

/// Studio's blobs for unions it made: two touching 2-stud cubes; a red
/// 4×2×2 block and a green 2×4×2 one turned half a radian; an 8-stud red
/// ball with a blue 10×4×4 cylinder subtracted along X (wide deltas).
const TWO_CUBES: &[u8] = include_bytes!("../../../../assets/tests/csg/TwoCubes.solidmesh");
const RED_GREEN: &[u8] = include_bytes!("../../../../assets/tests/csg/RedGreenBlocks.solidmesh");
const BALL_MINUS_CYLINDER: &[u8] =
    include_bytes!("../../../../assets/tests/csg/BallMinusCylinder.solidmesh");

fn close(a: DVec3, b: [f64; 3], tolerance: f64) -> bool {
    (a - DVec3::from(b)).abs().max_element() < tolerance
}

#[test]
fn studio_blobs_decode() {
    let cubes = decode(TWO_CUBES).unwrap();
    assert_eq!((cubes.positions.len(), cubes.triangles.len()), (8, 12));
    assert!(cubes.colors.iter().all(|&c| c == [163, 162, 165]));
    let solid = cubes.solid(DMat4::IDENTITY);
    assert!((solid.volume() - 16.0).abs() < 1e-4);

    let ball = decode(BALL_MINUS_CYLINDER).unwrap();
    assert_eq!((ball.positions.len(), ball.triangles.len()), (280, 560));
    let mut colors = ball.colors.clone();
    colors.sort();
    colors.dedup();
    assert_eq!(colors, [[0, 0, 255], [255, 0, 0]]);
}

/// The values Studio saved next to each blob come back from its mesh.
#[test]
fn mass_properties_match_what_studio_saves() {
    let ball = decode(BALL_MINUS_CYLINDER).unwrap().solid(DMat4::IDENTITY);
    let mass = ball.mass_properties().unwrap();
    assert!((mass.volume - 168.409485).abs() < 1e-3, "{mass:?}");
    assert!(close(
        mass.inertia_diagonal,
        [1451.61792, 1125.4126, 1125.4126],
        0.05
    ));

    let blocks = decode(RED_GREEN).unwrap().solid(DMat4::IDENTITY);
    let mass = blocks.mass_properties().unwrap();
    assert!((mass.volume - 28.0356178).abs() < 1e-3, "{mass:?}");
    assert!(close(
        mass.centroid,
        [0.260120928, -0.429297447, -0.180481866],
        1e-4
    ));
    assert!(close(
        mass.inertia_diagonal,
        [50.4832153, 50.8064346, 64.7511444],
        1e-3
    ));
    assert!(close(
        mass.inertia_off_diagonal,
        [-8.98200035, -8.09648514, -8.03164482],
        1e-3
    ));
}

#[test]
fn a_two_coloured_union_round_trips() {
    let red = Solid::from_mesh(&shapes::block(), Mat4::from_scale(Vec3::new(4.0, 2.0, 2.0)))
        .painted([255, 0, 0]);
    let green = Solid::from_mesh(
        &shapes::sphere(),
        Mat4::from_translation(Vec3::X * 2.0) * Mat4::from_scale(Vec3::splat(3.0)),
    )
    .painted([0, 255, 0]);
    let solid = red.union(green).unwrap().finish(None).unwrap();
    let blob = encode(&solid);
    assert_eq!(&blob[..2], &[3, 1]);
    assert_eq!(
        u32::from_le_bytes(blob[2..6].try_into().unwrap()) as usize,
        blob.len() - HEADER
    );
    let back = decode(&blob).unwrap();
    let triangles: usize = solid.polygons.iter().map(|p| p.vertices.len() - 2).sum();
    assert_eq!(back.triangles.len(), triangles);
    let back = back.solid(DMat4::IDENTITY);
    assert!((back.volume() - solid.volume()).abs() < 1e-3);
    for color in [[255, 0, 0], [0, 255, 0]] {
        assert!(back.polygons.iter().any(|p| p.color == color));
    }
}

#[test]
fn studio_blobs_survive_our_encoder() {
    for blob in [TWO_CUBES, RED_GREEN, BALL_MINUS_CYLINDER] {
        let solid = decode(blob).unwrap().solid(DMat4::IDENTITY);
        let again = decode(&encode(&solid)).unwrap().solid(DMat4::IDENTITY);
        assert_eq!(again.polygons.len(), solid.polygons.len());
        assert!((again.volume() - solid.volume()).abs() < 1e-4);
    }
}

#[test]
fn wide_deltas_round_trip() {
    let values = [0, 63, 0, 64, 1000, 3, 300_000, 0, 5];
    let mut out = Vec::new();
    write_deltas(&mut out, values);
    let (back, read) = read_deltas(&out, values.len(), 300_001).unwrap();
    assert_eq!(back, values);
    assert_eq!(read, out.len());
    assert!(read_deltas(&out, values.len(), 1000).is_none());
}

#[test]
fn junk_is_refused() {
    assert!(decode(b"").is_none());
    assert!(decode(&TWO_CUBES[..100]).is_none());
    let mut compressed = TWO_CUBES.to_vec();
    compressed[6] = 1;
    assert!(decode(&compressed).is_none());
}
