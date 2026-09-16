use glam::{Mat3, Mat4, Quat, Vec3};
use rbx_dom::Ref;
use rbx_viewer::Pose;

use super::*;

/// Looking down -Z from ten studs out, at a point on the z=0 plane.
fn looking_at(x: f32, y: f32) -> Ray {
    Ray::new(Vec3::new(x, y, 10.0), Vec3::NEG_Z)
}

/// No grid and nothing to soft-snap onto: a drag that lands exactly where the
/// cursor puts it.
fn free() -> Landing<'static> {
    Landing {
        grid: 0.0,
        angle: 0.0,
        neighbours: &[],
        reach: 0.0,
    }
}

/// A rotate increment of `degrees` and no stud grid.
fn degrees(degrees: f32) -> Landing<'static> {
    Landing {
        angle: degrees.to_radians(),
        ..free()
    }
}

/// A grid of `increment` studs and nothing to soft-snap onto.
fn grid(increment: f32) -> Landing<'static> {
    Landing {
        grid: increment,
        ..free()
    }
}

#[test]
fn an_axis_drag_slides_by_how_far_the_cursor_moved_along_it() {
    let drag = Drag::Axis {
        origin: Vec3::ZERO,
        axis: Vec3::X,
        grabbed: 2.0,
    };

    // Grabbed at 2 studs along X, now pointing at 6.5: the part has travelled
    // 4.5, and not at all on the other two axes.
    let moved = moved_to(drag, looking_at(6.5, 0.0), free()).expect("the axis is across the view");
    assert!((moved - Vec3::new(4.5, 0.0, 0.0)).length() < 1e-4);

    // Back past where it started.
    let moved = moved_to(drag, looking_at(-1.0, 0.0), free()).expect("the axis is across the view");
    assert!((moved - Vec3::new(-3.0, 0.0, 0.0)).length() < 1e-4);
}

#[test]
fn an_axis_drag_holds_still_at_the_point_it_was_grabbed() {
    let drag = Drag::Axis {
        origin: Vec3::new(4.0, 1.0, -2.0),
        axis: Vec3::Y,
        grabbed: 3.0,
    };
    // Three studs up the part's own Y axis, which starts at y = 1.
    let ray = Ray::new(Vec3::new(4.0, 4.0, 10.0), Vec3::NEG_Z);

    // The cursor has not moved since the grab, so neither has the part.
    let moved = moved_to(drag, ray, free()).expect("the axis is across the view");
    assert!((moved - Vec3::new(4.0, 1.0, -2.0)).length() < 1e-4);
}

#[test]
fn an_axis_sighted_end_on_has_no_answer_rather_than_a_wild_one() {
    let drag = Drag::Axis {
        origin: Vec3::ZERO,
        axis: Vec3::Z,
        grabbed: 0.0,
    };
    assert_eq!(moved_to(drag, looking_at(0.0, 0.0), free()), None);
}

#[test]
fn a_plane_drag_keeps_the_part_where_it_was_under_the_cursor() {
    // Grabbed at the origin on a plane facing the camera, with the part's own
    // centre a stud up and to the right of the grab point.
    let drag = Drag::Plane {
        point: Vec3::ZERO,
        normal: Vec3::Z,
        offset: Vec3::new(1.0, 1.0, 0.0),
    };

    let moved = moved_to(drag, looking_at(3.0, 4.0), free()).expect("the ray crosses the plane");
    assert!((moved - Vec3::new(4.0, 5.0, 0.0)).length() < 1e-4);
}

#[test]
fn a_plane_drag_stays_on_the_plane_it_started_on() {
    // The plane is world-fixed, so a ray arriving from somewhere else still
    // lands on it rather than dragging the part towards the new eye.
    let drag = Drag::Plane {
        point: Vec3::new(0.0, 0.0, -5.0),
        normal: Vec3::Z,
        offset: Vec3::ZERO,
    };

    let from_the_side = Ray::new(Vec3::new(-20.0, 0.0, 15.0), Vec3::new(1.0, 0.0, -1.0));
    let moved = moved_to(drag, from_the_side, free()).expect("the ray crosses the plane");
    assert!((moved.z + 5.0).abs() < 1e-4, "left the plane at {moved}");
}

