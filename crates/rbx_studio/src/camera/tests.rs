use rbx_dom::{CFrameData, Instance, Vector3Data};
use rbx_viewer::Pose;

use super::*;

fn pose(position: [f32; 3], yaw: f32, pitch: f32, fov_degrees: f32) -> Pose {
    Pose {
        position: glam::Vec3::from(position),
        yaw,
        pitch,
        fov_degrees,
    }
}

// A known camera position with focus two studs down the look vector;
// the rotation matrix is calibrated against Studio's own values.
const DEMO_ROTATION: [f32; 9] = [
    -0.76181036,
    0.30220547,
    -0.57299984,
    -4.656613e-10,
    0.88453233,
    0.46650687,
    0.6478026,
    0.35539314,
    -0.6738438,
];

fn insert(dom: &mut WeakDom, id: u32, class: &str) -> Ref {
    let reference = Ref::new(id);
    dom.insert(Instance::new(reference, class, class));
    reference
}

fn set(dom: &mut WeakDom, referent: Ref, property: &str, value: Variant) {
    dom.get_mut(referent)
        .expect("an inserted instance")
        .properties_mut()
        .insert(property.to_string(), value);
}

fn cframe(position: [f32; 3], rotation: [f32; 9]) -> Variant {
    Variant::CFrame(CFrameData {
        position: Vector3Data {
            x: position[0],
            y: position[1],
            z: position[2],
        },
        rotation,
    })
}

/// A Workspace whose `CurrentCamera` points at `camera_class`, itself carrying
/// `pose` when given.
fn place(camera_class: &str, pose: Option<Variant>) -> WeakDom {
    let mut dom = WeakDom::new();
    let workspace = insert(&mut dom, 1, "Workspace");
    let camera = insert(&mut dom, 2, camera_class);
    dom.set_parent(camera, Some(workspace));
    set(&mut dom, workspace, "CurrentCamera", Variant::Ref(camera));
    if let Some(pose) = pose {
        set(&mut dom, camera, "CFrame", pose);
    }

    dom
}

#[test]
fn the_eye_and_look_at_come_from_the_camera_cframe() {
    let dom = place(
        "Camera",
        Some(cframe([-121.42307, 54.06973, -126.13958], DEMO_ROTATION)),
    );

    let camera = PlaceCamera::from_dom(&dom).expect("the place's own camera");

    assert_eq!(camera.eye, [-121.42307, 54.06973, -126.13958]);
    let look = [
        camera.look_at[0] - camera.eye[0],
        camera.look_at[1] - camera.eye[1],
        camera.look_at[2] - camera.eye[2],
    ];
    for (axis, expected) in look.iter().zip([0.57299984, -0.46650687, 0.6738438]) {
        assert!((axis - expected).abs() < 1e-5, "{look:?}");
    }
}

#[test]
fn an_identity_rotation_looks_down_negative_z() {
    let identity = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    let dom = place("Camera", Some(cframe([0.0, 5.0, 0.0], identity)));

    let camera = PlaceCamera::from_dom(&dom).expect("the place's own camera");

    assert_eq!(camera.look_at, [0.0, 5.0, -1.0]);
}

#[test]
fn a_place_without_a_camera_has_no_viewpoint() {
    let mut dom = WeakDom::new();
    insert(&mut dom, 1, "Workspace");
    assert_eq!(PlaceCamera::from_dom(&dom), None);

    // Named, but gone from the file.
    let mut dom = WeakDom::new();
    let workspace = insert(&mut dom, 1, "Workspace");
    set(
        &mut dom,
        workspace,
        "CurrentCamera",
        Variant::Ref(Ref::new(9)),
    );
    assert_eq!(PlaceCamera::from_dom(&dom), None);

    assert_eq!(PlaceCamera::from_dom(&WeakDom::new()), None);
}

#[test]
fn a_current_camera_pointing_elsewhere_has_no_viewpoint() {
    let identity = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    let dom = place("Part", Some(cframe([0.0, 5.0, 0.0], identity)));

    assert_eq!(PlaceCamera::from_dom(&dom), None);
}

#[test]
fn a_camera_without_a_usable_cframe_has_no_viewpoint() {
    assert_eq!(PlaceCamera::from_dom(&place("Camera", None)), None);

    let zeroed = place("Camera", Some(cframe([0.0, 5.0, 0.0], [0.0; 9])));
    assert_eq!(PlaceCamera::from_dom(&zeroed), None);
}

#[test]
fn a_valid_field_of_view_is_read_from_the_camera() {
    let identity = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    let mut dom = place("Camera", Some(cframe([0.0, 5.0, 0.0], identity)));
    let referent = current_camera(&dom).expect("a CurrentCamera");
    set(&mut dom, referent, "FieldOfView", Variant::Float32(40.0));

    let camera = PlaceCamera::from_dom(&dom).expect("the place's own camera");

    assert_eq!(camera.fov_degrees, 40.0);
}

#[test]
fn a_missing_field_of_view_defaults_to_seventy_degrees() {
    let identity = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    let dom = place("Camera", Some(cframe([0.0, 5.0, 0.0], identity)));

    let camera = PlaceCamera::from_dom(&dom).expect("the place's own camera");

    assert_eq!(camera.fov_degrees, DEFAULT_FOV_DEGREES);
}

