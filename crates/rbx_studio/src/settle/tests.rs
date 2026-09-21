use glam::{Mat3, Mat4, Vec3};
use rbx_dom::{CFrameData, Variant, Vector3Data, WeakDom};
use rbx_viewer::pick::Meshes;

use super::*;

fn part(dom: &mut WeakDom, parent: Ref, name: &str, position: Vec3, size: Vec3) -> Ref {
    part_of(dom, parent, "Part", name, position, size)
}

fn part_of(
    dom: &mut WeakDom,
    parent: Ref,
    class: &str,
    name: &str,
    position: Vec3,
    size: Vec3,
) -> Ref {
    let part = dom.new_instance(class, name, Some(parent));
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
/// and z=-2..2, a 5 × 1 × 5 plate whose corners are off the world's grid,
/// and a 2-stud crate resting on the ground at the origin — returned as
/// `(dom, crate)`. The crate is what every test drags.
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
    part(
        &mut dom,
        workspace,
        "Plate",
        Vec3::new(-39.7, 0.5, 0.2),
        Vec3::new(5.0, 1.0, 5.0),
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

fn down_at(x: f32, z: f32) -> Ray {
    Ray::new(Vec3::new(x, 50.0, z), Vec3::NEG_Y)
}

/// Straight down from 15 studs over the block's top.
fn overhead() -> Pose {
    Pose {
        position: Vec3::new(20.0, 19.0, 0.0),
        yaw: 0.0,
        pitch: std::f32::consts::FRAC_PI_2,
        fov_degrees: 70.0,
        ortho_scale: 10.0,
    }
}

/// A step with neither grid nor soft snaps, the crate held `grabbed` from its
/// centre.
fn step(cursor: Ray, grabbed: Vec3) -> Settle {
    Settle {
        cursor,
        grabbed,
        grid: 0.0,
        snap_to_parts: false,
        pose: overhead(),
        orthographic: false,
        last: None,
        align: true,
        tilt: Mat3::IDENTITY,
        turning: None,
    }
}

/// The crate held by the middle of its top face.
fn from_above(cursor: Ray) -> Settle {
    step(cursor, Vec3::new(0.0, 1.0, 0.0))
}

fn close(a: Vec3, b: Vec3) -> bool {
    (a - b).length() < 1e-3
}

/// `dragged` as it stands now, which is where a drag of it grabs it.
fn held(dom: &WeakDom, dragged: Ref) -> Vec<(Ref, Mat4)> {
    vec![(dragged, pick::model_of(dom, dragged).expect("a part"))]
}

fn land_on(dom: &WeakDom, dragged: Ref, settle: Settle) -> Option<(Settled, Mat4)> {
    let database = ReflectionDatabase::embedded();
    let held = held(dom, dragged);
    let landed = settled(dom, &database, &Meshes::default(), &held, settle)?;
    let model = landed.carry * held[0].1;
    Some((landed, model))
}

fn rest(dom: &WeakDom, dragged: Ref, settle: Settle) -> Option<Vec3> {
    land_on(dom, dragged, settle).map(|(_, model)| model.w_axis.truncate())
}

#[test]
fn a_drag_over_a_raised_part_settles_on_top_of_it() {
    let (dom, dragged) = tabletop();
    let rested = rest(&dom, dragged, from_above(down_at(20.0, 0.0))).expect("over the block");
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
    // Past the edge of the ground, with nothing landed on yet: the caller
    // falls back to the flat drag plane rather than getting a guess.
    assert_eq!(rest(&dom, dragged, from_above(down_at(5000.0, 0.0))), None);
}

#[test]
fn over_open_space_a_drag_keeps_to_the_plane_it_last_landed_in() {
    let (dom, dragged) = tabletop();
    let (landed, _) =
        land_on(&dom, dragged, from_above(down_at(250.0, 0.0))).expect("over the ground");
    let mut settle = from_above(Ray::new(
        Vec3::new(300.0, 50.0, 0.0),
        Vec3::new(0.0, -1.0, 0.1),
    ));
    settle.last = Some(landed.frame);
    let rested = rest(&dom, dragged, settle).expect("the ground's plane");
    assert!(
        close(rested, Vec3::new(300.0, 1.0, 5.0)),
        "rested at {rested}"
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
    let ground = target_under(&dom, &database, &Meshes::default(), cursor, &[dragged])
        .expect("the ground is below");
    let (distance, _) = ground.raycast(cursor).expect("the ray meets it");
    let hit = cursor.at(distance);
    assert!(close(hit, Vec3::ZERO), "hit {hit}");
    let rested = rest(&dom, dragged, from_above(cursor)).expect("the ground is below");
    assert!(
        close(rested, Vec3::new(0.0, 1.0, 0.0)),
        "climbed to {rested}"
    );
}

#[test]
fn the_grabbed_point_lands_straight_above_where_the_cursor_meets_the_face() {
    let (dom, dragged) = tabletop();
    // Grabbed by its front face from a raised eye, so the cursor's ray runs
    // on through the crate and meets the ground five studs behind it.
    let cursor = Ray::new(Vec3::new(0.0, 3.0, 10.0), Vec3::new(0.0, -2.0, -10.0));
    let rested =
        rest(&dom, dragged, step(cursor, Vec3::new(0.0, 0.2, 1.0))).expect("the ground is behind");
    // Studio drops the grabbed point square onto the face under the cursor:
    // the crate's front face now stands over (0, 0, -5).
    assert!(
        close(rested, Vec3::new(0.0, 1.0, -6.0)),
        "rested at {rested}"
    );
}

#[test]
fn a_part_grabbed_off_centre_keeps_that_offset_from_the_cursor() {
    let (dom, dragged) = tabletop();
    let rested = rest(
        &dom,
        dragged,
        step(down_at(10.0, 0.0), Vec3::new(0.5, 1.0, 0.5)),
    )
    .expect("the ground is below");
    assert!(
        close(rested, Vec3::new(9.5, 1.0, -0.5)),
        "rested at {rested}"
    );
}

#[test]
fn crossing_onto_a_raised_part_jumps_by_its_height_and_nothing_else() {
    let (dom, dragged) = tabletop();
    // A hair either side of the block's x=18 edge, seen from straight above.
    let before = rest(&dom, dragged, from_above(down_at(17.9, 0.0))).expect("over the ground");
    let after = rest(&dom, dragged, from_above(down_at(18.1, 0.0))).expect("over the block");
    let jump = after - before;
    assert!(close(jump, Vec3::new(0.2, 4.0, 0.0)), "jumped by {jump}");
}

#[test]
fn the_side_of_a_part_is_rested_against_rather_than_on() {
    let (dom, dragged) = tabletop();
    // Looking along +X at the block's x=18 face, two studs up it.
    let cursor = Ray::new(Vec3::new(10.0, 2.0, 0.0), Vec3::X);
    let rested = rest(&dom, dragged, from_above(cursor)).expect("the block's side");
    // Its +X face flush with the block's -X face at x=18, and the grabbed
    // point — its top, a stud above its centre — level with where the cursor
    // met the wall.
    assert!(
        close(rested, Vec3::new(17.0, 1.0, 0.0)),
        "rested at {rested}"
    );
}

#[test]
fn a_snapped_drag_lands_on_the_grid_of_the_faces_nearest_corner() {
    let (dom, dragged) = tabletop();
    // Over the plate, whose nearest corner to the cursor is (-37.2, 1, 2.7):
    // 1.7 and 1.8 in from it round to 2 and 2, which is (-39.2, 1, 0.7) —
    // not the world grid's (-39, 1, 1).
    let mut settle = from_above(down_at(-38.9, 0.9));
    settle.grid = 1.0;
    let rested = rest(&dom, dragged, settle).expect("over the plate");
    assert!(
        close(rested, Vec3::new(-39.2, 2.0, 0.7)),
        "rested at {rested}"
    );
}

#[test]
fn snap_to_parts_pulls_the_crate_flush_with_the_edge_it_is_near() {
    let (dom, dragged) = tabletop();
    // Over the block's top, the crate's side a tenth of a stud past its
    // x = 18 edge and nothing else within reach (0.28 studs from 15 up).
    let mut settle = from_above(down_at(18.9, 0.5));
    settle.snap_to_parts = true;
    let (landed, model) = land_on(&dom, dragged, settle).expect("over the block");
    let centre = model.w_axis.truncate();
    assert!(
        close(centre, Vec3::new(19.0, 5.0, 0.5)),
        "rested at {centre}"
    );
    assert_eq!(landed.landing.aligned.len(), 1);
}

/// An 8-wide wedge 2 high and 4 deep at the origin, its slope rising 1 in 2
/// towards +Z, and a 4 × 1 × 2 plank off to the side — `(dom, plank)`.
fn ramp() -> (WeakDom, Ref) {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let wedge = Vec3::new(8.0, 2.0, 4.0);
    part_of(&mut dom, workspace, "WedgePart", "Ramp", Vec3::Y, wedge);
    let size = Vec3::new(4.0, 1.0, 2.0);
    let plank = part(
        &mut dom,
        workspace,
        "Plank",
        Vec3::new(30.0, 0.5, 0.0),
        size,
    );
    (dom, plank)
}

/// The ramp's slope, facing up and towards -Z.
fn slope_normal() -> Vec3 {
    Vec3::new(0.0, 2.0, -1.0).normalize()
}

#[test]
fn a_part_dragged_onto_a_slope_lies_flat_on_it() {
    let (dom, plank) = ramp();
    // Onto the slope's middle line, held by the middle of its top.
    let (_, model) = land_on(&dom, plank, from_above(down_at(1.0, 0.0))).expect("the ramp");
    let up = model.y_axis.truncate().normalize();
    assert!(close(up, slope_normal()), "up is {up}");
    assert!(close(model.x_axis.truncate().normalize(), Vec3::X));
    // Its underside on the slope: its centre half its height off it.
    let centre = model.w_axis.truncate();
    assert!(
        close(centre, Vec3::new(1.0, 1.0, 0.0) + slope_normal() * 0.5),
        "rested at {centre}"
    );
}

#[test]
fn holding_orientation_keeps_the_part_as_it_was_grabbed() {
    let (dom, plank) = ramp();
    let mut settle = from_above(down_at(1.0, 0.0));
    settle.align = false;
    let (_, model) = land_on(&dom, plank, settle).expect("the ramp");
    assert!(close(model.y_axis.truncate(), Vec3::Y));
    assert!(close(model.x_axis.truncate(), Vec3::X * 4.0));
}

#[test]
fn r_spins_the_part_a_quarter_about_the_slopes_normal() {
    let (dom, plank) = ramp();
    let (landed, _) = land_on(&dom, plank, from_above(down_at(1.0, 0.0))).expect("the ramp");
    let normal = landed.frame.y;
    let mut settle = from_above(down_at(1.0, 0.0));
    settle.tilt = tilt::turned(&landed.frame, Mat3::IDENTITY, Mat3::IDENTITY, true, normal);
    let (_, model) = land_on(&dom, plank, settle).expect("the ramp");
    let (along, up) = (
        model.x_axis.truncate().normalize(),
        model.y_axis.truncate().normalize(),
    );
    assert!(close(up, slope_normal()), "up is {up}");
    // Its length now runs up the slope, not across it.
    assert!(along.dot(Vec3::X).abs() < 1e-4, "along {along}");
    assert!(close(along.cross(normal).cross(normal), -along));
}

#[test]
fn a_turn_half_eased_in_stands_half_way_round() {
    let (dom, plank) = ramp();
    let mut settle = from_above(down_at(40.0, 40.0));
    settle.last =
        land_on(&dom, plank, from_above(down_at(1.0, 0.0))).map(|(landed, _)| landed.frame);
    let quarter = Mat3::from_rotation_y(std::f32::consts::FRAC_PI_2);
    settle.tilt = quarter;
    settle.turning = Some((Mat3::IDENTITY, 0.5));
    let (_, model) = land_on(&dom, plank, settle).expect("the slope's plane");
    let along = model.x_axis.truncate().normalize();
    // An eighth of a turn about the slope's normal from lying across it.
    assert!(
        (along.dot(Vec3::X) - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-4,
        "{along}"
    );
    assert!(along.dot(slope_normal()).abs() < 1e-4);
}
