use glam::Vec3;
use rbx_dom::{Ref, Variant, WeakDom};
use rbx_viewer::pick::Ray;
use rbx_viewer::sun::{place, Body, Placement};

use super::*;
use crate::history::{History, DEFAULT_CAP};

fn close(a: Vec3, b: Vec3) -> bool {
    (a - b).length() < 1e-4
}

fn tool(mode: Mode) -> SunTool {
    SunTool {
        mode,
        ..SunTool::default()
    }
}

/// A scene whose every ray lands on one flat floor at the origin, facing up.
fn floor(part: Ref) -> impl Fn(Ray, Option<Ref>) -> Option<(Ref, Surface)> {
    move |_, _| {
        Some((
            part,
            Surface {
                point: Vec3::ZERO,
                normal: Vec3::Y,
            },
        ))
    }
}

fn sky(_: Ray, _: Option<Ref>) -> Option<(Ref, Surface)> {
    None
}

#[test]
fn sky_aims_the_body_down_the_cursor_ray_and_draws_no_guide() {
    let ray = Ray::new(Vec3::ZERO, Vec3::new(1.0, 2.0, 0.5));
    let aim = tool(Mode::Sky)
        .aim(ray, true, sky)
        .expect("sky needs no surface");
    assert!(close(aim.toward, ray.direction));
    assert_eq!(aim.guide(&place(Body::Sun, aim.toward)), None);
}

#[test]
fn face_aims_the_body_straight_down_the_normal_from_the_hit() {
    let ray = Ray::new(Vec3::new(3.0, 5.0, 1.0), -Vec3::Y);
    let aim = tool(Mode::Face)
        .aim(ray, true, floor(Ref::new(1)))
        .expect("the floor is under the cursor");
    assert!(close(aim.toward, Vec3::Y));
    let guide = aim.guide(&place(Body::Sun, aim.toward)).expect("a guide");
    assert_eq!((guide.from, guide.target), (Vec3::ZERO, None));

    assert_eq!(tool(Mode::Face).aim(ray, true, sky), None);
}

// Looking down at 45° from -X, the mirror image of the eye about the
// floor's normal is up at 45° toward +X.
#[test]
fn glint_aims_the_body_where_its_reflection_reaches_the_camera() {
    let ray = Ray::new(Vec3::new(-1.0, 1.0, 0.0), Vec3::new(1.0, -1.0, 0.0));
    let aim = tool(Mode::Glint)
        .aim(ray, true, floor(Ref::new(1)))
        .expect("the floor is under the cursor");
    assert!(close(aim.toward, Vec3::new(1.0, 1.0, 0.0).normalize()));
    assert_eq!(glint(Vec3::Y, Vec3::Y), Vec3::Y, "head-on glints back");
}

#[test]
fn shadow_holds_the_caster_from_the_press_and_aims_through_it() {
    let caster = Ref::new(7);
    let ground = Ref::new(8);
    let mut sun = tool(Mode::Shadow);
    let press = Ray::new(Vec3::new(0.0, 10.0, 0.0), -Vec3::Y);
    sun.press(press, |_, _| {
        Some((
            caster,
            Surface {
                point: Vec3::new(0.0, 4.0, 0.0),
                normal: Vec3::Y,
            },
        ))
    });

    // The ground is found leaving the caster out, so the ray can pass
    // through it to whatever stands beyond.
    let ground_at = |at: Vec3| {
        move |_: Ray, exclude: Option<Ref>| {
            assert_eq!(exclude, Some(caster), "the caster is left out");
            Some((
                ground,
                Surface {
                    point: at,
                    normal: Vec3::Y,
                },
            ))
        }
    };
    let cursor = Ray::new(Vec3::new(4.0, 10.0, 0.0), -Vec3::Y);
    assert_eq!(
        sun.aim(press, true, ground_at(Vec3::ZERO)),
        None,
        "the press only picks the caster up"
    );

    let aim = sun
        .aim(cursor, false, ground_at(Vec3::new(4.0, 0.0, 0.0)))
        .expect("caster and ground both found");
    assert!(close(aim.toward, Vec3::new(-4.0, 4.0, 0.0).normalize()));
    let guide = aim.guide(&place(Body::Sun, aim.toward)).expect("a guide");
    assert_eq!(guide.from, Vec3::new(0.0, 4.0, 0.0));
    assert_eq!(guide.target, Some(Vec3::new(4.0, 0.0, 0.0)));
    assert_eq!(shadow(Vec3::ONE, Vec3::ONE), None);
}

#[test]
fn shadow_pressed_on_nothing_places_nothing() {
    let mut sun = tool(Mode::Shadow);
    let ray = Ray::new(Vec3::ZERO, Vec3::Y);
    sun.press(ray, sky);
    assert_eq!(sun.aim(ray, false, floor(Ref::new(1))), None);
}

// The line is a box with no width or depth, which the preview pass draws as
// its one remaining edge: from where the shadow lands, through the caster,
// on toward the light.
#[test]
fn a_guide_is_one_collapsed_box_and_a_marker_on_the_target() {
    let guide = Guide {
        from: Vec3::new(0.0, 4.0, 0.0),
        toward: Vec3::Y,
        target: Some(Vec3::ZERO),
    };
    let boxes = guide.boxes(2.0);
    assert_eq!(boxes.len(), 2);
    let line = boxes[0];
    let end = Vec3::new(0.0, 4.0 + 2.0 * GUIDE_ARMS, 0.0);
    assert!(close(line.transform_point3(Vec3::splat(-0.5)), Vec3::ZERO));
    assert!(close(line.transform_point3(Vec3::splat(0.5)), end));
    assert_eq!(boxes[1].w_axis.truncate(), Vec3::ZERO);

    let face = Guide {
        target: None,
        ..guide
    };
    assert_eq!(face.boxes(2.0).len(), 1, "no marker without a target");
}

