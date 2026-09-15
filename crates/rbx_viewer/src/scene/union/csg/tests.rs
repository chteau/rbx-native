use std::collections::BTreeMap;

use glam::{DVec3, Mat4, Quat, Vec3};
use rbx_dom::Variant;

use super::super::tree::{Leaf, Node};
use super::{evaluate, largest_additive_color, Failure, Solid};
use crate::scene::shape::Geometry;
use crate::scene::ShapeKind;
use crate::shapes;

const TOLERANCE: f64 = 1e-4;

fn block(size: Vec3, at: Vec3) -> Solid {
    Solid::from_mesh(
        &shapes::block(),
        Mat4::from_translation(at) * Mat4::from_scale(size),
    )
}

fn leaf(kind: ShapeKind, size: Vec3, at: Vec3, negate: bool) -> Node {
    Node::Leaf(Leaf {
        negate,
        geometry: Geometry {
            kind,
            size,
            offset: Vec3::ZERO,
        },
        cframe: Mat4::from_translation(at),
        properties: BTreeMap::new(),
    })
}

fn union_of(children: Vec<Node>) -> Node {
    Node::Operation {
        negate: false,
        children,
    }
}

/// `Solid::subtract`/`union` no longer weld or watertight-check every
/// intermediate step (too costly for a tree of hundreds of leaves) —
/// `evaluate` does that once, at the end. These regression tests exercise
/// one boolean directly, so they call the same pair of passes themselves.
fn assert_watertight(solid: &Solid) {
    let welded = super::weld_t_junctions(solid.polygons.clone());
    assert!(
        super::is_watertight(&welded),
        "result has open or duplicated edges"
    );
}

fn assert_volume(solid: &Solid, expected: f64) {
    let volume = solid.volume();
    assert!(
        (volume - expected).abs() < TOLERANCE,
        "volume {volume}, expected {expected}"
    );
}

#[test]
fn subtracting_a_rotated_box_stays_watertight() {
    // A box rotated only around Y, fully inside the outer box on every axis:
    // the outer's own faces are never geometrically touched by it, yet
    // without `merge_quad_pairs` the two triangles making up each outer face
    // independently mis-cut against the rotated planes and left real gaps —
    // caught here as `Failure::Leaky` before `merge_quad_pairs` existed.
    let outer = block(Vec3::splat(4.0), Vec3::ZERO);
    let inner = Solid::from_mesh(
        &shapes::block(),
        Mat4::from_quat(glam::Quat::from_rotation_y(0.3)) * Mat4::from_scale(Vec3::splat(2.0)),
    );
    let result = outer.subtract(inner).expect("subtraction must succeed");
    assert_watertight(&result);
}

#[test]
fn subtracting_a_tilted_box_stays_watertight() {
    let outer = block(Vec3::splat(4.0), Vec3::ZERO);
    let inner = Solid::from_mesh(
        &shapes::block(),
        Mat4::from_translation(Vec3::new(1.0, 1.0, -1.0))
            * Mat4::from_quat(glam::Quat::from_euler(glam::EulerRot::XYZ, 0.4, 0.7, 0.2))
            * Mat4::from_scale(Vec3::new(5.0, 2.0, 5.0)),
    );
    let result = outer.subtract(inner).expect("subtraction must succeed");
    assert_watertight(&result);
}

#[test]
fn a_placed_block_has_its_own_volume() {
    assert_volume(&block(Vec3::new(2.0, 3.0, 4.0), Vec3::X), 24.0);
}

#[test]
fn a_mirroring_matrix_keeps_the_solid_outward() {
    let mirrored = Solid::from_mesh(
        &shapes::block(),
        Mat4::from_scale(Vec3::new(-2.0, 1.0, 1.0)),
    );
    assert_volume(&mirrored, 2.0);
}