#[test]
fn a_ray_that_has_turned_along_the_drag_plane_has_no_answer() {
    let drag = Drag::Plane {
        point: Vec3::ZERO,
        normal: Vec3::Z,
        offset: Vec3::ZERO,
    };
    let along = Ray::new(Vec3::new(0.0, 0.0, 0.0), Vec3::X);

    assert_eq!(moved_to(drag, along, free()), None);
}

#[test]
fn a_snapped_axis_drag_lands_on_whole_increments_of_travel() {
    let drag = Drag::Axis {
        origin: Vec3::ZERO,
        axis: Vec3::X,
        grabbed: 2.0,
    };

    // 4.4 studs of travel rounds down to 4, 4.6 rounds up to 5.
    let moved = moved_to(drag, looking_at(6.4, 0.0), grid(1.0)).expect("the axis is across");
    assert!(
        (moved - Vec3::new(4.0, 0.0, 0.0)).length() < 1e-4,
        "{moved}"
    );
    let moved = moved_to(drag, looking_at(6.6, 0.0), grid(1.0)).expect("the axis is across");
    assert!(
        (moved - Vec3::new(5.0, 0.0, 0.0)).length() < 1e-4,
        "{moved}"
    );
}

#[test]
fn a_snapped_drag_rounds_the_travel_not_the_world_position() {
    // A part that already stood off-grid must not jump onto the grid the
    // moment it is picked up: zero travel is zero travel, snapped or not.
    let drag = Drag::Axis {
        origin: Vec3::new(0.3, 0.0, 0.0),
        axis: Vec3::X,
        grabbed: 0.3,
    };
    let moved = moved_to(drag, looking_at(0.3, 0.0), grid(1.0)).expect("the axis is across");
    assert!(
        (moved - Vec3::new(0.3, 0.0, 0.0)).length() < 1e-4,
        "{moved}"
    );

    // And a whole increment of travel keeps the same fractional offset.
    let moved = moved_to(drag, looking_at(1.4, 0.0), grid(1.0)).expect("the axis is across");
    assert!(
        (moved - Vec3::new(1.3, 0.0, 0.0)).length() < 1e-4,
        "{moved}"
    );
}

#[test]
fn a_snapped_free_drag_rounds_every_axis_of_its_travel() {
    let drag = Drag::Plane {
        point: Vec3::ZERO,
        normal: Vec3::Z,
        offset: Vec3::ZERO,
    };
    let moved = moved_to(drag, looking_at(3.4, 4.6), grid(1.0)).expect("the ray crosses");
    assert!(
        (moved - Vec3::new(3.0, 5.0, 0.0)).length() < 1e-4,
        "{moved}"
    );
}

#[test]
fn a_free_drag_soft_snaps_its_grab_point_onto_a_nearby_surface() {
    // A part filling y ∈ [-1, 1]: the cursor puts the grab point at 1.2, just
    // above its top face, and it settles onto it.
    let neighbour =
        Mat4::from_scale_rotation_translation(Vec3::splat(2.0), Quat::IDENTITY, Vec3::ZERO);
    let drag = Drag::Plane {
        point: Vec3::new(0.0, 5.0, 0.0),
        normal: Vec3::Z,
        offset: Vec3::ZERO,
    };
    let landing = Landing {
        neighbours: &[neighbour],
        reach: 0.5,
        ..free()
    };

    let moved = moved_to(drag, looking_at(0.0, 1.2), landing).expect("the ray crosses");
    assert!(
        (moved - Vec3::new(0.0, 1.0, 0.0)).length() < 1e-4,
        "{moved}"
    );
}