fn lighting_dom(extra: &[(&str, Variant)]) -> (WeakDom, Ref) {
    let mut dom = WeakDom::new();
    dom.new_instance("Workspace", "Workspace", None);
    let lighting = dom.new_instance("Lighting", "Lighting", None);
    dom.set_property(lighting, TIME_OF_DAY, Variant::String("14:30:00".into()))
        .unwrap();
    dom.set_property(lighting, LATITUDE, Variant::Float32(0.0))
        .unwrap();
    for (name, value) in extra {
        dom.set_property(lighting, name, value.clone()).unwrap();
    }
    dom.take_changes();
    (dom, lighting)
}

#[test]
fn the_tool_is_unavailable_without_a_lighting_service() {
    let mut dom = WeakDom::new();
    dom.new_instance("Workspace", "Workspace", None);
    assert_eq!(lighting(&dom), None);

    let (dom, service) = lighting_dom(&[]);
    assert_eq!(lighting(&dom), Some(service));
}

#[test]
fn a_step_writes_the_saved_clock_and_the_latitude() {
    let (dom, lighting) = lighting_dom(&[]);
    let properties = dom.get(lighting).unwrap().properties();
    let placement = at(6.25, 40.0);

    let writes = writes(properties, &placement);
    assert_eq!(writes.len(), 2);
    assert_eq!(writes[1], (TIME_OF_DAY, Variant::String("06:15:00".into())));
    assert_eq!(writes[0], (LATITUDE, Variant::Float32(40.0)));
}

// A `ClockTime` a script left behind is read ahead of `TimeOfDay`, so it is
// kept in step, in its own float width.
#[test]
fn a_clock_time_already_there_is_kept_in_step() {
    let (dom, lighting) = lighting_dom(&[(CLOCK_TIME, Variant::Float64(14.5))]);
    let placement = at(9.0, 23.5);
    let writes = writes(dom.get(lighting).unwrap().properties(), &placement);
    assert!(writes
        .iter()
        .any(|(name, value)| *name == CLOCK_TIME && matches!(value, Variant::Float64(_))));
}

#[test]
fn a_step_that_changes_nothing_writes_nothing() {
    let (mut dom, lighting) = lighting_dom(&[]);
    let placement = at(8.0, 10.0);
    for (name, value) in writes(dom.get(lighting).unwrap().properties(), &placement) {
        dom.set_property(lighting, name, value).unwrap();
    }
    assert!(writes(dom.get(lighting).unwrap().properties(), &placement).is_empty());
}

#[test]
fn a_time_of_day_is_spelled_the_way_a_place_file_spells_it() {
    assert_eq!(time_of_day(14.5), "14:30:00");
    assert_eq!(time_of_day(24.0), "00:00:00");
    assert_eq!(time_of_day(-0.5), "23:30:00");
    assert_eq!(time_of_day(6.0 + 7.0 / 3600.0), "06:00:07");
}

#[test]
fn the_readout_names_the_body_and_owns_up_to_a_clamp() {
    let noon = place(Body::Sun, Vec3::Y);
    assert_eq!(readout(Body::Sun, &noon), "Sun 12:00 · lat 23.5°");
    let held = place(Body::Moon, -Vec3::Z);
    assert!(held.clamped);
    assert!(readout(Body::Moon, &held).ends_with("lat 90.0° (limit)"));
}

// Three drag steps, each a different sky, written the way `shell::sun`
// writes them: history is pushed on the first real write alone, so one undo
// puts the whole gesture back and there is nothing left behind it.
#[test]
fn a_whole_drag_is_one_undo_step() {
    let (mut dom, lighting) = lighting_dom(&[]);
    let before = dom.get(lighting).unwrap().properties().clone();
    let mut history = History::new(DEFAULT_CAP);
    let mut sun = tool(Mode::Sky);
    let ray = |toward: Vec3| Ray::new(Vec3::ZERO, toward);

    sun.press(ray(Vec3::X), sky);
    for (index, toward) in [Vec3::X, Vec3::new(1.0, 1.0, 0.0), Vec3::Y]
        .into_iter()
        .enumerate()
    {
        let aim = sun.aim(ray(toward), index == 0, sky).expect("sky aims");
        let placement = place(Body::Sun, aim.toward);
        let writes = writes(dom.get(lighting).unwrap().properties(), &placement);
        assert!(!writes.is_empty(), "every step here moves the sun");
        if sun.opens_step() {
            dom.take_changes();
            history.push(dom.clone());
        }
        for (name, value) in writes {
            dom.set_property(lighting, name, value).unwrap();
        }
        history.record_changes(dom.take_changes());
    }

    let (undone, _) = history.undo(dom.clone()).expect("the drag to undo");
    assert_eq!(undone.get(lighting).unwrap().properties(), &before);
    assert!(history.undo(undone).is_none(), "one step, not three");

    sun.press(ray(Vec3::X), sky);
    assert!(sun.opens_step(), "the next drag opens its own step");
}

/// A placement as `rbx_viewer::sun::place` would report it — only the two
/// numbers matter to what a step writes.
fn at(clock_time: f32, geographic_latitude: f32) -> Placement {
    Placement {
        clock_time,
        geographic_latitude,
        direction: Vec3::Y,
        clamped: false,
    }
}