#[test]
fn subtracting_a_smaller_box_carves_a_watertight_pocket() {
    let outer = block(Vec3::splat(2.0), Vec3::ZERO);
    let inner = block(Vec3::splat(1.0), Vec3::new(0.5, 0.5, 0.5));
    let carved = outer.subtract(inner).expect("subtraction must succeed");
    // 8 minus the 1x1x1 corner that overlapped: the pocket is closed, so the
    // divergence sum still reads the exact remaining volume.
    assert_volume(&carved, 7.0);
    assert!(carved.to_mesh().indices.len() > 36, "the pocket adds faces");
}

/// Pins the exact geometry of a corner notch, the shape this module's real
/// regression (the legacy rock carving a double-rotated cavity instead of a
/// shallow facet — see `tree::walk`'s `echo` parameter) was ultimately about:
/// not just "some positive volume survived", but the right volume with every
/// face — original hull and freshly exposed notch alike — wound outward.
#[test]
fn a_corner_notch_pins_exact_geometry_and_outward_winding() {
    // [0,2]^3 minus [1,3]^3: a 2-unit cube with its (2,2,2) corner notched by
    // exactly the 1x1x1 overlap at [1,2]^3.
    let outer = block(Vec3::splat(2.0), Vec3::splat(1.0));
    let inner = block(Vec3::splat(2.0), Vec3::splat(2.0));
    let notched = outer.subtract(inner).expect("subtraction must succeed");
    assert_volume(&notched, 7.0);
    assert_watertight(&notched);

    // A point just inside the remaining solid, close to the notch corner:
    // every outward-wound boundary face (original hull or freshly exposed
    // notch wall alike) has its normal pointing away from it, so the sign is
    // positive everywhere — see the module's plane convention in `bsp.rs`.
    let reference = DVec3::splat(0.9);
    for polygon in &notched.polygons {
        let centroid: DVec3 =
            polygon.vertices.iter().copied().sum::<DVec3>() / polygon.vertices.len() as f64;
        assert!(
            polygon.plane.normal.dot(centroid - reference) > 0.0,
            "face at {centroid:?} (normal {:?}) does not point away from the solid's interior",
            polygon.plane.normal
        );
    }

    // Exactly 3 new planar faces are exposed by the notch: the cut planes
    // x=1, y=1, z=1 (offset 1 — every original hull plane sits at 0 or 2
    // instead), each possibly landing as more than one polygon fragment (the
    // BSP splits a face against every plane it is clipped through, and
    // `merge_quad_pairs` only re-fuses an exact triangulated-quad diagonal,
    // not arbitrary coplanar neighbors), so this counts distinct planes by
    // their own (normal, offset) identity rather than raw polygons or where
    // a fragment's centroid happens to land.
    let mut cut_planes: Vec<(i64, i64, i64, i64)> = notched
        .polygons
        .iter()
        .map(|polygon| {
            let n = polygon.plane.normal;
            let round = |v: f64| (v * 1e6).round() as i64;
            (round(n.x), round(n.y), round(n.z), round(polygon.plane.w))
        })
        .filter(|&(nx, ny, nz, w)| w == 1_000_000 && (nx.abs() + ny.abs() + nz.abs() == 1_000_000))
        .collect();
    cut_planes.sort();
    cut_planes.dedup();
    assert_eq!(
        cut_planes.len(),
        3,
        "the notch must expose exactly its 3 cut faces"
    );
}

#[test]
fn a_fully_enclosed_cavity_has_inward_normals() {
    let outer = block(Vec3::splat(2.0), Vec3::ZERO);
    let inner = block(Vec3::splat(1.0), Vec3::ZERO);
    let cavity = outer.subtract(inner).expect("subtraction must succeed");
    assert_volume(&cavity, 7.0);
    assert_watertight(&cavity);

    // The cavity is centered, so its wall normals point back toward the
    // outer cube's own center (into the empty pocket), the opposite sign
    // from the outer hull's faces, which point away from it.
    let center = DVec3::ZERO;
    let (mut hull, mut cavity_faces) = (0, 0);
    for polygon in &cavity.polygons {
        let centroid: DVec3 =
            polygon.vertices.iter().copied().sum::<DVec3>() / polygon.vertices.len() as f64;
        let outward = polygon.plane.normal.dot(centroid - center);
        if centroid.abs().max_element() > 0.9 {
            assert!(outward > 0.0, "outer hull face must point away from center");
            hull += 1;
        } else {
            assert!(
                outward < 0.0,
                "cavity wall must point toward center (inward)"
            );
            cavity_faces += 1;
        }
    }
    assert!(hull > 0 && cavity_faces > 0, "both shells must be present");
}

