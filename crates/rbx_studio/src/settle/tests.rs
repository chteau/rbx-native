use glam::{Mat4, Vec3};
use rbx_dom::{CFrameData, Variant, Vector3Data, WeakDom};

use super::*;

fn part(dom: &mut WeakDom, parent: Ref, name: &str, position: Vec3, size: Vec3) -> Ref {
    let part = dom.new_instance("Part", name, Some(parent));
    let _ = dom.set_property(
        part,
        "CFrame",
        Variant::CFrame(CFrameData {
            position: Vector3Data {
                x: position.x,
                y: position.y,
                z: position.z,
            },
            rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        }),
    );
    let _ = dom.set_property(
        part,
        "size",
        Variant::Vector3(Vector3Data {
            x: size.x,
            y: size.y,
            z: size.z,
        }),
    );
    part
}

/// A ground whose top is y=0, a 4-stud block standing on it spanning x=18..22
/// and z=-2..2, and a 2-stud crate resting on the ground at the origin —
/// returned as `(dom, crate)`. The crate is what every test drags.
fn tabletop() -> (WeakDom, Ref) {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    part(
        &mut dom,
        workspace,
        "Ground",
        Vec3::new(0.0, -8.0, 0.0),
        Vec3::new(512.0, 16.0, 512.0),
    );
    part(
        &mut dom,
        workspace,
        "Block",
        Vec3::new(20.0, 2.0, 0.0),
        Vec3::splat(4.0),
    );
    let dragged = part(
        &mut dom,
        workspace,
        "Crate",
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::splat(2.0),
    );
    (dom, dragged)
}

const CRATE_CENTRE: Vec3 = Vec3::new(0.0, 1.0, 0.0);

fn down_at(x: f32, z: f32) -> Ray {
    Ray::new(Vec3::new(x, 50.0, z), Vec3::NEG_Y)
}

/// The crate grabbed from straight above, by the middle of its top face.
fn grabbed_from_above(cursor: Ray) -> Settle {
    Settle {
        cursor,
        grab: Ray::new(Vec3::new(0.0, 2.0, 0.0), Vec3::NEG_Y),
        centre: CRATE_CENTRE,
    }
}

fn close(a: Vec3, b: Vec3) -> bool {
    (a - b).length() < 1e-3
}

#[test]
fn a_drag_over_a_raised_part_settles_on_top_of_it() {
    let (dom, dragged) = tabletop();
    let database = ReflectionDatabase::embedded();

    let rested = settled(
        &dom,
        &database,
        dragged,
        grabbed_from_above(down_at(20.0, 0.0)),
    )
    .expect("the cursor is over the block");
    // The block's top is at y=4; a 2-stud crate resting on it has its centre
    // a stud higher.
    assert!(
        close(rested, Vec3::new(20.0, 5.0, 0.0)),
        "rested at {rested}"
    );
}

#[test]
fn a_drag_over_open_space_has_no_surface_to_settle_on() {
    let (dom, dragged) = tabletop();
    let database = ReflectionDatabase::embedded();

    // Past the edge of the ground: nothing under the cursor at all, so the
    // caller falls back to the flat drag plane rather than getting a guess.
    assert_eq!(
        settled(
            &dom,
            &database,
            dragged,
            grabbed_from_above(down_at(5000.0, 0.0))
        ),
        None
    );
}

#[test]
fn the_dragged_part_is_never_the_surface_it_settles_on() {
    let (dom, dragged) = tabletop();
    let database = ReflectionDatabase::embedded();

    // Straight down through the crate itself: the nearest part along this ray
    // *is* the crate. Settling onto it would stack the crate on its own top,
    // a stud higher on every mouse move.
    let cursor = down_at(0.0, 0.0);
    let surface = surface_under(&dom, &database, cursor, dragged).expect("the ground is below");
    assert!(close(surface.point, Vec3::ZERO), "hit {surface:?}");
    assert!(close(surface.normal, Vec3::Y));

    let rested =
        settled(&dom, &database, dragged, grabbed_from_above(cursor)).expect("the ground is below");
    assert!(close(rested, CRATE_CENTRE), "climbed to {rested}");
}

