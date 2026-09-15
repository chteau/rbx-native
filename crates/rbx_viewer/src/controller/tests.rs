use super::*;
use crate::input::{CameraInput, CameraKey};
use crate::scene::tests_support::bounds_from;

fn bounds() -> Bounds {
    bounds_from(Vec3::splat(-10.0), Vec3::splat(10.0))
}

fn spawned() -> Controller {
    Controller::new(Start::Spawn, &bounds(), Some(30.0), 0.2)
}

fn held(keys: &[CameraKey]) -> Input {
    let mut input = Input::default();
    for key in keys {
        input.apply(CameraInput::Key {
            key: *key,
            pressed: true,
        });
    }
    input
}

fn looking(dx: f32, dy: f32) -> Input {
    let mut input = Input::default();
    input.apply(CameraInput::LookButton(true));
    input.apply(CameraInput::MouseLook { dx, dy });
    input
}

fn wheeled(notches: f32, look_button: bool) -> Input {
    let mut input = Input::default();
    input.apply(CameraInput::LookButton(look_button));
    input.apply(CameraInput::Wheel { notches });
    input
}

/// The pose a free-flying controller reports, for tests that then measure it.
fn pose(from: Viewpoint) -> Pose {
    match from {
        Viewpoint::Free(pose) => pose,
        Viewpoint::Orbit(yaw) => panic!("still orbiting at yaw {yaw}"),
    }
}

fn step(controller: &mut Controller, input: &mut Input, dt: Duration) -> Viewpoint {
    controller.update(input, dt, Duration::ZERO, &bounds())
}

#[test]
fn orbit_mode_ignores_input_until_the_first_trigger() {
    let mut controller = Controller::new(Start::Orbit, &bounds(), Some(30.0), 0.2);

    let from = controller.update(
        &mut Input::default(),
        Duration::from_millis(16),
        Duration::from_secs(2),
        &bounds(),
    );
    assert_eq!(
        from,
        Viewpoint::Orbit(Camera::orbit_yaw(Duration::from_secs(2)))
    );
}

#[test]
fn a_movement_key_switches_to_free_mode_without_a_jump() {
    let bounds = bounds();
    let mut controller = Controller::new(Start::Orbit, &bounds, Some(30.0), 0.2);
    let mut input = held(&[CameraKey::Forward]);

    let elapsed = Duration::from_millis(2500);
    let before = Camera::orbit_pose(&bounds, Camera::orbit_yaw(elapsed));
    let after = pose(controller.update(&mut input, Duration::ZERO, elapsed, &bounds));

    assert_eq!(after, before);
}

#[test]
fn the_default_mode_starts_free_at_the_spawn_pose() {
    let from = spawned().update(
        &mut Input::default(),
        Duration::ZERO,
        Duration::ZERO,
        &bounds(),
    );

    assert_eq!(from, Viewpoint::Free(Camera::spawn_pose(&bounds())));
}

// What an embedder opening a place at its own `Camera` gets: that exact pose,
// with nothing orbiting first.
#[test]
fn a_starting_pose_is_flown_from_as_given() {
    let start = Pose {
        position: Vec3::new(12.0, 34.0, -56.0),
        yaw: 0.7,
        pitch: -0.2,
        fov_degrees: 55.0,
    };
    let mut controller = Controller::new(Start::Pose(start), &bounds(), Some(30.0), 0.2);

    let from = step(&mut controller, &mut Input::default(), Duration::ZERO);

    assert_eq!(from, Viewpoint::Free(start));
}

// Idle detection, which is what lets an embedded viewport stop rendering: with
// nothing held and no deltas pending, the viewpoint never budges.
#[test]
fn an_untouched_camera_reports_the_same_viewpoint_every_tick() {
    let mut controller = spawned();
    let mut input = Input::default();
    let dt = Duration::from_millis(33);

    let first = step(&mut controller, &mut input, dt);
    assert_eq!(step(&mut controller, &mut input, dt), first);
    assert_eq!(step(&mut controller, &mut input, dt), first);
}

#[test]
fn a_held_key_or_a_mouse_look_moves_the_viewpoint_off_its_resting_place() {
    let dt = Duration::from_millis(33);
    let resting = step(&mut spawned(), &mut Input::default(), dt);

    for mut input in [held(&[CameraKey::Forward]), looking(5.0, 0.0)] {
        assert_ne!(step(&mut spawned(), &mut input, dt), resting);
    }
}