#[test]
fn union_of_overlapping_cubes_pins_exact_geometry() {
    // [0,2]^3 union [1,3]^3: overlap is exactly [1,2]^3, volume 1.
    let a = block(Vec3::splat(2.0), Vec3::splat(1.0));
    let b = block(Vec3::splat(2.0), Vec3::splat(2.0));
    let merged = a.union(b).expect("union must succeed");
    assert_volume(&merged, 15.0);
    assert_watertight(&merged);
}

#[test]
fn subtracting_a_disjoint_box_changes_nothing() {
    let a = block(Vec3::splat(1.0), Vec3::ZERO);
    let far = block(Vec3::splat(1.0), Vec3::new(5.0, 0.0, 0.0));
    assert_volume(&a.subtract(far).unwrap(), 1.0);
}

#[test]
fn subtracting_an_enclosing_box_leaves_nothing() {
    let a = block(Vec3::splat(1.0), Vec3::ZERO);
    let big = block(Vec3::splat(4.0), Vec3::ZERO);
    let gone = a.subtract(big).unwrap();
    assert!(gone.is_empty());
}

#[test]
fn union_of_overlapping_boxes_counts_the_overlap_once() {
    let a = block(Vec3::splat(2.0), Vec3::ZERO);
    let b = block(Vec3::splat(2.0), Vec3::new(1.0, 0.0, 0.0));
    // 8 + 8 - the 1x2x2 shared slab.
    assert_volume(&a.union(b).unwrap(), 12.0);
}

#[test]
fn union_of_disjoint_boxes_adds_their_volumes() {
    let a = block(Vec3::splat(1.0), Vec3::ZERO);
    let b = block(Vec3::splat(1.0), Vec3::new(3.0, 0.0, 0.0));
    assert_volume(&a.union(b).unwrap(), 2.0);
}

#[test]
fn coplanar_faces_do_not_leak() {
    // Two unit cubes sharing a whole face: the classic BSP trouble case.
    let a = block(Vec3::splat(1.0), Vec3::ZERO);
    let b = block(Vec3::splat(1.0), Vec3::new(1.0, 0.0, 0.0));
    assert_volume(&a.union(b).unwrap(), 2.0);
}

#[test]
fn carving_a_ball_out_of_a_block_keeps_the_result_closed() {
    let block = block(Vec3::splat(2.0), Vec3::ZERO);
    let ball = Solid::from_mesh(&shapes::sphere(), Mat4::from_scale(Vec3::splat(2.0)));
    let ball_volume = ball.volume();
    let carved = block.subtract(ball).unwrap();
    assert_watertight(&carved);
    assert_volume(&carved, 8.0 - ball_volume);
}

#[test]
fn the_mesh_carries_flat_normals_and_bounds() {
    let mesh = block(Vec3::new(2.0, 4.0, 6.0), Vec3::ZERO).to_mesh();
    assert_eq!(mesh.indices.len(), 36);
    assert_eq!(mesh.bounds.min, [-1.0, -2.0, -3.0]);
    assert_eq!(mesh.bounds.max, [1.0, 2.0, 3.0]);
    for vertex in &mesh.vertices {
        let normal = Vec3::from(vertex.normal);
        assert!((normal.length() - 1.0).abs() < 1e-5);
        // Every normal is axis-aligned: flat shading, no smoothing across faces.
        assert!(normal.abs().max_element() > 0.999);
    }
}

