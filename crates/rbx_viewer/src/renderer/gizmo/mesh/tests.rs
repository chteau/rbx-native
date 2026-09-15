use glam::Vec3;

use super::*;
use crate::gizmo::basis;

fn handles() -> Handles {
    Handles::new(Vec3::new(4.0, 1.0, -2.0), basis(None), 3.0)
}

/// Twice the enclosed volume, by the divergence theorem: positive only if
/// every triangle in the mesh is wound the same way round and facing outwards,
/// which is what the handles rely on to keep back-face culling from eating
/// their front halves (or, worse, showing both).
fn signed_volume(vertices: &[Vertex]) -> f32 {
    vertices
        .chunks_exact(3)
        .map(|triangle| {
            let [a, b, c] = [0, 1, 2].map(|corner| Vec3::from(triangle[corner].position));
            a.dot(b.cross(c))
        })
        .sum()
}

#[test]
fn a_gizmo_fits_the_buffer_it_reserved() {
    for kind in [Kind::Move, Kind::Scale, Kind::Rotate] {
        let vertices = mesh(kind, &handles(), Vec3::splat(40.0));
        assert!(
            vertices.len() <= CAPACITY,
            "{kind:?} overruns the buffer at {} vertices",
            vertices.len()
        );
        assert_eq!(vertices.len() % 3, 0, "the mesh is a triangle list");
        assert!(!vertices.is_empty(), "{kind:?} drew nothing");
    }
}

#[test]
fn the_move_arms_fill_exactly_what_they_reserve() {
    let vertices = arms(&handles(), Vec3::splat(40.0), arrow);
    assert_eq!(vertices.len(), ARROW_VERTICES);
}

#[test]
fn nothing_is_drawn_further_out_than_an_arm() {
    let handles = handles();
    // A ring's tube straddles its own circle and a block's corners stand off
    // its axis, so neither stops dead at the arm; an arrow's point does.
    let arm = handles.arm();
    for (kind, slack) in [(Kind::Move, 1e-4), (Kind::Scale, HANDLE_RADIUS * arm)] {
        let reaches: Vec<f32> = mesh(kind, &handles, Vec3::splat(40.0))
            .iter()
            .map(|vertex| (Vec3::from(vertex.position) - handles.origin()).length())
            .collect();

        let furthest = reaches.iter().copied().fold(0.0f32, f32::max);
        assert!(furthest <= arm + slack, "{kind:?} reaches {furthest}");
        // And something does reach the full arm: the arrow's point, and the
        // outer face of the Scale block.
        assert!(furthest >= arm - 1e-3, "{kind:?} stops short at {furthest}");
    }
}

#[test]
fn every_arrow_is_wound_outwards() {
    let mut arrow_mesh = Vec::new();
    arrow(&mut arrow_mesh, Vec3::ZERO, Vec3::X, 2.0, [1.0, 0.0, 0.0]);
    assert!(
        signed_volume(&arrow_mesh) > 0.0,
        "an inside-out arrow: back-face culling would hollow it out"
    );

    // The same the other way along the axis, where the frame the ring is
    // built in flips with it.
    let mut backwards = Vec::new();
    arrow(
        &mut backwards,
        Vec3::ZERO,
        Vec3::NEG_X,
        2.0,
        [1.0, 0.0, 0.0],
    );
    assert!(signed_volume(&backwards) > 0.0);
}

#[test]
fn a_whole_gizmo_is_wound_outwards() {
    assert!(signed_volume(&arms(&handles(), Vec3::splat(40.0), arrow)) > 0.0);
}

/// A torus is a closed surface, so the same divergence-theorem check that
/// catches an inside-out arrow catches an inside-out ring.
#[test]
fn the_rotation_rings_are_wound_outwards() {
    let volume = signed_volume(&rings(&handles(), Vec3::splat(40.0)));
    assert!(volume > 0.0, "inside-out rings enclose {volume}");
}

/// The blocks are built face by face rather than swept like the shafts, so the
/// winding is checked against each face's own outward direction — a closed
/// convex box has every triangle facing away from its centre.
#[test]
fn a_scale_block_faces_outwards_on_every_side() {
    let (origin, direction, arm) = (Vec3::ZERO, Vec3::Y, 4.0);
    let mut block = Vec::new();
    handle(&mut block, origin, direction, arm, [0.0, 1.0, 0.0]);

    let centre = origin + direction * (arm - HANDLE_RADIUS * arm);
    // The shaft comes first and is swept, not built from quads; only the last
    // `VERTICES_PER_BLOCK` vertices are the block.
    let block = &block[block.len() - VERTICES_PER_BLOCK..];
    for triangle in block.chunks_exact(3) {
        let [a, b, c] = [0, 1, 2].map(|corner| Vec3::from(triangle[corner].position));
        let normal = (b - a).cross(c - a);
        let outward = (a + b + c) / 3.0 - centre;
        assert!(
            normal.dot(outward) > 0.0,
            "a block face wound back towards its own centre"
        );
    }
}

