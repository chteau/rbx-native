use glam::Vec3;

use super::*;

/// Looking down -Z from ten studs out, at a point on the z=0 plane.
fn looking_at(x: f32, y: f32) -> Ray {
    Ray::new(Vec3::new(x, y, 10.0), Vec3::NEG_Z)
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
    let moved = moved_to(drag, looking_at(6.5, 0.0)).expect("the axis is across the view");
    assert!((moved - Vec3::new(4.5, 0.0, 0.0)).length() < 1e-4);

    // Back past where it started.
    let moved = moved_to(drag, looking_at(-1.0, 0.0)).expect("the axis is across the view");
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
    let moved = moved_to(drag, ray).expect("the axis is across the view");
    assert!((moved - Vec3::new(4.0, 1.0, -2.0)).length() < 1e-4);
}

#[test]
fn an_axis_sighted_end_on_has_no_answer_rather_than_a_wild_one() {
    let drag = Drag::Axis {
        origin: Vec3::ZERO,
        axis: Vec3::Z,
        grabbed: 0.0,
    };
    assert_eq!(moved_to(drag, looking_at(0.0, 0.0)), None);
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

    let moved = moved_to(drag, looking_at(3.0, 4.0)).expect("the ray crosses the plane");
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
    let moved = moved_to(drag, from_the_side).expect("the ray crosses the plane");
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

    assert_eq!(moved_to(drag, along), None);
}