#[test]
fn the_mesh_projects_uvs_along_each_facets_dominant_axis() {
    let mesh = block(Vec3::new(2.0, 4.0, 6.0), Vec3::ZERO).to_mesh();

    // Every corner used to hardcode uv (0, 0): assert they actually vary now.
    assert!(
        mesh.vertices
            .windows(2)
            .any(|pair| pair[0].uv != pair[1].uv),
        "uvs must vary across the mesh, not all read the same texel"
    );

    for vertex in &mesh.vertices {
        let normal = Vec3::from(vertex.normal);
        let position = Vec3::from(vertex.position);
        // An axis-aligned box face: the UV must come from the two axes
        // perpendicular to the (axis-aligned) normal, scaled by the same
        // texel density an ordinary part's material tiles at.
        let expected = match normal.abs().to_array() {
            [x, _, _] if x > 0.5 => [position.y, position.z],
            [_, y, _] if y > 0.5 => [position.x, position.z],
            _ => [position.x, position.y],
        }
        .map(|component| component / rbx_materials::DEFAULT_STUDS_PER_TILE);
        assert_eq!(vertex.uv, expected);
    }
}

#[test]
fn a_tree_unions_additive_leaves_then_carves_negated_ones() {
    let tree = union_of(vec![
        leaf(ShapeKind::Box, Vec3::splat(2.0), Vec3::ZERO, false),
        leaf(
            ShapeKind::Box,
            Vec3::splat(2.0),
            Vec3::new(1.0, 0.0, 0.0),
            false,
        ),
        leaf(
            ShapeKind::Box,
            Vec3::splat(1.0),
            Vec3::new(0.5, 0.0, 0.0),
            true,
        ),
    ]);
    // 12 (see the overlapping union) minus a 1x1x1 bite fully inside it.
    assert_volume(&evaluate(&tree).unwrap(), 11.0);
}

#[test]
fn evaluate_a_single_box_minus_box_tree_stays_positive_and_bounded() {
    // The volume/orientation gate `evaluate` runs on every result: a single
    // box carved by an overlapping box must land strictly between empty and
    // its own uncarved volume, never negative (an inside-out result) and
    // never above the additive ceiling (see `additive_volume_bound`).
    let tree = union_of(vec![
        leaf(ShapeKind::Box, Vec3::splat(2.0), Vec3::ZERO, false),
        leaf(
            ShapeKind::Box,
            Vec3::splat(2.0),
            Vec3::new(1.0, 1.0, 1.0),
            true,
        ),
    ]);
    let solid = evaluate(&tree).expect("a partial overlap must stay a valid solid");
    // 8 minus the 1x1x1 corner shared with the negated box.
    assert_volume(&solid, 7.0);
    assert!(
        solid.volume() > 0.0,
        "volume must be positive, not inverted"
    );
    assert!(
        solid.volume() < 8.0,
        "a subtract can only shrink the additive base"
    );
}

#[test]
fn a_disconnected_boolean_keeps_only_its_largest_piece() {
    // Two additive boxes joined only at a shared corner, both then carved
    // through their middle by one wide negation: the corner link is cut
    // away, leaving two separate solids. `evaluate` keeps just the bigger
    // one rather than handing the renderer a debris field — see
    // `largest_connected_component`.
    let tree = union_of(vec![
        leaf(
            ShapeKind::Box,
            Vec3::splat(2.0),
            Vec3::new(-1.0, 0.0, 0.0),
            false,
        ),
        leaf(
            ShapeKind::Box,
            Vec3::splat(4.0),
            Vec3::new(2.0, 0.0, 0.0),
            false,
        ),
        leaf(ShapeKind::Box, Vec3::new(0.5, 6.0, 6.0), Vec3::ZERO, true),
    ]);
    let solid = evaluate(&tree).expect("two disjoint remainders must still resolve");
    // Only the larger box's own trimmed remainder (4x4x4 minus a 0.25-thick
    // slice = 60.0) survives; the smaller box's 7.0 remainder is discarded.
    assert_volume(&solid, 60.0);
}

