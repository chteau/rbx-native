use super::*;
use crate::controller::CameraFeel;
use crate::controller::Cursor;
use crate::input::{CameraInput, CameraKey};

const FRAME: Duration = Duration::from_millis(5);

fn feel() -> Feel {
    Feel::of(CameraFeel::default())
}

/// Studio's test camera: 30 studs back from the origin, looking down -Z.
fn start() -> Pose {
    crate::camera::look_at_pose(Vec3::new(0.0, 10.0, 30.0), Vec3::new(0.0, 10.0, 0.0))
}

fn orbiting(mode: CameraMode) -> (StudioCamera, Pose) {
    let mut camera = StudioCamera::new(mode);
    camera.place(30.0);
    (camera, start())
}

fn run(camera: &mut StudioCamera, pose: &mut Pose, input: &mut Input, room: Room, seconds: f32) {
    for _ in 0..(seconds / FRAME.as_secs_f32()).round() as usize {
        camera.step(pose, input, FRAME, feel(), room);
    }
}

fn once(camera: &mut StudioCamera, pose: &mut Pose, events: &[CameraInput], room: Room) {
    let mut input = Input::default();
    for event in events {
        input.apply(*event);
    }
    camera.step(pose, &mut input, FRAME, feel(), room);
}

fn key(key: CameraKey, pressed: bool) -> CameraInput {
    CameraInput::Key { key, pressed }
}

#[test]
fn the_right_button_orbits_the_focus_at_studios_rate() {
    let (mut camera, mut pose) = orbiting(CameraMode::Adaptive);
    let events = [
        CameraInput::LookButton(true),
        CameraInput::MouseLook { dx: 100.0, dy: 0.0 },
    ];
    once(&mut camera, &mut pose, &events, Room::default());
    let turned = (start().yaw - pose.yaw).to_degrees();
    assert!((turned - 65.0).abs() < 1e-3, "{turned}");
    let focus = pose.position + pose.basis().0 * 30.0;
    assert!(focus.distance(Vec3::new(0.0, 10.0, 0.0)) < 1e-3, "{focus}");
}

#[test]
fn flying_turns_the_view_in_place() {
    let mut camera = StudioCamera::new(CameraMode::Fps);
    let mut pose = start();
    let events = [
        CameraInput::LookButton(true),
        CameraInput::MouseLook { dx: 0.0, dy: 50.0 },
    ];
    once(&mut camera, &mut pose, &events, Room::default());
    assert_eq!(pose.position, start().position);
    assert!((pose.pitch.to_degrees() - 32.5).abs() < 1e-3);
}

#[test]
fn five_wheel_notches_halve_the_distance_to_the_focus() {
    let (mut camera, mut pose) = orbiting(CameraMode::Adaptive);
    once(
        &mut camera,
        &mut pose,
        &[CameraInput::Wheel { notches: 5.0 }],
        Room::default(),
    );
    run(
        &mut camera,
        &mut pose,
        &mut Input::default(),
        Room::default(),
        1.0,
    );
    assert!((pose.position.z - 15.0).abs() < 1e-3, "{}", pose.position);
    assert!((camera.speed() - 30.0).abs() < 1e-3);
}

#[test]
fn the_zoom_glides_in_with_a_short_time_constant() {
    let (mut camera, mut pose) = orbiting(CameraMode::Adaptive);
    once(
        &mut camera,
        &mut pose,
        &[CameraInput::Wheel { notches: 5.0 }],
        Room::default(),
    );
    run(
        &mut camera,
        &mut pose,
        &mut Input::default(),
        Room::default(),
        0.036 - 0.005,
    );
    let closed = (30.0 - pose.position.z) / 15.0;
    assert!((closed - 0.632).abs() < 0.02, "{closed}");
}