#[test]
fn a_part_already_resting_holds_still_until_the_cursor_moves() {
    let (dom, dragged) = tabletop();
    let database = ReflectionDatabase::embedded();

    // Grabbed by its front face from a raised eye, so the grab ray runs on
    // through the crate at an angle and meets the ground well behind it.
    let grab = Ray::new(Vec3::new(0.0, 3.0, 10.0), Vec3::new(0.0, -2.0, -10.0));
    let settle = Settle {
        cursor: grab,
        grab: Ray::new(Vec3::new(0.0, 1.2, 1.0), grab.direction),
        centre: CRATE_CENTRE,
    };

    let rested = settled(&dom, &database, dragged, settle).expect("the ground is behind");
    assert!(
        close(rested, CRATE_CENTRE),
        "jumped to {rested} on a still cursor"
    );
}

#[test]
fn a_part_grabbed_off_centre_keeps_that_offset_from_the_cursor() {
    let (dom, dragged) = tabletop();
    let database = ReflectionDatabase::embedded();

    // Grabbed half a stud towards the crate's +X/+Z corner.
    let settle = Settle {
        cursor: down_at(10.0, 0.0),
        grab: Ray::new(Vec3::new(0.5, 2.0, 0.5), Vec3::NEG_Y),
        centre: CRATE_CENTRE,
    };

    let rested = settled(&dom, &database, dragged, settle).expect("the ground is below");
    assert!(
        close(rested, Vec3::new(9.5, 1.0, -0.5)),
        "rested at {rested}"
    );
}

#[test]
fn crossing_onto_a_raised_part_jumps_by_its_height_and_nothing_else() {
    let (dom, dragged) = tabletop();
    let database = ReflectionDatabase::embedded();

    // A hair either side of the block's x=18 edge, seen from straight above.
    let before = settled(
        &dom,
        &database,
        dragged,
        grabbed_from_above(down_at(17.9, 0.0)),
    )
    .expect("over the ground");
    let after = settled(
        &dom,
        &database,
        dragged,
        grabbed_from_above(down_at(18.1, 0.0)),
    )
    .expect("over the block");

    let jump = after - before;
    // Up by exactly the block's 4 studs, across by exactly the cursor's own
    // fifth of a stud, and not a hair in z.
    assert!(close(jump, Vec3::new(0.2, 4.0, 0.0)), "jumped by {jump}");
}

#[test]
fn a_floating_part_drops_onto_the_surface_under_the_cursor() {
    let (dom, dragged) = tabletop();
    let database = ReflectionDatabase::embedded();

    // The same crate held three studs in the air when grabbed.
    let settle = Settle {
        cursor: down_at(10.0, 0.0),
        grab: Ray::new(Vec3::new(0.0, 5.0, 0.0), Vec3::NEG_Y),
        centre: Vec3::new(0.0, 4.0, 0.0),
    };

    let rested = settled(&dom, &database, dragged, settle).expect("the ground is below");
    assert!(
        close(rested, Vec3::new(10.0, 1.0, 0.0)),
        "rested at {rested}"
    );
}

#[test]
fn the_side_of_a_part_is_rested_against_rather_than_on() {
    let (dom, dragged) = tabletop();
    let database = ReflectionDatabase::embedded();

    // Looking along +X at the block's x=18 face, two studs up it.
    let cursor = Ray::new(Vec3::new(10.0, 2.0, 0.0), Vec3::X);
    let surface = surface_under(&dom, &database, cursor, dragged).expect("the block's side");
    assert!(close(surface.normal, Vec3::NEG_X), "hit {surface:?}");

    // Grabbed from above, so the grab ray runs along the crate's own +X face
    // plane and the grab point is dropped onto it instead.
    let rested =
        settled(&dom, &database, dragged, grabbed_from_above(cursor)).expect("the block's side");
    // Its +X face flush with the block's -X face at x=18, and the grab point
    // — its top, a stud above its centre — level with where the cursor met
    // the wall, exactly as it stood relative to the cursor when grabbed.
    assert!(
        close(rested, Vec3::new(17.0, 1.0, 0.0)),
        "rested at {rested}"
    );
}

