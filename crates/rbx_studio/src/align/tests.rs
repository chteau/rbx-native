use std::f32::consts::FRAC_PI_2;

use glam::{Mat3, Mat4, Vec3};
use rbx_dom::Ref;

use super::*;

/// A box part standing square to the world at `position`, `size` studs a
/// side — same shape as `transform::tests::part_at`, duplicated here rather
/// than shared: a test fixture two modules deep in `pub(crate)` internals is
/// not worth a shared helper over.
fn part_at(referent: u32, position: Vec3, size: Vec3) -> Target {
    Target {
        referent: Ref::new(referent),
        model: Mat4::from_translation(position) * Mat4::from_scale(size),
    }
}

/// A part turned a quarter turn about world Y, so its own local X sits on
/// world -Z and its local Z sits on world X — mirrors
/// `transform::tests::block`'s own orientation, which asserts exactly that.
fn turned_part_at(referent: u32, position: Vec3, size: Vec3) -> Target {
    let orientation = Mat3::from_rotation_y(FRAC_PI_2);
    Target {
        referent: Ref::new(referent),
        model: Mat4::from_cols(
            (orientation.x_axis * size.x).extend(0.0),
            (orientation.y_axis * size.y).extend(0.0),
            (orientation.z_axis * size.z).extend(0.0),
            position.extend(1.0),
        ),
    }
}

fn options(axis: Axis, mode: Mode, space: Space, relative_to: RelativeTo) -> Options {
    let mut options = Options {
        axes: [false; 3],
        mode,
        space,
        relative_to,
    };
    options.toggle_axis(axis);
    options
}

fn moved_x(moves: &[(Ref, Vec3)], referent: u32) -> f32 {
    moves
        .iter()
        .find(|(r, _)| *r == Ref::new(referent))
        .unwrap_or_else(|| panic!("part {referent} was not moved"))
        .1
        .x
}

#[test]
fn centering_aligns_every_objects_own_centre_on_the_selections_collective_centre() {
    // A: x spans -1..1. B: x spans 8..12. Collective centre is (−1+12)/2 = 5.5.
    let entries = vec![
        vec![part_at(1, Vec3::new(0.0, 0.0, 0.0), Vec3::splat(2.0))],
        vec![part_at(2, Vec3::new(10.0, 0.0, 0.0), Vec3::splat(4.0))],
    ];
    let moves = plan(
        &entries,
        0,
        options(
            Axis::X,
            Mode::Center,
            Space::World,
            RelativeTo::SelectionBounds,
        ),
    );

    assert!((moved_x(&moves, 1) - 5.5).abs() < 1e-5);
    assert!((moved_x(&moves, 2) - 5.5).abs() < 1e-5);
}

#[test]
fn min_aligns_every_objects_near_face_on_the_selections_lowest_face() {
    let entries = vec![
        vec![part_at(1, Vec3::new(0.0, 0.0, 0.0), Vec3::splat(2.0))], // spans -1..1
        vec![part_at(2, Vec3::new(10.0, 0.0, 0.0), Vec3::splat(4.0))], // spans 8..12
    ];
    let moves = plan(
        &entries,
        0,
        options(
            Axis::X,
            Mode::Min,
            Space::World,
            RelativeTo::SelectionBounds,
        ),
    );

    // The selection's lowest face is already at x = -1 (part 1's own min).
    assert!(
        (moved_x(&moves, 1) - 0.0).abs() < 1e-5,
        "already at the min"
    );
    // Part 2's min (8) has to reach -1: its centre (10) moves by -9.
    assert!((moved_x(&moves, 2) - 1.0).abs() < 1e-5);
}

#[test]
fn max_aligns_every_objects_far_face_on_the_selections_highest_face() {
    let entries = vec![
        vec![part_at(1, Vec3::new(0.0, 0.0, 0.0), Vec3::splat(2.0))], // spans -1..1
        vec![part_at(2, Vec3::new(10.0, 0.0, 0.0), Vec3::splat(4.0))], // spans 8..12
    ];
    let moves = plan(
        &entries,
        0,
        options(
            Axis::X,
            Mode::Max,
            Space::World,
            RelativeTo::SelectionBounds,
        ),
    );

    // Part 1's max (1) has to reach the selection's highest face (12).
    assert!((moved_x(&moves, 1) - 11.0).abs() < 1e-5);
    // Part 2 already carries the selection's own highest face.
    assert!(
        (moved_x(&moves, 2) - 10.0).abs() < 1e-5,
        "already at the max"
    );
}

#[test]
fn active_object_gives_a_different_answer_than_selection_bounds_and_never_moves_itself() {
    let entries = vec![
        vec![part_at(1, Vec3::new(0.0, 0.0, 0.0), Vec3::splat(2.0))],
        vec![part_at(2, Vec3::new(10.0, 0.0, 0.0), Vec3::splat(4.0))],
    ];
    // Active object is index 1 (part 2), whose own centre-X is 10 — not the
    // 5.5 `centering_aligns_...` found relative to the collective bounds.
    let moves = plan(
        &entries,
        1,
        options(
            Axis::X,
            Mode::Center,
            Space::World,
            RelativeTo::ActiveObject,
        ),
    );

    assert_eq!(
        moves.len(),
        1,
        "the active object itself is not in the plan"
    );
    assert!((moved_x(&moves, 1) - 10.0).abs() < 1e-5);
}