// Roblox's identity CFrame looks down -Z with the studs' own X/Y axes — this
// is the calibration point every other `cframe_from_pose` case is trusted
// against, since it is the one rotation this module already had a
// hand-verified expectation for (`an_identity_rotation_looks_down_negative_z`).
#[test]
fn a_pose_with_zero_yaw_and_pitch_builds_the_identity_rotation() {
    let identity = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    let cframe = cframe_from_pose(pose([0.0, 0.0, 0.0], 0.0, 0.0, 70.0));
    for (axis, expected) in cframe.rotation.iter().zip(identity) {
        assert!((axis - expected).abs() < 1e-6, "{:?}", cframe.rotation);
    }
}

// A pose yawed 90 degrees with no pitch: `write_pose`'s CFrame, read back
// through `PlaceCamera::from_dom` exactly as Studio would reopen the file,
// must land on the same eye and the same look direction the render thread
// was actually drawing.
#[test]
fn a_written_pose_round_trips_through_place_camera() {
    let mut dom = WeakDom::new();
    let workspace = insert(&mut dom, 1, "Workspace");

    write_pose(
        &mut dom,
        pose([10.0, 20.0, -30.0], std::f32::consts::FRAC_PI_2, 0.0, 90.0),
    );

    let camera_ref = existing_camera(&dom, workspace).expect("write_pose made a Camera");
    assert_eq!(dom.get(camera_ref).expect("the camera").class(), "Camera");

    let camera = PlaceCamera::from_dom(&dom).expect("write_pose's own camera");
    assert_eq!(camera.eye, [10.0, 20.0, -30.0]);
    assert_eq!(camera.fov_degrees, 90.0);

    let look = [
        camera.look_at[0] - camera.eye[0],
        camera.look_at[1] - camera.eye[1],
        camera.look_at[2] - camera.eye[2],
    ];
    for (axis, expected) in look
        .iter()
        .zip(free_flight_direction(std::f32::consts::FRAC_PI_2, 0.0))
    {
        assert!((axis - expected).abs() < 1e-5, "{look:?}");
    }
}

// Straight up and straight down: `cframe_from_pose`'s right-vector fallback
// only ever matters this close to ±90°, and even there the result must stay a
// finite, round-trippable rotation rather than the NaN a naive cross product
// would produce.
#[test]
fn a_near_vertical_pose_still_round_trips() {
    for pitch in [89.0_f32.to_radians(), -89.0_f32.to_radians()] {
        let mut dom = WeakDom::new();
        insert(&mut dom, 1, "Workspace");
        write_pose(&mut dom, pose([0.0, 5.0, 0.0], 0.3, pitch, 70.0));

        let camera = PlaceCamera::from_dom(&dom).expect("a Camera near the pole");
        let look = [
            camera.look_at[0] - camera.eye[0],
            camera.look_at[1] - camera.eye[1],
            camera.look_at[2] - camera.eye[2],
        ];
        assert!(look.iter().all(|axis| axis.is_finite()), "{look:?}");
        for (axis, expected) in look.iter().zip(free_flight_direction(0.3, pitch)) {
            assert!((axis - expected).abs() < 1e-4, "{look:?}");
        }
    }
}

// Reusing the place's own camera rather than leaving a second one behind
// every call — the DOM write runs a few times a second while the camera
// flies, so `write_pose` must overwrite the same instance, not multiply it.
#[test]
fn write_pose_reuses_the_existing_camera_across_calls() {
    let mut dom = WeakDom::new();
    let workspace = insert(&mut dom, 1, "Workspace");

    write_pose(&mut dom, pose([0.0, 0.0, 0.0], 0.0, 0.0, 70.0));
    let first = existing_camera(&dom, workspace).expect("the first write's camera");

    write_pose(&mut dom, pose([5.0, 0.0, 0.0], 1.0, 0.0, 70.0));
    let second = existing_camera(&dom, workspace).expect("the second write's camera");

    assert_eq!(first, second);
    let camera = PlaceCamera::from_dom(&dom).expect("the reused camera");
    assert_eq!(camera.eye, [5.0, 0.0, 0.0]);
}

// A `Workspace` that never had a `Camera` at all: `write_pose` must create
// one, exactly as a script's `Instance.new("Camera", workspace)` would, rather
// than silently doing nothing.
#[test]
fn write_pose_creates_a_camera_when_the_place_has_none() {
    let mut dom = WeakDom::new();
    let workspace = insert(&mut dom, 1, "Workspace");
    assert_eq!(existing_camera(&dom, workspace), None);

    write_pose(&mut dom, pose([1.0, 2.0, 3.0], 0.0, 0.0, 70.0));

    let camera = PlaceCamera::from_dom(&dom).expect("write_pose created a camera");
    assert_eq!(camera.eye, [1.0, 2.0, 3.0]);
}

// No `Workspace` at all (a model file, say): nowhere to attach a camera, so
// this must not panic or insert one at the DOM's root.
#[test]
fn write_pose_is_a_no_op_without_a_workspace() {
    let mut dom = WeakDom::new();
    write_pose(&mut dom, pose([1.0, 2.0, 3.0], 0.0, 0.0, 70.0));
    assert_eq!(PlaceCamera::from_dom(&dom), None);
}

#[test]
fn a_degenerate_field_of_view_never_reaches_the_renderer() {
    let identity = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

    for degenerate in [0.0, -40.0, f32::NAN] {
        let mut dom = place("Camera", Some(cframe([0.0, 5.0, 0.0], identity)));
        let referent = current_camera(&dom).expect("a CurrentCamera");
        set(
            &mut dom,
            referent,
            "FieldOfView",
            Variant::Float32(degenerate),
        );

        let camera = PlaceCamera::from_dom(&dom).expect("the place's own camera");

        assert_eq!(camera.fov_degrees, DEFAULT_FOV_DEGREES, "for {degenerate}");
    }
}
