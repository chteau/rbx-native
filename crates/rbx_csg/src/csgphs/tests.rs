use glam::{DMat4, DVec3, Vec3};

use super::*;
use crate::{shapes, solidmesh};

/// Studio's blobs for unions it made in an empty place, each set to a
/// fidelity in the Properties way and saved: an 8×4×8 block with a 3-stud
/// vertical hole (Default: 4 hulls, Precise: 24), an L of an 8×2×2 and a
/// 2×6×2 block (Hull: 1, Precise: 2), and two touching cubes at Box.
const HOLE_DEFAULT: &[u8] = include_bytes!("../../../../assets/tests/csg/HoleDefault.csgphs");
const HOLE_PRECISE: &[u8] = include_bytes!("../../../../assets/tests/csg/HolePrecise.csgphs");
const L_HULL: &[u8] = include_bytes!("../../../../assets/tests/csg/LHull.csgphs");
const L_PRECISE: &[u8] = include_bytes!("../../../../assets/tests/csg/LPrecise.csgphs");
const BOX: &[u8] = include_bytes!("../../../../assets/tests/csg/Box.csgphs");
/// The hole block's `SolidMeshHolder`.
const HOLE: &[u8] = include_bytes!("../../../../assets/tests/csg/Hole.solidmesh");

/// Every triangle of `shape` has all its vertices behind or on its plane,
/// and every edge meets its reverse once: a closed, outward, convex hull.
fn assert_convex(shape: &Shape) {
    let v: Vec<DVec3> = shape
        .vertices
        .iter()
        .map(|p| DVec3::from(p.map(f64::from)))
        .collect();
    let mut edges = std::collections::HashMap::new();
    for &[a, b, c] in &shape.triangles {
        let [pa, pb, pc] = [a, b, c].map(|i| v[i as usize]);
        let normal = (pb - pa).cross(pc - pa);
        assert!(normal.length() > 1e-9, "degenerate triangle");
        for &p in &v {
            assert!(normal.normalize().dot(p - pa) < 1e-3, "a vertex is outside");
        }
        for e in [(a, b), (b, c), (c, a)] {
            *edges.entry(e).or_insert(0) += 1;
        }
    }
    for (&(a, b), &n) in &edges {
        assert_eq!((n, edges.get(&(b, a))), (1, Some(&1)), "open edge");
    }
    assert_eq!(shape.triangles.len(), 2 * shape.vertices.len() - 4);
}

fn raw(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    ruzstd::decoding::StreamingDecoder::new(&bytes[HEADER..])
        .unwrap()
        .read_to_end(&mut out)
        .unwrap();
    out
}

#[test]
fn studio_blobs_decode_to_closed_convex_hulls() {
    for (bytes, fidelity, hulls, vertices) in [
        (HOLE_DEFAULT, Fidelity::Default, 4, 40),
        (HOLE_PRECISE, Fidelity::PreciseConvexDecomposition, 24, 152),
        (L_HULL, Fidelity::Hull, 1, 10),
        (L_PRECISE, Fidelity::PreciseConvexDecomposition, 2, 19),
    ] {
        let decoded = decode(bytes).unwrap();
        assert_eq!(decoded.fidelity, fidelity);
        assert_eq!(decoded.hulls.len(), hulls);
        let total: usize = decoded.hulls.iter().map(|h| h.vertices.len()).sum();
        assert_eq!(total, vertices);
        decoded.hulls.iter().for_each(assert_convex);
    }
    assert_eq!(
        decode(BOX).unwrap(),
        Decoded {
            fidelity: Fidelity::Box,
            hulls: Vec::new()
        }
    );
}

#[test]
fn coding_studio_hulls_again_gives_studio_bytes() {
    for bytes in [HOLE_DEFAULT, HOLE_PRECISE, L_HULL, L_PRECISE] {
        let decoded = decode(bytes).unwrap();
        let ours = encode_shapes(&decoded.hulls, decoded.fidelity);
        assert_eq!(ours[..HEADER], bytes[..HEADER]);
        // Single segment: the frame says how big it is, as Studio needs.
        assert_eq!(ours[HEADER + 4] & 0x20, 0x20);
        assert_eq!(raw(&ours), raw(bytes));
    }
}

#[test]
fn box_is_the_block_marker() {
    let solid = Solid::from_mesh(&shapes::block(), glam::Mat4::IDENTITY);
    assert_eq!(encode(&solid, Fidelity::Box), BOX);
    assert_eq!(fidelity(BOX), Some(Fidelity::Box));
}

#[test]
fn every_fidelity_reads_back_as_itself() {
    let hole = solidmesh::decode(HOLE).unwrap().solid(DMat4::IDENTITY);
    for fidelity in [
        Fidelity::Default,
        Fidelity::Hull,
        Fidelity::Box,
        Fidelity::PreciseConvexDecomposition,
    ] {
        let blob = encode(&hole, fidelity);
        assert_eq!(super::fidelity(&blob), Some(fidelity));
        let decoded = decode(&blob).unwrap();
        assert_eq!(decoded.fidelity, fidelity);
        decoded.hulls.iter().for_each(assert_convex);
        assert_eq!(Fidelity::from_value(fidelity.value()), fidelity);
    }
}

/// The hull of the hole block is the whole box; its pieces leave the hole
/// open, so a point in the hole is inside no piece.
#[test]
fn the_hole_stays_open_in_the_decomposition() {
    let hole = solidmesh::decode(HOLE).unwrap().solid(DMat4::IDENTITY);
    let inside = |shape: &Shape, p: Vec3| {
        shape.triangles.iter().all(|t| {
            let [a, b, c] = t.map(|i| Vec3::from(shape.vertices[i as usize]));
            (b - a).cross(c - a).dot(p - a) <= 1e-4
        })
    };
    let hull = decode(&encode(&hole, Fidelity::Hull)).unwrap();
    assert!(inside(&hull.hulls[0], Vec3::ZERO));
    for fidelity in [Fidelity::Default, Fidelity::PreciseConvexDecomposition] {
        let pieces = decode(&encode(&hole, fidelity)).unwrap().hulls;
        assert!(pieces.len() >= 4, "{fidelity:?}: {} pieces", pieces.len());
        assert!(!pieces.iter().any(|s| inside(s, Vec3::ZERO)));
        assert!(!pieces.iter().any(|s| inside(s, Vec3::new(0.7, 1.5, 0.7))));
        // Default's quarters bridge the curve with a chord; Precise follows it.
        let near_wall = pieces.iter().any(|s| inside(s, Vec3::new(0.95, 1.5, 0.95)));
        assert_eq!(near_wall, fidelity == Fidelity::Default);
        // The solid corners are still covered.
        assert!(pieces.iter().any(|s| inside(s, Vec3::new(3.5, 1.5, 3.5))));
    }
}