#[test]
fn a_negated_compound_carves_all_of_its_pieces() {
    let tree = union_of(vec![
        leaf(ShapeKind::Box, Vec3::splat(4.0), Vec3::ZERO, false),
        Node::Operation {
            negate: true,
            children: vec![
                leaf(
                    ShapeKind::Box,
                    Vec3::splat(1.0),
                    Vec3::new(1.0, 0.0, 0.0),
                    false,
                ),
                leaf(
                    ShapeKind::Box,
                    Vec3::splat(1.0),
                    Vec3::new(-1.0, 0.0, 0.0),
                    false,
                ),
            ],
        },
    ]);
    assert_volume(&evaluate(&tree).unwrap(), 62.0);
}

#[test]
fn a_tree_with_nothing_additive_is_empty_not_a_mesh() {
    let tree = union_of(vec![leaf(ShapeKind::Box, Vec3::ONE, Vec3::ZERO, true)]);
    assert_eq!(evaluate(&tree).err(), Some(Failure::Empty));
}

#[test]
fn a_fully_carved_tree_is_empty_not_a_mesh() {
    let tree = union_of(vec![
        leaf(ShapeKind::Box, Vec3::ONE, Vec3::ZERO, false),
        leaf(ShapeKind::Box, Vec3::splat(3.0), Vec3::ZERO, true),
    ]);
    assert_eq!(evaluate(&tree).err(), Some(Failure::Empty));
}

#[test]
fn a_zero_sized_leaf_contributes_nothing() {
    let tree = union_of(vec![
        leaf(ShapeKind::Box, Vec3::ONE, Vec3::ZERO, false),
        leaf(ShapeKind::Ball, Vec3::ZERO, Vec3::ZERO, false),
    ]);
    assert_volume(&evaluate(&tree).unwrap(), 1.0);
}

/// Every asset this module has been tested against so far (see
/// `super::super::tests`'s real-rock fixture) is one additive leaf with many
/// negations — it never exercises `Solid::union` at all. A builder's organic
/// rock is the opposite shape: several overlapping *additive* boulders with
/// no negation in sight. This constructs one purely through the tree API and
/// checks every surviving facet's plane still points outward, via a ray-cast
/// parity oracle rather than a known-in-advance reference point (there is no
/// simple analytic "inside" point for an irregular 4-lump union).
#[test]
fn a_multi_lump_organic_union_keeps_every_facet_wound_outward() {
    let tree = union_of(vec![
        leaf(ShapeKind::Box, Vec3::new(3.0, 2.5, 3.0), Vec3::ZERO, false),
        rotated_leaf(
            ShapeKind::Box,
            Vec3::new(2.5, 2.0, 2.8),
            Vec3::new(1.3, 0.4, 0.6),
            Quat::from_rotation_y(0.44),
            false,
        ),
        leaf(
            ShapeKind::Ball,
            Vec3::splat(2.2),
            Vec3::new(-1.0, 0.3, 0.8),
            false,
        ),
        rotated_leaf(
            ShapeKind::Box,
            Vec3::splat(2.0),
            Vec3::new(0.5, -0.8, -1.0),
            Quat::from_rotation_x(0.26),
            false,
        ),
    ]);
    let solid = evaluate(&tree).expect("a multi-lump additive union must resolve");
    assert!(
        solid.polygons.len() > 20,
        "the lump cluster should fragment into more than a handful of facets"
    );

    let triangles: Vec<[DVec3; 3]> = solid
        .polygons
        .iter()
        .flat_map(|polygon| {
            (1..polygon.vertices.len() - 1).map(|i| {
                [
                    polygon.vertices[0],
                    polygon.vertices[i],
                    polygon.vertices[i + 1],
                ]
            })
        })
        .collect();
    // Deliberately not axis-aligned and not a multiple of either rotation
    // above, so it is vanishingly unlikely to graze an edge or vertex exactly.
    let ray_dir = DVec3::new(0.481, 0.601, 0.638).normalize();

    let mut inverted = Vec::new();
    for polygon in &solid.polygons {
        let centroid: DVec3 =
            polygon.vertices.iter().copied().sum::<DVec3>() / polygon.vertices.len() as f64;
        let min_edge = polygon
            .vertices
            .iter()
            .zip(polygon.vertices.iter().cycle().skip(1))
            .map(|(&a, &b)| (b - a).length())
            .fold(f64::INFINITY, f64::min);
        // Skip slivers too thin for a surface-offset probe to stay meaningful.
        if !(min_edge.is_finite()) || min_edge < 1e-4 {
            continue;
        }
        let probe = centroid + polygon.plane.normal * (min_edge * 1e-3).min(1e-4);
        let crossings = triangles
            .iter()
            .filter(|triangle| ray_triangle_hit(probe, ray_dir, triangle).is_some())
            .count();
        if crossings % 2 != 0 {
            inverted.push((centroid, polygon.plane.normal));
        }
    }
    assert!(
        inverted.is_empty(),
        "facet(s) wound inward (normal points into the solid, not out of it): {inverted:?}"
    );
}

