use std::sync::OnceLock;

use glam::Vec3;
use rbx_dom::{CFrameData, Ref, Variant, Vector3Data, WeakDom};
use rbx_reflection::ReflectionDatabase;
use rbx_viewer::Pose;

use super::{
    already_in, camera_of, contents, fit, insertable, matched, orbit, pan, pose_of, write_pose,
    Gesture, Step, FRAME_CLASS,
};
use crate::history::History;

fn database() -> &'static ReflectionDatabase {
    static DATABASE: OnceLock<ReflectionDatabase> = OnceLock::new();
    DATABASE.get_or_init(ReflectionDatabase::embedded)
}

fn part(dom: &mut WeakDom, parent: Ref, name: &str, at: [f32; 3], size: [f32; 3]) -> Ref {
    let part = dom.new_instance("Part", name, Some(parent));
    let [x, y, z] = at;
    dom.set_property(
        part,
        "CFrame",
        Variant::CFrame(CFrameData {
            position: Vector3Data { x, y, z },
            rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        }),
    )
    .unwrap();
    let [x, y, z] = size;
    dom.set_property(part, "size", Variant::Vector3(Vector3Data { x, y, z }))
        .unwrap();
    part
}

/// A place with a `Workspace` holding a model of two parts, and a
/// `ScreenGui` holding a `ViewportFrame` with nothing under it.
fn place() -> (WeakDom, Ref, Ref) {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let model = dom.new_instance("Model", "Car", Some(workspace));
    part(&mut dom, model, "Body", [0.0, 0.0, 0.0], [4.0, 2.0, 8.0]);
    part(&mut dom, model, "Roof", [0.0, 2.0, 0.0], [4.0, 1.0, 4.0]);
    let gui = dom.new_instance("StarterGui", "StarterGui", None);
    let screen = dom.new_instance("ScreenGui", "Test", Some(gui));
    let frame = dom.new_instance(FRAME_CLASS, "VF", Some(screen));
    dom.take_changes();
    (dom, model, frame)
}

fn pose(z: f32) -> Pose {
    Pose {
        position: Vec3::new(1.0, 2.0, z),
        yaw: 0.3,
        pitch: -0.2,
        fov_degrees: 50.0,
        ortho_scale: 1.0,
    }
}

fn close(a: Vec3, b: Vec3) -> bool {
    a.distance(b) < 1e-3
}

#[test]
fn a_frame_with_no_camera_gets_one_wired_up_and_the_pose_saved_on_the_frame() {
    let (mut dom, _, frame) = place();
    assert_eq!(camera_of(&dom, database(), frame), None);
    assert!(pose_of(&dom, database(), frame).is_none());

    let camera = write_pose(&mut dom, database(), frame, pose(10.0));
    let instance = dom.get(camera).unwrap();
    assert_eq!(instance.class(), "Camera");
    assert_eq!(dom.parent(camera), Some(frame));
    let properties = dom.get(frame).unwrap().properties();
    assert_eq!(properties.get("CurrentCamera"), Some(&Variant::Ref(camera)));
    // What a saved place keeps, since `CurrentCamera` itself does not save.
    assert!(matches!(
        properties.get("CameraCFrame"),
        Some(Variant::CFrame(_))
    ));
    assert_eq!(
        properties.get("CameraFieldOfView"),
        Some(&Variant::Float32(50f32.to_radians()))
    );

    let read = pose_of(&dom, database(), frame).unwrap();
    assert!(close(read.position, pose(10.0).position));
    assert!((read.yaw - 0.3).abs() < 1e-4 && (read.pitch + 0.2).abs() < 1e-4);
    assert!((read.fov_degrees - 50.0).abs() < 1e-4);

    // A second write moves the same camera rather than making another.
    assert_eq!(write_pose(&mut dom, database(), frame, pose(5.0)), camera);
    assert_eq!(dom.get(frame).unwrap().children().len(), 1);
}

#[test]
fn a_reloaded_frame_reuses_its_camera_and_reads_its_saved_pose() {
    let (mut dom, _, frame) = place();
    let camera = write_pose(&mut dom, database(), frame, pose(10.0));
    // A reopened place: `CurrentCamera` did not survive the save.
    let _ = dom.remove_property(frame, "CurrentCamera");
    assert_eq!(camera_of(&dom, database(), frame), Some(camera));
    dom.remove(camera);
    let read = pose_of(&dom, database(), frame).expect("the frame's own CameraCFrame");
    assert!(close(read.position, pose(10.0).position));
}