#[test]
fn flying_zooms_towards_the_surface_under_the_cursor() {
    let mut camera = StudioCamera::new(CameraMode::Fps);
    let mut pose = start();
    let hit = Vec3::new(20.0, 2.0, -10.0);
    let room = Room {
        cursor: Some(Cursor {
            direction: (hit - pose.position).normalize(),
            hit: Some(hit),
        }),
        ..Room::default()
    };
    once(
        &mut camera,
        &mut pose,
        &[CameraInput::Wheel { notches: 5.0 }],
        room,
    );
    run(
        &mut camera,
        &mut pose,
        &mut Input::default(),
        Room::default(),
        1.0,
    );
    let halfway = (start().position + hit) * 0.5;
    assert!(pose.position.distance(halfway) < 1e-3, "{}", pose.position);
    assert_eq!(pose.yaw, start().yaw);
}

#[test]
fn the_fly_speed_follows_the_focus_distance_then_freezes_once_flying() {
    let (mut camera, mut pose) = orbiting(CameraMode::Adaptive);
    assert_eq!(camera.speed(), 60.0);
    let mut input = Input::default();
    input.apply(key(CameraKey::Forward, true));
    run(&mut camera, &mut pose, &mut input, Room::default(), 2.0);
    let flown = 30.0 - pose.position.z;
    assert_eq!(camera.speed(), 60.0);
    // 2 s at 60 studs/s, less the ramp's lag of one time constant.
    assert!((flown - 60.0 * (2.0 - 0.137)).abs() < 0.5, "{flown}");
}

#[test]
fn keys_ramp_up_slowly_and_stop_quickly() {
    let mut camera = StudioCamera::new(CameraMode::Fps);
    let mut pose = start();
    let mut input = Input::default();
    input.apply(key(CameraKey::Forward, true));
    run(&mut camera, &mut pose, &mut input, Room::default(), 0.135);
    let share = camera.velocity.length() / 60.0;
    assert!((share - 0.632).abs() < 0.02, "{share}");
    run(&mut camera, &mut pose, &mut input, Room::default(), 2.0);
    input.apply(key(CameraKey::Forward, false));
    run(&mut camera, &mut pose, &mut input, Room::default(), 0.040);
    let left = camera.velocity.length() / 60.0;
    assert!((left - 0.368).abs() < 0.02, "{left}");
}

#[test]
fn shift_slows_the_keys_to_a_fifth() {
    let mut camera = StudioCamera::new(CameraMode::Fps);
    let mut pose = start();
    let mut input = Input::default();
    input.apply(key(CameraKey::Slow, true));
    input.apply(key(CameraKey::Right, true));
    run(&mut camera, &mut pose, &mut input, Room::default(), 2.0);
    assert!((camera.velocity.length() - 12.0).abs() < 1e-2);
}

#[test]
fn the_wheel_with_the_right_button_steps_the_multiplier() {
    let (mut camera, mut pose) = orbiting(CameraMode::Adaptive);
    let events = [
        CameraInput::LookButton(true),
        CameraInput::Wheel { notches: 1.0 },
    ];
    once(&mut camera, &mut pose, &events, Room::default());
    assert!(
        (camera.multiplier - 2.378).abs() < 1e-3,
        "{}",
        camera.multiplier
    );
    assert_eq!(pose.position, start().position);
}

#[test]
fn alt_and_the_right_button_dolly_towards_the_focus() {
    let (mut camera, mut pose) = orbiting(CameraMode::Adaptive);
    let events = [
        CameraInput::LookButton(true),
        CameraInput::Dolly(true),
        CameraInput::MouseLook { dx: 100.0, dy: 0.0 },
    ];
    once(&mut camera, &mut pose, &events, Room::default());
    assert!((pose.position.z - 47.62).abs() < 0.01, "{}", pose.position);
    assert_eq!(pose.yaw, start().yaw);
}