#[test]
fn a_free_drag_out_of_reach_of_everything_lands_where_the_cursor_is() {
    let neighbour =
        Mat4::from_scale_rotation_translation(Vec3::splat(2.0), Quat::IDENTITY, Vec3::ZERO);
    let drag = Drag::Plane {
        point: Vec3::new(0.0, 5.0, 0.0),
        normal: Vec3::Z,
        offset: Vec3::ZERO,
    };
    let landing = Landing {
        neighbours: &[neighbour],
        reach: 0.5,
        ..free()
    };

    let moved = moved_to(drag, looking_at(0.0, 4.0), landing).expect("the ray crosses");
    assert!(
        (moved - Vec3::new(0.0, 4.0, 0.0)).length() < 1e-4,
        "{moved}"
    );
}

#[test]
fn a_grid_in_force_takes_the_place_of_soft_snapping_rather_than_stacking_on_it() {
    // creator-docs gives the two as alternatives — soft snapping is what a
    // cursor drag does "if snapping is disabled" — so a surface well within
    // reach must not pull a snapped drag off its increment.
    let neighbour =
        Mat4::from_scale_rotation_translation(Vec3::splat(2.0), Quat::IDENTITY, Vec3::ZERO);
    let drag = Drag::Plane {
        point: Vec3::ZERO,
        normal: Vec3::Z,
        offset: Vec3::ZERO,
    };
    let landing = Landing {
        grid: 1.0,
        neighbours: &[neighbour],
        reach: 5.0,
        ..free()
    };

    let moved = moved_to(drag, looking_at(0.0, 1.2), landing).expect("the ray crosses");
    assert!(
        (moved - Vec3::new(0.0, 1.0, 0.0)).length() < 1e-4,
        "{moved}"
    );
}

#[test]
fn a_plane_drag_asks_to_settle_along_the_ray_that_grabbed_it() {
    // Grabbed looking down -Z at a point on the part, with the centre a stud
    // behind and above it.
    let drag = Drag::Plane {
        point: Vec3::new(2.0, 3.0, 4.0),
        normal: Vec3::Z,
        offset: Vec3::new(0.0, 1.0, -1.0),
    };
    let cursor = looking_at(5.0, 6.0);

    let settle = drag.settle(cursor).expect("a cursor drag settles");
    assert_eq!(settle.cursor, cursor);
    // The grab ray starts where the grab landed and runs into the part, the
    // way the click did — the reverse of the plane's normal.
    assert_eq!(settle.grab.origin, Vec3::new(2.0, 3.0, 4.0));
    assert!((settle.grab.direction - Vec3::NEG_Z).length() < 1e-6);
    assert_eq!(settle.centre, Vec3::new(2.0, 4.0, 3.0));
}

#[test]
fn an_axis_drag_never_settles() {
    let drag = Drag::Axis {
        origin: Vec3::ZERO,
        axis: Vec3::X,
        grabbed: 2.0,
    };
    assert_eq!(drag.settle(looking_at(6.5, 0.0)), None);
}

/// Where one step of a drag puts the part's centre, for the gestures that only
/// move it.
fn moved_to(drag: Drag, ray: Ray, landing: Landing) -> Option<Vec3> {
    match advance(drag, ray, landing)?.1 {
        Change::Position(position) => Some(position),
        other => panic!("expected a move, got {other:?}"),
    }
}

/// A two-by-one-by-four block at the world origin, square to the world.
fn block() -> Target {
    Target {
        referent: Ref::new(1),
        model: Mat4::from_scale(Vec3::new(2.0, 1.0, 4.0)),
    }
}

/// The camera the Scale balls are sized for: ten studs down +Z looking at the
/// world origin, which is where [`looking_at`] fires its rays from too.
fn eye() -> Pose {
    Pose {
        position: Vec3::new(0.0, 0.0, 10.0),
        yaw: 0.0,
        pitch: 0.0,
        fov_degrees: 70.0,
        ortho_scale: 25.0,
    }
}

/// The Scale handles standing on one target's own faces, seen from [`eye`].
fn faces(target: Target) -> Faces {
    Faces::new(target.model, eye(), false)
}

/// The Scale drag that grabbing the block's +X face opens, with the cursor
/// standing exactly on that face (one stud out, half of the 2-stud width).
fn grabbed_x_face() -> Drag {
    Drag::Size {
        origin: Vec3::ZERO,
        axis: Vec3::X,
        grabbed: 1.0,
        size: Vec3::new(2.0, 1.0, 4.0),
        component: 0,
    }
}