// A held look button with a still mouse must not count as movement, or an
// embedded view would redraw forever while the button rests down.
#[test]
fn holding_the_look_button_without_moving_the_mouse_changes_nothing() {
    let mut controller = spawned();
    let mut input = Input::default();
    input.apply(CameraInput::LookButton(true));
    let dt = Duration::from_millis(33);

    let first = step(&mut controller, &mut input, dt);
    assert_eq!(step(&mut controller, &mut input, dt), first);
}

// Velocity now eases towards its target instead of snapping, so a single-step
// distance no longer scales linearly with `dt` during that ramp-up. Once the
// ease has settled (well past `MOVEMENT_TIME_CONSTANT`'s ~180ms), steady-state
// distance-per-second must still match the old, un-smoothed behaviour.
#[test]
fn once_settled_moving_forward_covers_twice_the_distance_in_twice_the_time() {
    let distance_after_settling = |dt| {
        let mut controller = spawned();
        let mut input = held(&[CameraKey::Forward]);
        for _ in 0..50 {
            step(&mut controller, &mut input, Duration::from_millis(16));
        }
        let start = pose(step(&mut controller, &mut input, Duration::ZERO)).position;
        let moved = pose(step(&mut controller, &mut input, dt)).position;
        (moved - start).length()
    };

    let short = distance_after_settling(Duration::from_millis(100));
    let long = distance_after_settling(Duration::from_millis(200));
    assert!((long - 2.0 * short).abs() < 1e-2);
}

#[test]
fn shift_slows_movement_down() {
    let dt = Duration::from_millis(100);
    let start = Camera::spawn_pose(&bounds()).position;
    let distance = |keys: &[CameraKey]| {
        let moved = pose(step(&mut spawned(), &mut held(keys), dt));
        (moved.position - start).length()
    };

    let fast = distance(&[CameraKey::Forward]);
    let slow = distance(&[CameraKey::Forward, CameraKey::Slow]);
    assert!((slow - fast * SHIFT_SLOW_FACTOR).abs() < 1e-3);
}

#[test]
fn a_rightward_mouse_move_turns_the_view_right_not_left() {
    // Starting from yaw 0 (looking down -Z), Studio's convention is that dragging
    // the mouse right turns the view towards +X — the inverse-look regression.
    let moved = pose(step(
        &mut spawned(),
        &mut looking(10.0, 0.0),
        Duration::from_millis(16),
    ));

    assert!(look_direction(moved.yaw, moved.pitch).x > 0.0);
}

#[test]
fn moving_the_mouse_up_looks_up_not_down() {
    // `dy` grows downward, so "mouse up" is dy < 0; the look vector's vertical
    // component must grow, never shrink (no accidental Y inversion).
    let moved = pose(step(
        &mut spawned(),
        &mut looking(0.0, -10.0),
        Duration::from_millis(16),
    ));

    assert!(look_direction(moved.yaw, moved.pitch).y > 0.0);
}

#[test]
fn the_mouse_only_turns_the_view_while_the_look_button_is_held() {
    let mut input = Input::default();
    input.apply(CameraInput::MouseLook { dx: 50.0, dy: 50.0 });

    let moved = pose(step(&mut spawned(), &mut input, Duration::from_millis(16)));

    assert_eq!(moved, Camera::spawn_pose(&bounds()));
}

#[test]
fn d_strafes_right_and_a_strafes_left() {
    // The local movement vector only says "sideways"; which way that lands in world
    // space is the half that was inverted. From the spawn pose (looking -Z, Y up),
    // screen-right is +X.
    let start = Camera::spawn_pose(&bounds()).position;

    for (key, expected_sign) in [(CameraKey::Right, 1.0_f32), (CameraKey::Left, -1.0)] {
        let moved = pose(step(
            &mut spawned(),
            &mut held(&[key]),
            Duration::from_millis(100),
        ))
        .position
            - start;

        assert!(
            moved.x * expected_sign > 0.0,
            "{key:?} moved x by {}, expected sign {expected_sign}",
            moved.x
        );
        assert!(moved.z.abs() < 1e-4, "{key:?} should not move along z");
    }
}

#[test]
fn pitch_is_clamped_even_after_a_huge_mouse_swing() {
    let mut controller = spawned();
    let dt = Duration::from_millis(16);

    let up = pose(step(&mut controller, &mut looking(0.0, 1_000_000.0), dt));
    assert!((up.pitch.to_degrees() - MAX_PITCH_DEGREES).abs() < 1e-3);

    let down = pose(step(&mut controller, &mut looking(0.0, -1_000_000.0), dt));
    assert!((down.pitch.to_degrees() + MAX_PITCH_DEGREES).abs() < 1e-3);
}