#[test]
fn the_middle_button_pans_at_the_focus_while_orbiting() {
    let (mut camera, mut pose) = orbiting(CameraMode::Adaptive);
    let room = Room {
        height: Some(395.0),
        ..Room::default()
    };
    let events = [
        CameraInput::PanButton(Some(1.0)),
        CameraInput::MouseLook {
            dx: -100.0,
            dy: 0.0,
        },
    ];
    once(&mut camera, &mut pose, &events, room);
    assert!((pose.position.x - 10.636).abs() < 0.01, "{}", pose.position);
}

#[test]
fn focus_glides_in_and_orbits_what_it_framed() {
    let (mut camera, mut pose) = orbiting(CameraMode::Adaptive);
    let target =
        crate::camera::look_at_pose(Vec3::new(20.0, 2.0, -3.92), Vec3::new(20.0, 2.0, -10.0));
    camera.frame(target, 6.08);
    run(
        &mut camera,
        &mut pose,
        &mut Input::default(),
        Room::default(),
        0.5,
    );
    assert!(pose.position.distance(target.position) < 1e-3);
    assert!((camera.speed() - 12.16).abs() < 1e-2);
}

#[test]
fn fps_mode_keeps_its_own_speed_through_focus() {
    let mut camera = StudioCamera::new(CameraMode::Fps);
    camera.frame(start(), 6.0);
    assert_eq!(camera.speed(), 60.0);
    assert_eq!(camera.multiplier(), None);
}

#[test]
fn orbit_mode_carries_the_focus_along_with_the_keys() {
    let (mut camera, mut pose) = orbiting(CameraMode::Orbit);
    let mut input = Input::default();
    input.apply(key(CameraKey::Right, true));
    run(&mut camera, &mut pose, &mut input, Room::default(), 1.0);
    assert_eq!(camera.speed(), 60.0);
    assert!(camera.orbiting);
}

#[test]
fn focus_frames_the_box_at_studios_distance() {
    let (_, right, up) = start().basis();
    let basis = (-Vec3::Z, right, up);
    let aspect = 759.0 / 395.0;
    let framed = |size: Vec3| {
        let half = size * 0.5;
        let corners = (0..8).map(move |i| {
            Vec3::new(
                if i & 1 == 0 { -half.x } else { half.x },
                if i & 2 == 0 { -half.y } else { half.y },
                if i & 4 == 0 { -half.z } else { half.z },
            )
        });
        focus_distance(corners, Vec3::ZERO, basis, 70.0, aspect)
    };
    for (size, studio) in [
        (Vec3::splat(4.0), 6.081),
        (Vec3::new(10.0, 2.0, 2.0), 6.309),
        (Vec3::ONE, 1.520),
        (Vec3::new(200.0, 80.0, 2.0), 107.18),
    ] {
        let ours = framed(size);
        assert!(
            (ours - studio).abs() / studio < 2e-3,
            "{size}: {ours} vs {studio}"
        );
    }
}

#[test]
fn a_rebuilt_or_replaced_controller_keeps_the_multiplier() {
    use crate::controller::{CameraStyle, Controller, Start, DEFAULT_SENSITIVITY};
    let bounds = crate::scene::tests_support::bounds_from(Vec3::splat(-10.0), Vec3::splat(10.0));
    let feel = CameraFeel {
        style: CameraStyle::Studio(CameraMode::Adaptive),
        ..CameraFeel::default()
    };
    let fresh = || {
        let mut controller =
            Controller::new(Start::Pose(start()), &bounds, None, DEFAULT_SENSITIVITY);
        controller.set_feel(feel);
        controller
    };
    let mut old = fresh();
    if let Some(studio) = &mut old.studio {
        studio.multiplier = 4.0;
    }
    let mut placed = fresh();
    placed.adopt(&old, Some(10.0));
    assert_eq!(placed.multiplier(), Some(4.0));
    assert_eq!(placed.speed(), 40.0);
    let mut classic = Controller::new(Start::Pose(start()), &bounds, None, DEFAULT_SENSITIVITY);
    classic.adopt(&old, None);
    assert_eq!(classic.multiplier(), None);
}