#[test]
fn grabbing_a_ball_opens_a_resize_of_the_face_it_stands_on() {
    // The block is 2 studs wide, so its +X ball stands at x = 1, and that is
    // where the drag is grabbed.
    let target = block();
    let drag = grab_face(&faces(target), target, looking_at(1.0, 0.0)).expect("the +X ball");

    assert_eq!(drag, grabbed_x_face());
}

#[test]
fn grabbing_the_ball_on_the_far_face_resizes_the_part_the_other_way() {
    let target = block();
    let drag = grab_face(&faces(target), target, looking_at(-1.0, 0.0)).expect("the -X ball");

    let Drag::Size {
        axis, component, ..
    } = drag
    else {
        panic!("expected a resize, got {drag:?}");
    };
    // Pointing out through the grabbed face, so pulling away from the part
    // grows it whichever end was taken hold of.
    assert!((axis - Vec3::NEG_X).length() < 1e-4, "{axis:?}");
    assert_eq!(component, 0);
}

/// What binding the handles to the part's own faces buys: the component a ball
/// resizes is the one whose face it stands on, not the world axis it happens
/// to lie nearest.
#[test]
fn a_turned_parts_ball_resizes_the_face_it_actually_sits_on() {
    // A quarter turn about Z carries the part's own X onto world +Y, so its
    // own X faces — one stud out either way of a 2-stud width — now stand a
    // stud above and below the part, where nothing of the world's own X is.
    let target = Target {
        referent: Ref::new(1),
        model: Mat4::from_scale_rotation_translation(
            Vec3::new(2.0, 1.0, 4.0),
            Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
            Vec3::ZERO,
        ),
    };
    let drag = grab_face(&faces(target), target, looking_at(0.0, 1.0)).expect("the +X ball");

    let Drag::Size {
        axis,
        component,
        size,
        ..
    } = drag
    else {
        panic!("expected a resize, got {drag:?}");
    };
    // The part's own X, pointing out through the face that was grabbed — a
    // resize along world Y that nonetheless writes `Size.X`.
    assert!((axis - Vec3::Y).length() < 1e-4, "{axis:?}");
    assert_eq!(component, 0);
    assert!((size - Vec3::new(2.0, 1.0, 4.0)).length() < 1e-4);
}

#[test]
fn pointing_at_no_ball_grabs_no_resize() {
    // Inside the part but well away from the middle of any of its faces,
    // which is what leaves a click there free to fall through to a pick.
    let target = Target {
        referent: Ref::new(1),
        model: Mat4::from_scale(Vec3::splat(40.0)),
    };
    assert_eq!(
        grab_face(&faces(target), target, looking_at(8.0, 8.0)),
        None
    );
}

/// Both halves together: grab the ball where it is drawn, pull the cursor out,
/// and the part grows by exactly the travel with its far face left standing.
#[test]
fn a_ball_grabbed_where_it_is_drawn_resizes_from_there() {
    let target = block();
    let drag = grab_face(&faces(target), target, looking_at(1.0, 0.0)).expect("the +X ball");

    let (_, change) = advance(drag, looking_at(4.0, 0.0), free()).expect("the drag has an answer");
    assert_eq!(
        change,
        Change::Size {
            size: Vec3::new(5.0, 1.0, 4.0),
            position: Vec3::new(1.5, 0.0, 0.0),
        }
    );
}

#[test]
fn a_scale_drag_grows_the_part_by_how_far_the_face_was_pulled() {
    // Pulled out to 4 studs from the centre: the +X face moved 3, and the -X
    // face has to stay where it was, so the part is 3 studs wider and its
    // middle has shifted by half of that.
    let change = advance(grabbed_x_face(), looking_at(4.0, 0.0), free())
        .expect("the axis is across the view")
        .1;

    assert_eq!(
        change,
        Change::Size {
            size: Vec3::new(5.0, 1.0, 4.0),
            position: Vec3::new(1.5, 0.0, 0.0),
        }
    );
    // Which is to say the untouched face has not moved: it stood at -1 and
    // still does.
    let Change::Size { size, position } = change else {
        unreachable!()
    };
    assert!((position.x - size.x * 0.5 + 1.0).abs() < 1e-4);
}