/// Same convex-polygon Möller–Trumbore test `discard_disconnected_debris`'s
/// neighbours already lean on elsewhere in this module, exposed here for the
/// ray-cast oracle above: `None` for a miss or an intersection behind `orig`.
fn ray_triangle_hit(orig: DVec3, dir: DVec3, [v0, v1, v2]: &[DVec3; 3]) -> Option<f64> {
    const EPS: f64 = 1e-9;
    let edge1 = *v1 - *v0;
    let edge2 = *v2 - *v0;
    let h = dir.cross(edge2);
    let a = edge1.dot(h);
    if a.abs() < EPS {
        return None;
    }
    let f = 1.0 / a;
    let s = orig - *v0;
    let u = f * s.dot(h);
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(edge1);
    let v = f * dir.dot(q);
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = f * edge2.dot(q);
    (t > EPS).then_some(t)
}

fn rotated_leaf(kind: ShapeKind, size: Vec3, at: Vec3, rotation: Quat, negate: bool) -> Node {
    Node::Leaf(Leaf {
        negate,
        geometry: Geometry {
            kind,
            size,
            offset: Vec3::ZERO,
        },
        cframe: Mat4::from_rotation_translation(rotation, at),
        properties: BTreeMap::new(),
    })
}

/// Stamps a leaf `Node` with its own pre-union `Color3uint8`, as a real
/// `BasePart` would carry before Studio baked the union.
fn colored(mut node: Node, color: [u8; 3]) -> Node {
    if let Node::Leaf(leaf) = &mut node {
        leaf.properties.insert(
            "Color3uint8".to_string(),
            Variant::Color3uint8 {
                r: color[0],
                g: color[1],
                b: color[2],
            },
        );
    }
    node
}

/// A union whose first-decoded additive leaf is the *smallest* piece and
/// carries a stray dark colour, while the largest piece (decoded later)
/// carries the intended light one. "First in tree order" is the wire order a
/// real asset just happens to decode in, not anything a builder controls, so
/// [`largest_additive_color`] must pick the leaf that actually dominates the
/// union's volume instead.
#[test]
fn largest_additive_color_prefers_the_largest_leaf_over_tree_order() {
    let tree = union_of(vec![
        colored(
            leaf(ShapeKind::Box, Vec3::splat(1.0), Vec3::ZERO, false),
            [10, 8, 6],
        ),
        colored(
            leaf(
                ShapeKind::Box,
                Vec3::splat(3.0),
                Vec3::new(0.4, 0.0, 0.0),
                false,
            ),
            [200, 195, 190],
        ),
    ]);
    assert_eq!(largest_additive_color(&tree), Some([200, 195, 190]));
}

#[test]
fn an_absurdly_wide_tree_is_refused_up_front() {
    let children = (0..600)
        .map(|i| {
            leaf(
                ShapeKind::Box,
                Vec3::ONE,
                Vec3::new(i as f32 * 2.0, 0.0, 0.0),
                false,
            )
        })
        .collect();
    assert_eq!(
        evaluate(&union_of(children)).err(),
        Some(Failure::TooComplex)
    );
}