#[test]
fn a_face_hit_reports_the_face_the_ray_enters() {
    let cube = Mat4::from_translation(Vec3::new(0.0, 0.0, -10.0));

    let from_front = Ray::new(Vec3::ZERO, Vec3::NEG_Z);
    let hit = face_hit(from_front, cube).expect("it points at the cube");
    assert!(close(hit.point, Vec3::new(0.0, 0.0, -9.5)));
    assert!(close(hit.normal, Vec3::Z));

    let from_above = Ray::new(Vec3::new(0.2, 5.0, -10.3), Vec3::NEG_Y);
    let hit = face_hit(from_above, cube).expect("it points at the cube");
    assert!(close(hit.normal, Vec3::Y));

    let past = Ray::new(Vec3::new(2.0, 0.0, 0.0), Vec3::NEG_Z);
    assert_eq!(face_hit(past, cube), None);
}

#[test]
fn a_turned_box_answers_with_its_own_face() {
    // A 2-stud cube turned an eighth of a turn about Y presents a corner to
    // -Z; the ray hitting just to one side of that corner enters through a
    // face whose normal is turned the same way.
    let turned =
        Mat4::from_rotation_y(std::f32::consts::FRAC_PI_4) * Mat4::from_scale(Vec3::splat(2.0));
    let ray = Ray::new(Vec3::new(0.5, 0.0, 10.0), Vec3::NEG_Z);
    let hit = face_hit(ray, turned).expect("it points at the cube");
    let expected = Vec3::new(1.0, 0.0, 1.0).normalize();
    assert!(close(hit.normal, expected), "normal {}", hit.normal);
}

#[test]
fn reach_is_half_the_size_along_an_axis_and_the_corner_when_turned() {
    let slab = Mat4::from_scale(Vec3::new(4.0, 1.0, 2.0));
    assert!((reach(slab, Vec3::X) - 2.0).abs() < 1e-5);
    assert!((reach(slab, Vec3::NEG_Y) - 0.5).abs() < 1e-5);

    // Turned an eighth of a turn, a 2-stud cube reaches its corner, √2, along
    // X rather than its face's 1.
    let turned =
        Mat4::from_rotation_y(std::f32::consts::FRAC_PI_4) * Mat4::from_scale(Vec3::splat(2.0));
    assert!((reach(turned, Vec3::X) - std::f32::consts::SQRT_2).abs() < 1e-5);
}

#[test]
fn a_turned_part_rests_on_its_lowest_corner() {
    // A 2-stud cube turned an eighth of a turn about Z hangs a corner √2
    // below its centre, and that corner is what has to meet the surface.
    let turned =
        Mat4::from_rotation_z(std::f32::consts::FRAC_PI_4) * Mat4::from_scale(Vec3::splat(2.0));
    let centre = Vec3::new(0.0, std::f32::consts::SQRT_2, 0.0);
    let settle = Settle {
        cursor: down_at(10.0, 0.0),
        grab: Ray::new(
            Vec3::new(0.0, 2.0 * std::f32::consts::SQRT_2, 0.0),
            Vec3::NEG_Y,
        ),
        centre,
    };
    let ground = Surface {
        point: Vec3::new(10.0, 0.0, 0.0),
        normal: Vec3::Y,
    };

    let rested = rest_on(settle, turned, ground);
    assert!(
        close(rested, Vec3::new(10.0, std::f32::consts::SQRT_2, 0.0)),
        "rested at {rested}"
    );
}
