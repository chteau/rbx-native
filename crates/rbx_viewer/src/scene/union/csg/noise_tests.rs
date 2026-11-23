//! What real places' unions failed on: faces meant to meet sitting a few
//! hundred-thousandths of a stud apart, as Roblox's own coordinates leave
//! them, so the boolean kept a sliver between them (see `bsp::EPSILON` and
//! `repair::clean`); and parts touching only along an edge, read as a leak.

use std::collections::BTreeMap;

use glam::{DVec3, Mat4, Vec3};

use super::super::tree::{Leaf, Node};
use super::bsp::{Plane, Polygon};
use super::repair::only_slivers;
use super::{clean, evaluate, evaluate_node, is_watertight, weld_t_junctions};
use crate::scene::shape::Geometry;
use crate::scene::ShapeKind;

fn cube(size: Vec3, at: Vec3, negate: bool) -> Node {
    Node::Leaf(Leaf {
        negate,
        geometry: Geometry {
            kind: ShapeKind::Box,
            size,
            offset: Vec3::ZERO,
        },
        cframe: Mat4::from_translation(at),
        properties: BTreeMap::new(),
    })
}

/// A 2-stud cube with its top half carved away by a negation whose side
/// stops 2e-5 short of the cube's own: the "exact" result keeps a wall that
/// thin standing at x = -1.
fn noisy_carve() -> Node {
    const NOISE: f32 = 2e-5;
    let width = 2.0 - NOISE;
    Node::Operation {
        negate: false,
        children: vec![
            cube(Vec3::splat(2.0), Vec3::ZERO, false),
            Node::Operation {
                negate: true,
                children: vec![cube(
                    Vec3::new(width, 1.0, 2.0),
                    Vec3::new(1.0 - width / 2.0, 0.5, 0.0),
                    false,
                )],
            },
        ],
    }
}

#[test]
fn the_boolean_leaves_no_sliver_to_weld() {
    let solid = evaluate_node(&noisy_carve(), &mut Vec::new()).expect("the boolean itself runs");

    assert!(is_watertight(&weld_t_junctions(solid.polygons)));
}

#[test]
fn cleaning_snaps_noisy_corners_and_drops_what_collapses() {
    let plane = Plane::from_points(DVec3::ZERO, DVec3::X, DVec3::Y).expect("a plane");
    let quad = |vertices: Vec<DVec3>| Polygon {
        vertices,
        plane,
        color: rbx_csg::DEFAULT_COLOR,
    };
    // A real face, and a sliver 3e-5 wide along its left edge.
    let face = quad(vec![
        DVec3::new(0.0, 0.0, 0.0),
        DVec3::new(1.0, 0.0, 0.0),
        DVec3::new(1.0, 1.0, 0.0),
        DVec3::new(0.0, 1.0, 0.0),
    ]);
    let sliver = quad(vec![
        DVec3::new(-3e-5, 0.0, 0.0),
        DVec3::new(0.0, 0.0, 0.0),
        DVec3::new(0.0, 1.0, 0.0),
        DVec3::new(-3e-5, 1.0, 0.0),
    ]);
    // The same corner as the face's, a float noise off.
    let neighbour = quad(vec![
        DVec3::new(1.0, 0.0, 0.0),
        DVec3::new(2.0, 0.0, 0.0),
        DVec3::new(2.0, 1.0, 0.0),
        DVec3::new(1.0 + 2e-5, 1.0 - 2e-5, 0.0),
    ]);

    let cleaned = clean(vec![face, sliver, neighbour]);

    assert_eq!(cleaned.len(), 2, "the sliver collapses to a line and goes");
    assert_eq!(cleaned[1].vertices[3], DVec3::new(1.0, 1.0, 0.0));
}

#[test]
fn a_carve_a_float_noise_short_of_a_face_still_closes() {
    let solid = evaluate(&noisy_carve(), None).expect("the sliver no longer fails the boolean");

    // The bottom half, 2 x 1 x 2, with no wall left standing on top.
    assert!((solid.volume() - 4.0).abs() < 1e-3, "{}", solid.volume());
    let mesh = solid.to_mesh();
    assert!(
        mesh.bounds.max[1] < 1e-3,
        "nothing above the cut: {:?}",
        mesh.bounds
    );
}

#[test]
fn parts_touching_only_along_an_edge_are_still_closed() {
    // A door frame's jamb and lintel set corner to corner: the edge they
    // share is non-manifold, four faces meet on it, not a leak.
    let tree = Node::Operation {
        negate: false,
        children: vec![
            cube(Vec3::splat(1.0), Vec3::ZERO, false),
            cube(Vec3::splat(1.0), Vec3::new(1.0, 1.0, 0.0), false),
        ],
    };

    let solid = evaluate(&tree, None).expect("a non-manifold edge is not a leak");

    assert!((solid.volume() - 2.0).abs() < 1e-6, "{}", solid.volume());
}

#[test]
fn a_gap_a_few_ten_thousandths_wide_is_a_sliver_not_a_hole() {
    // In weld cells (1e-4 studs): a cylinder's facet corner landing 6e-4 off
    // a box's edge leaves this thin loop open, as it did on a real lamp.
    let (p1, p2) = ((24999, -5000, -4994), (25000, -5000, -5000));
    let (p3, p4) = ((25000, 5000, -5000), (24999, 5000, -4994));
    assert!(only_slivers(&[(p1, p2), (p2, p3), (p3, p4), (p4, p1)]));

    // A missing triangle a stud across is a hole, however few its edges.
    let (a, b, c) = ((0, 0, 0), (10000, 0, 0), (0, 10000, 0));
    assert!(!only_slivers(&[(a, b), (b, c), (c, a)]));
}

#[test]
fn touching_parts_a_float_noise_apart_merge() {
    // Two cubes side by side, the second nudged 3e-5 into the first: a
    // builder's two bricks, as they come out of a real place file.
    let tree = Node::Operation {
        negate: false,
        children: vec![
            cube(Vec3::splat(1.0), Vec3::ZERO, false),
            cube(Vec3::splat(1.0), Vec3::new(1.0 - 3e-5, 0.0, 0.0), false),
        ],
    };

    let solid = evaluate(&tree, None).expect("two touching cubes union");

    assert!((solid.volume() - 2.0).abs() < 1e-3, "{}", solid.volume());
}