#[test]
fn a_scale_drag_pushed_inwards_shrinks_the_part() {
    let change = advance(grabbed_x_face(), looking_at(0.25, 0.0), free())
        .expect("the axis is across the view")
        .1;

    assert_eq!(
        change,
        Change::Size {
            size: Vec3::new(1.25, 1.0, 4.0),
            position: Vec3::new(-0.375, 0.0, 0.0),
        }
    );
}

#[test]
fn a_scale_drag_touches_only_the_axis_it_was_grabbed_on() {
    let Change::Size { size, position } = advance(grabbed_x_face(), looking_at(9.0, 0.0), free())
        .expect("the axis is across the view")
        .1
    else {
        unreachable!()
    };
    assert_eq!((size.y, size.z), (1.0, 4.0));
    assert_eq!((position.y, position.z), (0.0, 0.0));
}

#[test]
fn a_scale_drag_run_past_the_smallest_a_part_may_be_stops_there() {
    // Dragged far through the part and out the other side. `BasePart.Size`
    // bottoms out at 0.001, and a part that has stopped shrinking must stop
    // sliding too, or it would walk away under a cursor doing nothing.
    let far = advance(grabbed_x_face(), looking_at(-50.0, 0.0), free())
        .expect("the axis is across the view")
        .1;
    let further = advance(grabbed_x_face(), looking_at(-500.0, 0.0), free())
        .expect("the axis is across the view")
        .1;

    let Change::Size { size, .. } = far else {
        unreachable!()
    };
    assert!((size.x - MIN_SIZE).abs() < 1e-6, "clamped to {}", size.x);
    assert_eq!(far, further);
}

#[test]
fn a_scale_handle_on_the_far_face_grows_the_part_the_other_way() {
    // Grabbed on the -X face, so the axis points out through it: dragging the
    // cursor to -4 is 3 studs of growth, with the +X face left alone.
    let drag = Drag::Size {
        origin: Vec3::ZERO,
        axis: Vec3::NEG_X,
        grabbed: 1.0,
        size: Vec3::new(2.0, 1.0, 4.0),
        component: 0,
    };

    assert_eq!(
        advance(drag, looking_at(-4.0, 0.0), free())
            .expect("the axis is across the view")
            .1,
        Change::Size {
            size: Vec3::new(5.0, 1.0, 4.0),
            position: Vec3::new(-1.5, 0.0, 0.0),
        }
    );
}

#[test]
fn a_rotate_drag_turns_by_the_angle_the_cursor_swept() {
    // The Z ring, seen face on from +Z. Grabbed at its zero, which stands on
    // world X; the cursor moves a quarter turn round to world Y.
    let drag = Drag::Ring {
        origin: Vec3::ZERO,
        frame: (Vec3::Z, Vec3::X, Vec3::Y),
        orientation: Mat3::IDENTITY,
        last: 0.0,
        turned: 0.0,
    };

    let (_, change) =
        advance(drag, looking_at(0.0, 5.0), free()).expect("the ray crosses the ring");
    let Change::Orientation(turned) = change else {
        panic!("expected a rotation, got {change:?}");
    };
    // A quarter turn about Z carries world X onto world Y.
    assert!((turned.x_axis - Vec3::Y).length() < 1e-4);
    assert!((turned.z_axis - Vec3::Z).length() < 1e-4);
}

#[test]
fn a_rotate_drag_that_has_not_moved_leaves_the_part_alone() {
    let drag = Drag::Ring {
        origin: Vec3::ZERO,
        frame: (Vec3::Z, Vec3::X, Vec3::Y),
        orientation: Mat3::IDENTITY,
        last: 0.0,
        turned: 0.0,
    };

    let (_, change) =
        advance(drag, looking_at(5.0, 0.0), free()).expect("the ray crosses the ring");
    let Change::Orientation(turned) = change else {
        unreachable!()
    };
    assert!((turned - Mat3::IDENTITY).abs_diff_eq(Mat3::ZERO, 1e-4));
}