#[test]
fn creating_the_camera_and_a_whole_flight_are_one_undo_step() {
    let (mut dom, _, frame) = place();
    let before = dom.clone();
    let mut history = History::new(50);
    let mut gesture = Gesture::default();

    // The look button held through several moves, then the keys let go
    // while the move still eases out, then settled.
    let steps = [(true, true), (true, true), (false, true), (true, false)];
    let mut pushed = 0;
    for (index, (moved, busy)) in steps.into_iter().enumerate() {
        match gesture.step(moved, busy) {
            Some(Step::First) => {
                history.push(dom.clone());
                pushed += 1;
            }
            Some(Step::Continue) | None => {}
        }
        if moved {
            write_pose(&mut dom, database(), frame, pose(10.0 - index as f32));
        }
    }
    assert_eq!(pushed, 1, "one gesture, one undo entry");

    let (undone, _) = history.undo(dom.clone()).expect("one step to undo");
    assert_eq!(camera_of(&undone, database(), frame), None);
    assert!(undone
        .get(frame)
        .unwrap()
        .properties()
        .get("CurrentCamera")
        .is_none());
    assert_eq!(
        undone.get(frame).unwrap().children().len(),
        before.get(frame).unwrap().children().len()
    );
    assert!(history.undo(undone).is_none(), "and nothing under it");

    // The next press is a gesture of its own.
    assert_eq!(gesture.step(true, true), Some(Step::First));
}

#[test]
fn a_fit_holds_the_whole_model_in_a_frame_of_either_shape() {
    let (mut dom, model, frame) = place();
    assert_eq!(contents(&dom, database(), frame), None);
    let mut copy = dom.clone();
    let car = copy.get(model).unwrap().children().to_vec();
    for part in car {
        copy.set_parent(part, Some(frame));
    }
    dom = copy;

    let (centre, radius) = contents(&dom, database(), frame).unwrap();
    // The two boxes span y -1..2.5, x ±2, z ±4.
    assert!(close(centre, Vec3::new(0.0, 0.75, 0.0)));
    let corner = Vec3::new(2.0, 1.75, 4.0).length();
    assert!((radius - corner).abs() < 1e-3);

    for aspect in [0.5, 1.0, 2.0] {
        let fitted = fit((centre, radius), (0.4, 0.3), 70.0, aspect);
        let distance = fitted.position.distance(centre);
        // The sphere just touches the narrower half-angle.
        let vertical = 35f32.to_radians();
        let half = vertical.min((vertical.tan() * aspect).atan());
        assert!(
            (radius / distance - half.sin()).abs() < 1e-4,
            "aspect {aspect}"
        );
        // And the camera looks straight at the centre.
        let forward = Vec3::from(crate::camera::free_flight_direction(0.4, 0.3));
        assert!(close(fitted.position + forward * distance, centre));
    }
}

#[test]
fn the_picker_lists_workspace_models_and_parts_with_where_they_sit() {
    let (dom, model, frame) = place();
    let rows = insertable(&dom, database());
    let names: Vec<(&str, usize)> = rows
        .iter()
        .map(|row| (row.name.as_str(), row.depth))
        .collect();
    assert_eq!(names, [("Car", 0), ("Body", 1), ("Roof", 1)]);
    assert_eq!(rows[0].referent, model);
    assert!(rows[0].model && !rows[1].model);
    assert_eq!(rows[2].path, ["Car"]);

    assert_eq!(matched("Retaining wall", "WALL"), Some(10..14));
    assert_eq!(matched("Roof", "wall"), None);
    assert_eq!(matched("Roof", " "), None);

    assert!(!already_in(&dom, frame, &rows[0]));
    let mut dom = dom;
    dom.new_instance("Model", "Car", Some(frame));
    assert!(already_in(&dom, frame, &rows[0]));
    assert!(!already_in(&dom, frame, &rows[1]));
}

#[test]
fn an_orbit_keeps_its_distance_and_a_pan_keeps_its_aim() {
    let start = pose(10.0);
    let pivot = Vec3::new(1.0, 2.0, 0.0);
    let turned = orbit(start, pivot, 0.5, 0.1);
    assert!((turned.position.distance(pivot) - start.position.distance(pivot)).abs() < 1e-4);
    let forward = Vec3::from(crate::camera::free_flight_direction(
        turned.yaw,
        turned.pitch,
    ));
    let distance = turned.position.distance(pivot);
    assert!(
        close(turned.position + forward * distance, pivot),
        "still aimed at the pivot"
    );
    // Never over the top.
    assert!(orbit(start, pivot, 0.0, 10.0).pitch < std::f32::consts::FRAC_PI_2);

    let slid = pan(start, 2.0, 1.0);
    assert_eq!((slid.yaw, slid.pitch), (start.yaw, start.pitch));
    assert!((slid.position.distance(start.position) - 5f32.sqrt()).abs() < 1e-4);
}
