use glam::{Mat4, Quat, Vec3};

use super::*;
use crate::gizmo::{basis, ORIGIN_RADIUS};
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

/// Every tool's geometry, for the checks that have to hold for all five.
fn shapes() -> [Shape; 5] {
    [
        Shape::Move(handles()),
        Shape::Scale(faces()),
        Shape::Rotate(handles()),
        Shape::Transform(handles(), faces()),
        Shape::Pivot(
            handles(),
            Some((Hotspots::new(faces(), []), Some(faces().centre()))),
        ),
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
    let vertices = arms(&handles(), Vec3::splat(40.0), arrow, every, false);
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
    assert!(signed_volume(&arms(&handles(), Vec3::splat(40.0), arrow, every, false)) > 0.0);
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
    let vertices = arms(&handles, Vec3::new(100.0, 0.0, 0.0), arrow, every, false);

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
        // The free-drag ball's grey is no axis's, and only Move's and
        // Transform's gizmos carry one.
        let colors: std::collections::HashSet<[u32; 3]> = vertices
            .iter()
            .map(|vertex| vertex.color.map(f32::to_bits))
            .filter(|&color| color != ORIGIN_COLOR.map(f32::to_bits))
            .filter(|&color| color != HOTSPOT_COLOR.map(f32::to_bits))
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
    let vertices = arms(&handles, Vec3::splat(40.0), arrow, every, false);

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
    for (shape, origin) in [
        (Shape::Move(handles()), VERTICES_PER_BALL),
        (Shape::Scale(faces()), 0),
    ] {
        let all = mesh(&shape, None, eye).len();
        let held = End::of((Axis::X, -1.0));
        let one = mesh(&shape, Some(held), eye);
        assert_eq!(one.len() * 6 + origin, all, "{shape:?}");
    }
    // Rotate has no ends to hold; its rings are untouched.
    let rings = mesh(&Shape::Rotate(handles()), None, eye).len();
    let held = Some(End::of((Axis::Y, 1.0)));
    assert_eq!(mesh(&Shape::Rotate(handles()), held, eye).len(), rings);
}

/// Transform's combined gizmo is exactly Move's arrows, Scale's balls and
/// Rotate's rings, all drawn at once — not a cut-down version of any of
/// them, since all three are grabbable handles of their own in this tool.
#[test]
fn a_transform_gizmo_draws_every_arm_ball_and_ring() {
    let eye = Vec3::splat(40.0);
    let combined = mesh(&Shape::Transform(handles(), faces()), None, eye).len();
    let separate = mesh(&Shape::Move(handles()), None, eye).len()
        + mesh(&Shape::Scale(faces()), None, eye).len()
        + mesh(&Shape::Rotate(handles()), None, eye).len();
    assert_eq!(combined, separate);
    assert_eq!(combined, TRANSFORM_VERTICES);
}

/// Depth-test-off painting only works if what's nearer the eye is painted
/// last, and until Transform that was only ever checked *within* one kind
/// (`the_arms_are_painted_back_to_front` and its siblings above) — nothing
/// before mixed two kinds on screen at once. This checks the merge actually
/// interleaves by eye distance rather than drawing one kind's six (or
/// `RING_SEGMENTS`) pieces as an unbroken block before the next kind's.
#[test]
fn transform_interleaves_every_kind_by_eye_distance_not_by_kind() {
    // Move's and Rotate's reach (`handles().arm()`, 3 studs) is fixed on
    // screen regardless of the part's own size, but a Scale ball sits on the
    // part's actual surface — so a part stretched far enough towards the eye
    // pokes a ball closer to it than any arm tip or ring can reach, whatever
    // `handles()`'s own fixed arm length is. A part this long along Z, facing
    // the eye, is what forces that.
    let long = Mat4::from_scale_rotation_translation(
        Vec3::new(2.0, 2.0, 40.0),
        Quat::IDENTITY,
        Vec3::new(4.0, 1.0, -2.0),
    );
    let eye = Vec3::new(4.0, 1.0, 38.0);
    let near_faces = Faces::new(long, pose(eye), false);
    let vertices = mesh(&Shape::Transform(handles(), near_faces), None, eye);

    // The last-painted vertex — on top of everything else — has to be one of
    // the near ball's, not an arm's or a ring's, which a per-kind-then-
    // per-kind composition (every arm, then every ring, then every ball, or
    // any other fixed kind order) would get wrong whenever a ball happens to
    // stand nearer the eye than the kind painted after it.
    let ball_depth = near_faces
        .all()
        .map(|(axis, sign)| (near_faces.handle(axis, sign) - eye).length())
        .fold(f32::INFINITY, f32::min);
    let last = Vec3::from(
        vertices
            .last()
            .expect("a transform gizmo draws something")
            .position,
    );
    let last_depth = (last - eye).length();
    assert!(
        (last_depth - ball_depth).abs() < 0.5,
        "the last-painted vertex is {last_depth} studs out, not near the closest ball at {ball_depth}"
    );
}

/// Move's gizmo carries a small grey ball at its origin — the free-drag
/// handle — painted between the arms that point away from the eye and the
/// ones that point towards it, and gone while one arrow is held alone.
#[test]
fn the_free_drag_ball_sits_at_the_origin_between_the_far_and_near_arms() {
    let eye = Vec3::new(40.0, 0.0, 0.0);
    let origin = Vec3::new(4.0, 1.0, -2.0);
    let vertices = mesh(&Shape::Move(handles()), None, eye);
    let grey = ORIGIN_COLOR.map(f32::to_bits);
    let at: Vec<usize> = (0..vertices.len())
        .filter(|&index| vertices[index].color.map(f32::to_bits) == grey)
        .collect();
    assert_eq!(at.len(), VERTICES_PER_BALL);
    let reach = ORIGIN_RADIUS * handles().arm() + 1e-4;
    for &index in &at {
        assert!((Vec3::from(vertices[index].position) - origin).length() <= reach);
    }
    // +X points at this eye, -X away from it: the far arm first, the ball,
    // then the near one.
    let red = Axis::X.color().map(f32::to_bits);
    let reds: Vec<usize> = (0..vertices.len())
        .filter(|&index| vertices[index].color.map(f32::to_bits) == red)
        .collect();
    let (first_red, last_red) = (reds[0], reds[reds.len() - 1]);
    assert!(first_red < at[0] && at[at.len() - 1] < last_red);

    let held = Some(End::of((Axis::Y, 1.0)));
    assert!(mesh(&Shape::Move(handles()), held, eye)
        .iter()
        .all(|vertex| vertex.color.map(f32::to_bits) != grey));
}

#[test]
fn edit_pivot_draws_a_magenta_dot_per_hotspot_and_a_larger_one_where_it_snapped() {
    // Big enough that no two dots overlap.
    let model =
        Mat4::from_scale_rotation_translation(Vec3::splat(20.0), Quat::IDENTITY, Vec3::ZERO);
    let faces = Faces::new(model, pose(Vec3::splat(40.0)), false);
    let hotspots = Hotspots::new(faces, []);
    let snapped = faces.hotspots()[0];
    let vertices = mesh(
        &Shape::Pivot(handles(), Some((hotspots.clone(), Some(snapped)))),
        None,
        Vec3::splat(40.0),
    );
    let magenta: Vec<Vec3> = vertices
        .iter()
        .filter(|vertex| vertex.color == HOTSPOT_COLOR)
        .map(|vertex| Vec3::from(vertex.position))
        .collect();
    assert_eq!(magenta.len(), HOTSPOTS * VERTICES_PER_BALL);

    // How far the dot round each hotspot reaches.
    let reach = |point: Vec3| {
        magenta
            .iter()
            .map(|vertex| (*vertex - point).length())
            .filter(|&distance| distance < hotspots.radius(point) * SNAPPED_SCALE * 1.01)
            .fold(0.0f32, f32::max)
    };
    let other = faces.hotspots()[26];
    assert!((reach(other) - hotspots.radius(other)).abs() < 1e-3);
    assert!((reach(snapped) - hotspots.radius(snapped) * SNAPPED_SCALE).abs() < 1e-3);

    // Snapping off: the same arms, rings and ball, and no dots at all.
    let bare = mesh(&Shape::Pivot(handles(), None), None, Vec3::splat(40.0));
    assert!(bare.iter().all(|vertex| vertex.color != HOTSPOT_COLOR));
    assert_eq!(bare.len(), vertices.len() - magenta.len());
}

/// A model of more parts than the cap fills the pivot gizmo's whole budget
/// and never more: a dot per box hotspot, and seven per capped part.
#[test]
fn a_many_part_model_draws_its_parts_hotspots_up_to_the_cap() {
    let eye = Vec3::splat(40.0);
    let box_of = |centre: Vec3| {
        Faces::new(
            Mat4::from_scale_rotation_translation(Vec3::splat(2.0), Quat::IDENTITY, centre),
            pose(eye),
            false,
        )
    };
    let parts = (0..500).map(|index| box_of(Vec3::X * index as f32 * 3.0));
    let hotspots = Hotspots::new(box_of(Vec3::ZERO), parts);
    let vertices = mesh(&Shape::Pivot(handles(), Some((hotspots, None))), None, eye);
    let magenta = vertices
        .iter()
        .filter(|vertex| vertex.color == HOTSPOT_COLOR)
        .count();
    assert_eq!(
        magenta,
        (HOTSPOTS + HOTSPOT_PARTS * PART_HOTSPOTS) * VERTICES_PER_BALL
    );
    assert_eq!(
        vertices.len(),
        PIVOT_VERTICES,
        "the pivot gizmo's own budget"
    );
}

#[test]
fn a_hovered_ring_is_drawn_brighter_and_alone() {
    let mut vertices = rings(&handles(), Vec3::splat(40.0));
    light(&mut vertices, Axis::Y);
    let lit = vertices
        .iter()
        .filter(|vertex| vertex.color != Axis::Y.color());
    let lit: Vec<_> = lit
        .filter(|vertex| vertex.color != Axis::X.color() && vertex.color != Axis::Z.color())
        .collect();
    assert!(!lit.is_empty(), "the Y ring kept its colour");
    for vertex in &lit {
        assert!(
            vertex
                .color
                .iter()
                .zip(Axis::Y.color())
                .all(|(lit, plain)| *lit > plain),
            "{:?} is not brighter than the plain ring",
            vertex.color
        );
    }
    assert!(
        vertices
            .iter()
            .any(|vertex| vertex.color == Axis::X.color()),
        "the X ring lit up too"
    );
    assert!(
        !vertices
            .iter()
            .any(|vertex| vertex.color == Axis::Y.color()),
        "part of the Y ring stayed unlit"
    );
}

/// A hovered handle stays its own colour, only brighter: the lit Z arrow is
/// still blue, not the washed-out lavender a 55% mix towards white made of
/// it (which, seen end on beside a posed arm, read as a stray shape).
#[test]
fn a_lit_handle_keeps_its_hue() {
    for axis in [Axis::X, Axis::Y, Axis::Z] {
        let plain = axis.color();
        let lit = lit(plain);
        let main = (0..3)
            .max_by(|&a, &b| plain[a].total_cmp(&plain[b]))
            .unwrap();
        assert_eq!(lit[main], 1.0, "{axis:?} is at full strength when lit");
        for channel in (0..3).filter(|&c| c != main) {
            assert!(lit[channel] > plain[channel], "{axis:?} got brighter");
            assert!(
                lit[channel] < 0.3,
                "{axis:?} lit is {lit:?}: washed towards white"
            );
        }
    }
    // The colour sampled off the reported shape, in linear light: the old
    // wash of the Z arrow. The new one is nowhere near it.
    let old = [0.565, 0.598, 0.95];
    let new = lit(Axis::Z.color());
    assert!(new[0] < old[0] - 0.3 && new[1] < old[1] - 0.3, "{new:?}");
}