#[test]
fn the_wheel_only_adjusts_speed_while_the_look_button_is_held() {
    let mut controller = spawned();

    step(
        &mut controller,
        &mut wheeled(1.0, true),
        Duration::from_millis(16),
    );

    assert!((controller.speed() - 30.0 * SPEED_STEP).abs() < 1e-3);
}

#[test]
fn speed_is_bounded_both_ways() {
    let mut controller = spawned();

    for _ in 0..200 {
        step(&mut controller, &mut wheeled(1.0, true), Duration::ZERO);
    }
    assert!((controller.speed() - MAX_SPEED).abs() < 1e-3);

    for _ in 0..200 {
        step(&mut controller, &mut wheeled(-1.0, true), Duration::ZERO);
    }
    assert!((controller.speed() - MIN_SPEED).abs() < 1e-3);
}

// A wheel notch now eases in as a smoothed dolly hop rather than teleporting
// in the same tick, so the full `speed * ZOOM_SECONDS_PER_NOTCH` distance only
// shows up once the ease has had time (many tau) to settle.
#[test]
fn the_wheel_without_the_look_button_translates_instead_of_changing_speed() {
    let mut controller = spawned();
    let start = Camera::spawn_pose(&bounds()).position;
    let mut input = wheeled(1.0, false);

    step(&mut controller, &mut input, Duration::ZERO); // registers the notch
    let mut settle = Input::default();
    let moved = pose(step(&mut controller, &mut settle, Duration::from_secs(1)));

    assert_eq!(controller.speed(), 30.0);
    // One notch: `speed * ZOOM_SECONDS_PER_NOTCH` forward, spawn looks -Z.
    let expected = Vec3::new(0.0, 0.0, -30.0 * ZOOM_SECONDS_PER_NOTCH);
    assert!(((moved.position - start) - expected).length() < 1e-3);
}

#[test]
fn wheel_translation_scales_with_the_number_of_notches() {
    // The eased dolly is a linear function of the notch-implied distance at
    // any fixed dt, so proportionality holds even mid-transient.
    let start = Camera::spawn_pose(&bounds()).position;
    let distance = |notches| {
        let moved = pose(step(
            &mut spawned(),
            &mut wheeled(notches, false),
            Duration::from_millis(16),
        ));
        (moved.position - start).length()
    };

    assert!((distance(3.0) - 3.0 * distance(1.0)).abs() < 1e-3);
}

#[test]
fn recentre_restores_the_spawn_view_while_staying_free() {
    let mut controller = spawned();
    step(
        &mut controller,
        &mut held(&[CameraKey::Forward]),
        Duration::from_secs(1),
    );

    controller.recentre(&bounds());

    let from = step(&mut controller, &mut Input::default(), Duration::ZERO);
    assert_eq!(from, Viewpoint::Free(Camera::spawn_pose(&bounds())));
}

#[test]
fn recentre_restores_the_initial_orbit_view_for_the_legacy_orbit_flag() {
    let bounds = bounds();
    let mut controller = Controller::new(Start::Orbit, &bounds, Some(30.0), 0.2);
    controller.update(
        &mut held(&[CameraKey::Forward]),
        Duration::from_secs(1),
        Duration::ZERO,
        &bounds,
    );

    controller.recentre(&bounds);

    let from = step(&mut controller, &mut Input::default(), Duration::ZERO);
    let expected = Camera::orbit_pose(&bounds, INITIAL_ORBIT_YAW);
    assert_eq!(pose(from).position, expected.position);
}

#[test]
fn default_speed_is_flat_for_a_modest_scene() {
    assert_eq!(default_speed(&bounds()), DEFAULT_SPEED);
}

#[test]
fn default_speed_scales_with_radius_for_a_huge_scene() {
    let huge = bounds_from(Vec3::splat(-2000.0), Vec3::splat(2000.0));
    assert!(huge.radius() > LARGE_SCENE_RADIUS);
    assert_eq!(
        default_speed(&huge),
        huge.radius() / LARGE_SCENE_SPEED_DIVISOR
    );
}

#[test]
fn a_scaled_default_speed_still_gets_clamped() {
    let enormous = bounds_from(Vec3::splat(-1.0e9), Vec3::splat(1.0e9));
    let controller = Controller::new(Start::Spawn, &enormous, None, 0.2);
    assert_eq!(controller.speed(), MAX_SPEED);
}