#[test]
fn local_space_moves_along_the_reference_objects_own_turned_axis_not_the_world_one() {
    // The active object is turned a quarter turn about Y: its own local X
    // sits on world -Z (see `turned_part_at`), so aligning "Local X" moves
    // the other part along world Z, not world X.
    let entries = vec![
        vec![part_at(1, Vec3::new(0.0, 0.0, 5.0), Vec3::splat(2.0))],
        vec![turned_part_at(2, Vec3::ZERO, Vec3::new(4.0, 1.0, 2.0))],
    ];

    let world_moves = plan(
        &entries,
        1,
        options(
            Axis::X,
            Mode::Center,
            Space::World,
            RelativeTo::ActiveObject,
        ),
    );
    // Both objects already stand at world x = 0: World/X is a no-op here.
    assert_eq!(world_moves.len(), 1);
    assert!((world_moves[0].1 - Vec3::new(0.0, 0.0, 5.0)).length() < 1e-5);

    let local_moves = plan(
        &entries,
        1,
        options(
            Axis::X,
            Mode::Center,
            Space::Local,
            RelativeTo::ActiveObject,
        ),
    );
    // Local/X moves part 1 along world -Z until its centre matches the
    // active object's own centre along that same direction (world origin),
    // landing it at the origin rather than leaving it where World/X did.
    assert_eq!(local_moves.len(), 1);
    assert!((local_moves[0].1 - Vec3::ZERO).length() < 1e-5);
}

#[test]
fn a_single_object_selection_is_a_no_op() {
    let entries = vec![vec![part_at(1, Vec3::new(3.0, 4.0, 5.0), Vec3::splat(2.0))]];

    let moves = plan(
        &entries,
        0,
        options(
            Axis::X,
            Mode::Center,
            Space::World,
            RelativeTo::SelectionBounds,
        ),
    );

    assert!(moves.is_empty());
}

#[test]
fn an_empty_selection_is_a_no_op() {
    let moves: Vec<(Ref, Vec3)> = plan(
        &[],
        0,
        options(
            Axis::X,
            Mode::Center,
            Space::World,
            RelativeTo::SelectionBounds,
        ),
    );

    assert!(moves.is_empty());
}

#[test]
fn a_model_with_nothing_drawable_beneath_it_contributes_nothing_to_the_bound() {
    // The empty entry still occupies its own index (part 2 is the active
    // object, at index 1) so it must not shift who "active" refers to.
    let entries: Vec<Vec<Target>> = vec![
        vec![part_at(1, Vec3::new(0.0, 0.0, 0.0), Vec3::splat(2.0))],
        vec![part_at(2, Vec3::new(10.0, 0.0, 0.0), Vec3::splat(4.0))],
        Vec::new(),
    ];

    let moves = plan(
        &entries,
        1,
        options(
            Axis::X,
            Mode::Center,
            Space::World,
            RelativeTo::ActiveObject,
        ),
    );

    assert_eq!(
        moves.len(),
        1,
        "only part 1 moves; the empty entry has nothing to move"
    );
    assert!((moved_x(&moves, 1) - 10.0).abs() < 1e-5);
}

#[test]
fn every_toggled_axis_moves_independently_of_the_others() {
    let entries = vec![
        vec![part_at(1, Vec3::new(0.0, 0.0, 0.0), Vec3::splat(2.0))],
        vec![part_at(2, Vec3::new(10.0, 20.0, 0.0), Vec3::splat(2.0))],
    ];
    let mut options = options(
        Axis::X,
        Mode::Center,
        Space::World,
        RelativeTo::SelectionBounds,
    );
    options.toggle_axis(Axis::Y);

    let moves = plan(&entries, 0, options);

    let part_one = moves.iter().find(|(r, _)| *r == Ref::new(1)).unwrap().1;
    // Collective bounds: x centre (0+10)/2 = 5, y centre (0+20)/2 = 10, z untouched.
    assert!((part_one - Vec3::new(5.0, 10.0, 0.0)).length() < 1e-5);
}

/// The Align toolbar's **Selection Bounds** and the box drawn around a
/// selected `Model` have to be the same extent, or "align to the selection"
/// would move things to somewhere other than the edge the user can see.
///
/// They are not one function: this module measures along an arbitrary
/// direction, which `Space::Local` needs and `gizmo::bounds_of` cannot
/// express, and it measures each entry separately rather than unioning the
/// lot. On the world axes the two reduce to the same support function, which
/// is what this pins down — including for a part turned off those axes,
/// where a wrong reduction would show up first.
#[test]
fn selection_bounds_agree_with_the_box_drawn_around_the_selection() {
    let entries = [
        vec![part_at(
            1,
            Vec3::new(-4.0, 1.0, 0.0),
            Vec3::new(2.0, 6.0, 2.0),
        )],
        vec![turned_part_at(
            2,
            Vec3::new(9.0, 0.0, 3.0),
            Vec3::new(8.0, 2.0, 4.0),
        )],
    ];
    let flat: Vec<&Target> = entries.iter().flatten().collect();
    let (min, max) =
        rbx_viewer::gizmo::bounds_of(flat.iter().map(|target| target.model)).expect("two parts");

    for (axis, index) in [(Axis::X, 0), (Axis::Y, 1), (Axis::Z, 2)] {
        let direction = axis.world_direction();
        let along = |mode| bound_along(flat.iter().copied(), direction, mode).expect("two parts");
        assert!((along(Mode::Min) - min[index]).abs() < 1e-5, "{axis:?} min");
        assert!((along(Mode::Max) - max[index]).abs() < 1e-5, "{axis:?} max");
        assert!(
            (along(Mode::Center) - (min[index] + max[index]) * 0.5).abs() < 1e-5,
            "{axis:?} centre"
        );
    }
}