/// The reason a ring drag carries its own running total rather than measuring
/// against where it was grabbed: an angle only exists up to a full turn, so a
/// drag past half a turn would otherwise snap back the other way.
#[test]
fn a_rotate_drag_can_be_carried_past_half_a_turn() {
    let mut drag = Drag::Ring {
        origin: Vec3::ZERO,
        frame: (Vec3::Z, Vec3::X, Vec3::Y),
        orientation: Mat3::IDENTITY,
        last: 0.0,
        turned: 0.0,
    };

    // Walked round in eighths, well past the ±π seam and most of the way home.
    let mut turned = 0.0;
    for eighth in 1..=7 {
        let angle = std::f32::consts::TAU * eighth as f32 / 8.0;
        let ray = looking_at(5.0 * angle.cos(), 5.0 * angle.sin());
        let (next, _) = advance(drag, ray, free()).expect("the ray crosses the ring");
        drag = next;
        let Drag::Ring { turned: total, .. } = drag else {
            unreachable!()
        };
        assert!(total > turned, "the drag turned back at {total}");
        turned = total;
    }

    // Seven eighths forwards, not one eighth backwards.
    let expected = std::f32::consts::TAU * 7.0 / 8.0;
    assert!((turned - expected).abs() < 1e-3, "turned {turned}");
}

#[test]
fn a_ray_that_has_turned_along_a_ring_has_no_answer() {
    let drag = Drag::Ring {
        origin: Vec3::ZERO,
        frame: (Vec3::Z, Vec3::X, Vec3::Y),
        orientation: Mat3::IDENTITY,
        last: 0.0,
        turned: 0.0,
    };
    let along = Ray::new(Vec3::new(0.0, 5.0, 0.0), Vec3::NEG_Y);

    assert_eq!(advance(drag, along, free()), None);
}

#[test]
fn a_change_that_leaves_the_part_exactly_as_it_was_is_no_change() {
    let target = block();

    assert_eq!(applied(target, Change::Position(target.position())), None);
    assert_eq!(
        applied(
            target,
            Change::Size {
                size: target.size(),
                position: target.position(),
            }
        ),
        None
    );
    assert_eq!(
        applied(target, Change::Orientation(target.orientation())),
        None
    );
}

#[test]
fn a_change_carries_the_part_through_to_the_next_drag_step() {
    let target = block();

    let resized = applied(
        target,
        Change::Size {
            size: Vec3::new(5.0, 1.0, 4.0),
            position: Vec3::new(1.5, 0.0, 0.0),
        },
    )
    .expect("the part did change");
    assert!((resized.size() - Vec3::new(5.0, 1.0, 4.0)).length() < 1e-4);
    assert!((resized.position() - Vec3::new(1.5, 0.0, 0.0)).length() < 1e-4);

    let turned = applied(
        target,
        Change::Orientation(Mat3::from_rotation_y(std::f32::consts::FRAC_PI_2)),
    )
    .expect("the part did turn");
    // Turning changes neither the size nor where it stands.
    assert!((turned.size() - target.size()).length() < 1e-4);
    assert!((turned.position() - target.position()).length() < 1e-4);
    assert!((turned.orientation().x_axis - Vec3::NEG_Z).length() < 1e-4);
}

// The bug this exists for: a snapped Move's first samples round their travel
// to nothing, and a gesture that spent its "first" on one of them wrote every
// later step without ever opening an undo entry.
#[test]
fn a_gesture_opens_its_undo_entry_on_its_first_real_change_not_its_first_sample() {
    let anchor = block();
    let mut dragged = false;

    // Samples that leave the part where it is are not steps at all.
    let still = Change::Position(anchor.position());
    assert_eq!(stepped(anchor, still, &mut dragged), None);
    assert_eq!(stepped(anchor, still, &mut dragged), None);
    assert!(!dragged, "a sample that changed nothing began no gesture");

    let moved = Change::Position(anchor.position() + Vec3::X);
    let (target, first) = stepped(anchor, moved, &mut dragged).expect("the part moved");
    assert!(first, "the first real change opens the gesture");
    assert_eq!(target.position(), anchor.position() + Vec3::X);

    let further = Change::Position(anchor.position() + Vec3::X * 2.0);
    let (_, first) = stepped(target, further, &mut dragged).expect("the part moved again");
    assert!(!first, "later steps share the entry the first one opened");
}