// The property that actually matters for frame-rate independence: whatever
// `dt` gets sliced into, the total ease over the same wall-clock time should
// land in the same place (`exp(-a) * exp(-b) == exp(-(a + b))`).
#[test]
fn easing_towards_a_target_is_frame_rate_independent() {
    let target = Vec3::new(10.0, -4.0, 2.0);

    let mut many_small_steps = Vec3::ZERO;
    for _ in 0..100 {
        many_small_steps = ease_towards(
            many_small_steps,
            target,
            Duration::from_millis(1),
            MOVEMENT_TIME_CONSTANT,
        );
    }
    let one_big_step = ease_towards(
        Vec3::ZERO,
        target,
        Duration::from_millis(100),
        MOVEMENT_TIME_CONSTANT,
    );

    assert!((many_small_steps - one_big_step).length() < 1e-4);
}

// Pins the actual jitter fix: the *position* covered while easing towards a
// target must sum to the same total regardless of how the same wall-clock
// interval is chopped into frames, not just the velocity `ease_towards`
// converges to. `new_velocity * dt` fails this (it only agrees with the exact
// integral in the limit of infinitely many, infinitely small steps), which is
// exactly the mismatch that reads as jitter under a wobbly frame time even
// while the average frame rate holds steady.
#[test]
fn easing_the_position_is_also_frame_rate_independent() {
    let target = Vec3::new(10.0, -4.0, 2.0);

    let mut position = Vec3::ZERO;
    let mut velocity = Vec3::ZERO;
    for _ in 0..100 {
        let (new_velocity, displacement) = ease_and_advance(
            velocity,
            target,
            Duration::from_millis(1),
            MOVEMENT_TIME_CONSTANT,
        );
        velocity = new_velocity;
        position += displacement;
    }

    let (_, one_big_displacement) = ease_and_advance(
        Vec3::ZERO,
        target,
        Duration::from_millis(100),
        MOVEMENT_TIME_CONSTANT,
    );

    assert!((position - one_big_displacement).length() < 1e-4);
}

// A frame time that varies around a steady average (e.g. 8ms/24ms alternating
// instead of a flat 16ms) must still cover the same ground as the steady rate
// once both have run for the same wall-clock time — the concrete case a
// stable average FPS with jittery per-frame timing represents.
#[test]
fn uneven_frame_times_cover_the_same_ground_as_even_ones() {
    let target = Vec3::new(10.0, -4.0, 2.0);
    let total = |frame_times: &[u64]| {
        let mut position = Vec3::ZERO;
        let mut velocity = Vec3::ZERO;
        for &millis in frame_times {
            let (new_velocity, displacement) = ease_and_advance(
                velocity,
                target,
                Duration::from_millis(millis),
                MOVEMENT_TIME_CONSTANT,
            );
            velocity = new_velocity;
            position += displacement;
        }
        position
    };

    let steady = total(&[16; 10]);
    let jittery = total(&[8, 24, 8, 24, 8, 24, 8, 24, 8, 24]);
    assert!((steady - jittery).length() < 1e-4);
}

#[test]
fn easing_never_overshoots_or_reverses_towards_the_target() {
    let current = ease_towards(
        Vec3::ZERO,
        Vec3::new(5.0, 0.0, 0.0),
        Duration::from_millis(16),
        MOVEMENT_TIME_CONSTANT,
    );
    assert!(current.x > 0.0 && current.x < 5.0);
}

// WASD movement should ramp in, not snap to full speed on the very first
// frame: this is the whole point of smoothing.
#[test]
fn the_first_frame_of_a_held_key_moves_less_than_the_unsmoothed_distance() {
    let dt = Duration::from_millis(100);
    let moved = pose(step(&mut spawned(), &mut held(&[CameraKey::Forward]), dt));
    let start = Camera::spawn_pose(&bounds()).position;
    let distance = (moved.position - start).length();

    let unsmoothed = 30.0 * dt.as_secs_f32();
    assert!(distance < unsmoothed);
}

// Releasing the key decelerates instead of stopping dead: one frame after
// release, the camera should still be coasting from its eased-in velocity.
#[test]
fn releasing_the_key_coasts_instead_of_stopping_instantly() {
    let mut controller = spawned();
    let dt = Duration::from_millis(16);
    for _ in 0..20 {
        step(&mut controller, &mut held(&[CameraKey::Forward]), dt);
    }
    let before_release = pose(step(&mut controller, &mut Input::default(), Duration::ZERO));

    let after_release = pose(step(&mut controller, &mut Input::default(), dt));

    assert_ne!(after_release.position, before_release.position);
}