#[test]
fn the_arms_are_painted_back_to_front() {
    // Looking down the X axis from +X: the arm pointing away from the eye is
    // painted first and the one reaching towards it last, so that with the
    // depth test off the near arm still ends up on top.
    let handles = Handles::new(Vec3::ZERO, basis(None), 1.0);
    let vertices = arms(&handles, Vec3::new(100.0, 0.0, 0.0), arrow);

    let mut chunks = vertices.chunks_exact(VERTICES_PER_ARM);
    let first = chunks.next().expect("six arms");
    let last = chunks.next_back().expect("six arms");

    let reach = |arm: &[Vertex], pick: fn(f32, f32) -> f32, seed: f32| {
        arm.iter().map(|vertex| vertex.position[0]).fold(seed, pick)
    };
    assert!(
        reach(first, f32::min, 0.0) < -0.9,
        "the far arm is painted first"
    );
    assert!(
        reach(last, f32::max, 0.0) > 0.9,
        "the near arm is painted last"
    );
}

/// Three rings of one radius about one centre pass through each other's
/// planes, so only a per-slice order can be right — whole rings have no
/// correct order at all.
#[test]
fn the_ring_slices_are_painted_back_to_front() {
    let handles = Handles::new(Vec3::ZERO, basis(None), 1.0);
    let eye = Vec3::new(0.0, 0.0, 100.0);
    let vertices = rings(&handles, eye);

    let depth = |slice: &[Vertex]| {
        slice
            .iter()
            .map(|vertex| (Vec3::from(vertex.position) - eye).length())
            .sum::<f32>()
            / slice.len() as f32
    };
    let mut previous = f32::INFINITY;
    for slice in vertices.chunks_exact(VERTICES_PER_SLICE) {
        let now = depth(slice);
        assert!(
            now <= previous + 1e-3,
            "a slice at {now} painted after one at {previous}"
        );
        previous = now;
    }
}

#[test]
fn each_ring_lies_in_the_plane_of_its_own_axis() {
    let handles = Handles::new(Vec3::ZERO, basis(None), 2.0);
    let tube = RING_THICKNESS * handles.arm();

    for vertex in rings(&handles, Vec3::splat(40.0)) {
        let point = Vec3::from(vertex.position);
        // Whichever axis's colour it carries is the axis it turns about, so
        // that component is the one the tube's thickness has to account for.
        let axis = Axis::ALL
            .into_iter()
            .find(|axis| axis.color().map(f32::to_bits) == vertex.color.map(f32::to_bits))
            .expect("every ring is one axis's colour");
        let off_plane = point[axis as usize].abs();
        assert!(off_plane <= tube + 1e-4, "{off_plane} off the ring's plane");

        let radius = (point - handles.direction(axis) * point[axis as usize]).length();
        assert!(
            (radius - RING_RADIUS * handles.arm()).abs() <= tube + 1e-4,
            "a ring vertex {radius} studs out"
        );
    }
}

#[test]
fn each_arm_carries_its_axis_colour() {
    for kind in [Kind::Move, Kind::Scale, Kind::Rotate] {
        let vertices = mesh(kind, &handles(), Vec3::splat(40.0));
        let colors: std::collections::HashSet<[u32; 3]> = vertices
            .iter()
            .map(|vertex| vertex.color.map(f32::to_bits))
            .collect();

        assert_eq!(colors.len(), 3, "{kind:?}: three axes, three colours");
        for axis in Axis::ALL {
            assert!(colors.contains(&axis.color().map(f32::to_bits)));
        }
    }
}

#[test]
fn a_local_gizmo_points_along_the_parts_own_axes() {
    let rotation = glam::Mat3::from_rotation_y(std::f32::consts::FRAC_PI_2);
    let handles = Handles::new(Vec3::ZERO, basis(Some(rotation)), 1.0);
    let vertices = arms(&handles, Vec3::splat(40.0), arrow);

    // The red (X) arrow's tip now stands on world -Z, not world +X.
    let red = Axis::X.color().map(f32::to_bits);
    let reach = vertices
        .iter()
        .filter(|vertex| vertex.color.map(f32::to_bits) == red)
        .map(|vertex| Vec3::from(vertex.position))
        .fold(Vec3::ZERO, |furthest, point| {
            if point.length() > furthest.length() {
                point
            } else {
                furthest
            }
        });
    assert!(reach.x.abs() < 1e-4);
    assert!(reach.z.abs() > 0.9);
}

/// The red ring turns about the part's own X, so with the part a quarter turn
/// about Y it has to stand in the plane perpendicular to world -Z.
#[test]
fn a_local_rotation_ring_stands_in_the_parts_own_plane() {
    let rotation = glam::Mat3::from_rotation_y(std::f32::consts::FRAC_PI_2);
    let handles = Handles::new(Vec3::ZERO, basis(Some(rotation)), 2.0);
    let tube = RING_THICKNESS * handles.arm();
    let red = Axis::X.color().map(f32::to_bits);

    for vertex in rings(&handles, Vec3::splat(40.0)) {
        if vertex.color.map(f32::to_bits) != red {
            continue;
        }
        let point = Vec3::from(vertex.position);
        assert!(point.z.abs() <= tube + 1e-4, "the red ring left world Z");
    }
}
