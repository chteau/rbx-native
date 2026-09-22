use glam::{Mat4, Quat, Vec3};

use super::*;
use crate::gizmo::basis;
use crate::Pose;

fn handles() -> Handles {
    Handles::new(Vec3::new(4.0, 1.0, -2.0), basis(None), 3.0)
}

fn pose(position: Vec3) -> Pose {
    Pose {
        position,
        yaw: 0.0,
        pitch: 0.0,
        fov_degrees: 70.0,
        ortho_scale: 25.0,
    }
}

/// A 4×2×6 part standing where [`handles`] puts its gizmo, seen from well
/// outside it.
fn faces() -> Faces {
    let model = Mat4::from_scale_rotation_translation(
        Vec3::new(4.0, 2.0, 6.0),
        Quat::IDENTITY,
        Vec3::new(4.0, 1.0, -2.0),
    );
    Faces::new(model, pose(Vec3::splat(40.0)), false)
}

/// No handle held: every one drawn.
fn every(_: Axis, _: f32) -> bool {
    true
}

/// Every tool's geometry, for the checks that have to hold for all three.
fn shapes() -> [Shape; 3] {
    [
        Shape::Move(handles()),
        Shape::Scale(faces()),
        Shape::Rotate(handles()),
    ]
}

/// Twice the enclosed volume, by the divergence theorem: positive only if
/// every triangle in the mesh is wound the same way round and facing outwards,
/// which is what the handles rely on to keep back-face culling from eating
/// their front halves (or, worse, showing both).
fn signed_volume(vertices: &[Vertex]) -> f32 {
    vertices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|triangle| {
            let [a, b, c] = [0, 1, 2].map(|corner| Vec3::from(triangle[corner].position));
            a.dot(b.cross(c))
        })
        .sum()
}

#[test]
fn a_gizmo_fits_the_buffer_it_reserved() {
    for shape in shapes() {
        let vertices = mesh(&shape, None, Vec3::splat(40.0));
        assert!(
            vertices.len() <= CAPACITY,
            "{shape:?} overruns the buffer at {} vertices",
            vertices.len()
        );
        assert_eq!(vertices.len() % 3, 0, "the mesh is a triangle list");
        assert!(!vertices.is_empty(), "{shape:?} drew nothing");
    }
}

#[test]
fn the_move_arms_fill_exactly_what_they_reserve() {
    let vertices = arms(&handles(), Vec3::splat(40.0), arrow, every);
    assert_eq!(vertices.len(), ARROW_VERTICES);
}

#[test]
fn no_move_arrow_is_drawn_further_out_than_an_arm() {
    let handles = handles();
    let arm = handles.arm();
    let furthest = mesh(&Shape::Move(handles), None, Vec3::splat(40.0))
        .iter()
        .map(|vertex| (Vec3::from(vertex.position) - handles.origin()).length())
        .fold(0.0f32, f32::max);

    // An arrow's point stops dead at the arm, and something does reach it.
    assert!(furthest <= arm + 1e-4, "an arrow reaches {furthest}");
    assert!(furthest >= arm - 1e-3, "an arrow stops short at {furthest}");
}