// The bug this exists for: the toolbar's stud increment rounded a Move's
// travel and nothing else, so a snapped Scale still grew by whatever fraction
// the cursor happened to cover.
#[test]
fn a_snapped_scale_drag_grows_by_whole_increments_of_the_pull() {
    let (_, free_change) =
        advance(grabbed_x_face(), looking_at(3.6, 0.0), free()).expect("across the view");
    let (_, snapped_change) =
        advance(grabbed_x_face(), looking_at(3.6, 0.0), grid(1.0)).expect("across the view");
    let (
        Change::Size {
            size: free_size, ..
        },
        Change::Size { size, position },
    ) = (free_change, snapped_change)
    else {
        panic!("expected resizes, got {free_change:?} and {snapped_change:?}");
    };

    let grabbed = grabbed_x_face();
    let Drag::Size { size: was, .. } = grabbed else {
        unreachable!()
    };
    let free_growth = free_size.x - was.x;
    let growth = size.x - was.x;
    assert!(
        free_growth.fract().abs() > 1e-3,
        "the pull itself is off-grid"
    );
    assert!(
        (growth - free_growth.round()).abs() < 1e-4,
        "grown by a whole stud"
    );
    // Still holding the far face still: the centre moved by half the growth.
    assert!((position.x - growth * 0.5).abs() < 1e-4);
}

#[test]
fn a_snapped_rotate_drag_turns_by_whole_increments_of_the_sweep() {
    let drag = Drag::Ring {
        origin: Vec3::ZERO,
        frame: (Vec3::Z, Vec3::X, Vec3::Y),
        orientation: Mat3::IDENTITY,
        last: 0.0,
        turned: 0.0,
    };
    // Swept 60° round the ring with a 45° increment: the part turns 45°.
    let sixty = 60f32.to_radians();
    let (_, change) = advance(
        drag,
        looking_at(5.0 * sixty.cos(), 5.0 * sixty.sin()),
        degrees(45.0),
    )
    .expect("the ray crosses the ring");
    let Change::Orientation(turned) = change else {
        panic!("expected a rotation, got {change:?}");
    };
    let expected = Vec3::new(45f32.to_radians().cos(), 45f32.to_radians().sin(), 0.0);
    assert!((turned.x_axis - expected).length() < 1e-4);
}

// Rounding each step before adding it up would let a slow drag creep round
// the ring without ever crossing an increment: the total is what is rounded.
#[test]
fn a_snapped_rotate_drag_rounds_its_whole_sweep_not_each_step() {
    let mut drag = Drag::Ring {
        origin: Vec3::ZERO,
        frame: (Vec3::Z, Vec3::X, Vec3::Y),
        orientation: Mat3::IDENTITY,
        last: 0.0,
        turned: 0.0,
    };
    let mut last = None;
    for step in [20f32, 40.0] {
        let angle = step.to_radians();
        let (next, change) = advance(
            drag,
            looking_at(5.0 * angle.cos(), 5.0 * angle.sin()),
            degrees(45.0),
        )
        .expect("the ray crosses the ring");
        drag = next;
        last = Some(change);
    }
    let Some(Change::Orientation(turned)) = last else {
        panic!("expected a rotation, got {last:?}");
    };
    // Two 20° steps are a 40° sweep, which rounds up to one 45° increment —
    // where per-step rounding would have turned the part not at all.
    let expected = Vec3::new(45f32.to_radians().cos(), 45f32.to_radians().sin(), 0.0);
    assert!((turned.x_axis - expected).length() < 1e-4);
}