/// The whole of the fix: Scale's balls are bound to the part's own surface,
/// so what limits them is the part's size, not a screen-relative arm out of
/// its centre.
#[test]
fn every_scale_ball_is_drawn_on_the_face_it_resizes() {
    let faces = faces();
    let vertices = mesh(&Shape::Scale(faces), None, Vec3::splat(40.0));

    for vertex in vertices {
        let point = Vec3::from(vertex.position);
        // Whichever axis's colour it carries names the pair of faces it can
        // belong to, and it has to be on the surface of one of them.
        let axis = Axis::ALL
            .into_iter()
            .find(|axis| axis.color().map(f32::to_bits) == vertex.color.map(f32::to_bits))
            .expect("every ball is one axis's colour");
        let off = [1.0f32, -1.0]
            .into_iter()
            .map(|sign| (point - faces.handle(axis, sign)).length() - faces.radius(axis, sign))
            .fold(f32::INFINITY, f32::min);
        assert!(off <= 1e-3, "a ball vertex {off} studs off its own face");
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
    assert!(signed_volume(&arms(&handles(), Vec3::splat(40.0), arrow, every)) > 0.0);
}

/// A torus is a closed surface, so the same divergence-theorem check that
/// catches an inside-out arrow catches an inside-out ring.
#[test]
fn the_rotation_rings_are_wound_outwards() {
    let volume = signed_volume(&rings(&handles(), Vec3::splat(40.0)));
    assert!(volume > 0.0, "inside-out rings enclose {volume}");
}

/// A ball is a closed surface, so the same divergence-theorem check that
/// catches an inside-out arrow catches an inside-out one of these — but only
/// about its own centre, which is why it is built at the origin here.
#[test]
fn a_scale_ball_is_wound_outwards() {
    let mut sphere = Vec::new();
    ball(&mut sphere, Vec3::ZERO, 2.0, [0.0, 1.0, 0.0]);

    assert_eq!(sphere.len(), VERTICES_PER_BALL);
    let volume = signed_volume(&sphere);
    assert!(volume > 0.0, "an inside-out ball encloses {volume}");
    // And it really is a ball, not a hull that misses most of one: six times
    // the volume of a sphere of this radius, less what a 12×6 facetting loses.
    let sphere_volume = 6.0 * 4.0 / 3.0 * std::f32::consts::PI * 8.0;
    assert!(volume > sphere_volume * 0.8, "{volume} is not a whole ball");
    assert!(volume <= sphere_volume, "{volume} is more than a sphere");
}

/// Every vertex of a ball stands its radius off the face it is centred on, in
/// every direction — the check that it is a sphere rather than a disc or a
/// cone.
#[test]
fn a_scale_ball_is_round() {
    let mut sphere = Vec::new();
    ball(&mut sphere, Vec3::new(3.0, -1.0, 2.0), 0.5, [0.0, 1.0, 0.0]);

    for vertex in &sphere {
        let out = (Vec3::from(vertex.position) - Vec3::new(3.0, -1.0, 2.0)).length();
        assert!((out - 0.5).abs() < 1e-5, "a vertex {out} from the centre");
    }
    // Both poles and every direction round the equator are actually drawn.
    for direction in [Vec3::X, Vec3::NEG_X, Vec3::Y, Vec3::NEG_Y, Vec3::Z] {
        assert!(
            sphere.iter().any(|vertex| {
                (Vec3::from(vertex.position) - Vec3::new(3.0, -1.0, 2.0)).dot(direction) > 0.49
            }),
            "nothing drawn towards {direction:?}"
        );
    }
}

/// With the depth test off, a ball on the near face has to be painted after
/// the one on the far face or the part's back would show through its front.
#[test]
fn the_scale_balls_are_painted_back_to_front() {
    let faces = faces();
    let eye = Vec3::new(4.0, 1.0, 40.0);
    let vertices = balls(&faces, eye, every);

    let mut previous = f32::INFINITY;
    for chunk in vertices.as_chunks::<VERTICES_PER_BALL>().0 {
        // Which ball this is, by the face its vertices stand around — its own
        // vertices are all a radius off that, and no two faces are that close
        // together.
        let middle =
            chunk.iter().map(|v| Vec3::from(v.position)).sum::<Vec3>() / chunk.len() as f32;
        let depth = faces
            .all()
            .map(|(axis, sign)| faces.handle(axis, sign))
            .min_by(|a, b| (*a - middle).length().total_cmp(&(*b - middle).length()))
            .map(|handle| (handle - eye).length())
            .expect("six balls");
        assert!(
            depth <= previous + 1e-3,
            "a ball at {depth} painted after one at {previous}"
        );
        previous = depth;
    }
}

#[test]
fn the_arms_are_painted_back_to_front() {
    // Looking down the X axis from +X: the arm pointing away from the eye is
    // painted first and the one reaching towards it last, so that with the
    // depth test off the near arm still ends up on top.
    let handles = Handles::new(Vec3::ZERO, basis(None), 1.0);
    let vertices = arms(&handles, Vec3::new(100.0, 0.0, 0.0), arrow, every);

    let (chunks, _) = vertices.as_chunks::<VERTICES_PER_ARM>();
    let first = chunks.first().expect("six arms");
    let last = chunks.last().expect("six arms");

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
    for slice in vertices.as_chunks::<VERTICES_PER_SLICE>().0 {
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
    for shape in shapes() {
        let vertices = mesh(&shape, None, Vec3::splat(40.0));
        let colors: std::collections::HashSet<[u32; 3]> = vertices
            .iter()
            .map(|vertex| vertex.color.map(f32::to_bits))
            .collect();

        assert_eq!(colors.len(), 3, "{shape:?}: three axes, three colours");
        for axis in Axis::ALL {
            assert!(colors.contains(&axis.color().map(f32::to_bits)));
        }
    }
}

#[test]
fn a_local_gizmo_points_along_the_parts_own_axes() {
    let rotation = glam::Mat3::from_rotation_y(std::f32::consts::FRAC_PI_2);
    let handles = Handles::new(Vec3::ZERO, basis(Some(rotation)), 1.0);
    let vertices = arms(&handles, Vec3::splat(40.0), arrow, every);

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

/// A drag draws the one handle it holds: Studio builds just the dragged
/// Move arrow or Scale ball while it is held, and every handle again after.
#[test]
fn a_held_handle_is_drawn_alone() {
    let eye = Vec3::splat(40.0);
    for shape in [Shape::Move(handles()), Shape::Scale(faces())] {
        let all = mesh(&shape, None, eye).len();
        let held = End::of((Axis::X, -1.0));
        let one = mesh(&shape, Some(held), eye);
        assert_eq!(one.len() * 6, all, "{shape:?}");
    }
    // Rotate has no ends to hold; its rings are untouched.
    let rings = mesh(&Shape::Rotate(handles()), None, eye).len();
    let held = Some(End::of((Axis::Y, 1.0)));
    assert_eq!(mesh(&Shape::Rotate(handles()), held, eye).len(), rings);
}
