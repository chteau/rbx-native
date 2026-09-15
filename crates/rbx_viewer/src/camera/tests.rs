use super::*;
use crate::scene::tests_support::bounds_from;

fn orbit(yaw: f32) -> Viewpoint {
    Viewpoint::Orbit(yaw)
}

#[test]
fn distance_follows_the_scene_radius_but_never_collapses() {
    let wide = Camera::framing(&bounds_from(Vec3::splat(-100.0), Vec3::splat(100.0)));
    let tiny = Camera::framing(&bounds_from(Vec3::ZERO, Vec3::splat(0.3)));

    let radius = (Vec3::splat(200.0)).length() * 0.5;
    assert!((wide.distance - radius * 2.0).abs() < 1e-3);
    assert_eq!(tiny.distance, MIN_DISTANCE);
}

#[test]
fn the_target_lands_in_the_middle_of_the_frame() {
    let bounds = bounds_from(Vec3::new(-10.0, -2.0, -10.0), Vec3::new(10.0, 6.0, 10.0));
    let camera = Camera::framing(&bounds);

    let clip = camera.view_projection(orbit(0.7), 16.0 / 9.0) * bounds.center().extend(1.0);

    assert!(clip.w > 0.0);
    assert!((clip.x / clip.w).abs() < 1e-5);
    assert!((clip.y / clip.w).abs() < 1e-5);
    assert!((0.0..=1.0).contains(&(clip.z / clip.w)));
}

// The framing promise: at any yaw, every corner of the bounding box stays on screen.
#[test]
fn every_corner_stays_inside_the_frustum() {
    let (min, max) = (
        Vec3::new(-1024.0, -16.0, -1024.0),
        Vec3::new(1024.0, 1.0, 1024.0),
    );
    let camera = Camera::framing(&bounds_from(min, max));

    for step in 0..8 {
        let yaw = Camera::orbit_yaw(Duration::from_secs(step));
        let view_projection = camera.view_projection(orbit(yaw), 16.0 / 9.0);

        for corner in [min, max] {
            let clip = view_projection * corner.extend(1.0);
            assert!(clip.w > 0.0, "corner behind the camera at step {step}");

            let ndc = clip.truncate() / clip.w;
            assert!(
                ndc.x.abs() <= 1.001 && ndc.y.abs() <= 1.001,
                "{ndc} at {step}"
            );
            assert!((0.0..=1.0).contains(&ndc.z), "{ndc} at {step}");
        }
    }
}

// The skybox promise: turning the camera turns the sky, moving it does not.
#[test]
fn the_rotation_only_view_ignores_where_the_camera_stands() {
    let near = Camera::framing(&bounds_from(Vec3::ZERO, Vec3::splat(10.0)));
    let far = Camera::framing(&bounds_from(Vec3::ZERO, Vec3::splat(1000.0)));

    // Only x and y are compared: the skybox shader throws the depth away and
    // pins every corner to the far plane. (The projection no longer depends on
    // scene scale at all since reversed-Z gave it a fixed near and an infinite
    // far, so the two cameras' matrices are now identical outright — this test
    // still earns its keep by locking down that x/y only depends on rotation.)
    let screen = |clip: glam::Vec4| clip.truncate().truncate() / clip.w;
    let direction = Vec3::new(0.3, -0.2, 0.9).normalize();
    let a = near.view_rotation_projection(orbit(0.7), 1.5) * direction.extend(1.0);
    let b = far.view_rotation_projection(orbit(0.7), 1.5) * direction.extend(1.0);

    assert!(screen(a).abs_diff_eq(screen(b), 1e-4), "{a} vs {b}");
    // And a different yaw must not land in the same place.
    let turned = near.view_rotation_projection(orbit(1.9), 1.5) * direction.extend(1.0);
    assert!(!screen(a).abs_diff_eq(screen(turned), 1e-3));
}

// A real regression this once was: sky/star/sun geometry (`renderer::sky`/
// `stars.wgsl`/`sun.wgsl`) is built at near-unit magnitude, relying on the
// perspective divide to spread it across the screen regardless of its actual
// size. `view_rotation_projection` staying perspective even when the main
// camera has gone orthographic (see its own doc comment) is what keeps that
// working — an earlier version branched on `self.orthographic` here too and
// collapsed every unit-magnitude vertex to a single point at screen centre,
// leaving the sky a solid black void.
#[test]
fn orthographic_rotation_only_view_still_spreads_unit_geometry_across_the_screen() {
    let camera =
        Camera::framing(&bounds_from(Vec3::ZERO, Vec3::splat(20.0))).with_orthographic(true);
    let aspect = 16.0 / 9.0;
    let view_projection = camera.view_rotation_projection(orbit(0.0), aspect);

    // A corner of the unit "sky cube" (see `textures::sky::quad`) — the point
    // being tested is that this reaches *somewhere* well off centre, not that
    // any one particular direction does.
    let corner = Vec3::new(1.0, 1.0, 1.0);
    let clip = view_projection * corner.extend(1.0);
    let ndc = clip.truncate() / clip.w;

    assert!(
        ndc.x.abs() > 0.1 || ndc.y.abs() > 0.1,
        "a unit-magnitude vertex collapsed to near screen centre: {ndc}"
    );
}

// `view_rotation_projection` must ignore the main camera's orthographic flag
// entirely — it always matches what an ordinary perspective camera at this
// yaw/pitch would produce, main camera notwithstanding.
#[test]
fn orthographic_rotation_only_view_matches_the_ordinary_perspective_one() {
    let perspective = Camera::framing(&bounds_from(Vec3::ZERO, Vec3::splat(20.0)));
    let orthographic = perspective.with_orthographic(true);
    let aspect = 16.0 / 9.0;

    assert_eq!(
        perspective.view_rotation_projection(orbit(0.6), aspect),
        orthographic.view_rotation_projection(orbit(0.6), aspect)
    );
}

// The whole point of reversed-Z: depth must grow, not shrink, as a point gets
// closer to the eye, so a small fixed near plane never runs out of precision.
#[test]
fn reversed_depth_grows_as_a_point_gets_closer() {
    let camera = Camera::framing(&bounds_from(Vec3::ZERO, Vec3::splat(20.0)));
    let yaw = 0.3;
    let view_projection = camera.view_projection(orbit(yaw), 16.0 / 9.0);
    let eye = camera.eye(yaw);

    let depth = |point: Vec3| {
        let clip = view_projection * point.extend(1.0);
        clip.z / clip.w
    };

    let close = eye + (camera.target - eye) * 0.1;
    let far = eye + (camera.target - eye) * 0.9;
    assert!(
        depth(close) > depth(far),
        "{} vs {}",
        depth(close),
        depth(far)
    );
}

#[test]
fn look_at_pose_stands_at_eye_and_faces_the_target() {
    let eye = Vec3::new(-76.0, 46.0, -55.0);
    let target = Vec3::new(-76.0, 46.0, -57.0);

    let pose = look_at_pose(eye, target);

    assert_eq!(pose.position, eye);
    let looked = direction(pose.yaw, pose.pitch);
    assert!(
        looked.abs_diff_eq((target - eye).normalize(), 1e-5),
        "{looked}"
    );
}

#[test]
fn look_at_pose_falls_back_instead_of_producing_nan_when_degenerate() {
    let pose = look_at_pose(Vec3::ZERO, Vec3::ZERO);

    assert!(direction(pose.yaw, pose.pitch).is_finite());
}

#[test]
fn the_screenshot_yaw_can_be_overridden_in_degrees() {
    assert_eq!(
        Camera::screenshot_yaw(None),
        SCREENSHOT_YAW_DEGREES.to_radians()
    );
    assert_eq!(Camera::screenshot_yaw(Some(180.0)), std::f32::consts::PI);
}

#[test]
fn a_negative_pitch_puts_the_eye_below_the_target() {
    let camera = Camera::framing(&bounds_from(Vec3::ZERO, Vec3::splat(10.0)));

    assert!(camera.eye(0.0).y > camera.target.y);
    assert!(camera.pitched(-40.0).eye(0.0).y < camera.target.y);
}

#[test]
fn a_full_turn_takes_eight_seconds() {
    assert_eq!(Camera::orbit_yaw(Duration::ZERO), 0.0);
    assert!((Camera::orbit_yaw(Duration::from_secs(8)) - std::f32::consts::TAU).abs() < 1e-5);
}

#[test]
fn orbit_pose_sits_exactly_where_the_orbit_camera_would() {
    let bounds = bounds_from(Vec3::ZERO, Vec3::splat(20.0));
    let camera = Camera::framing(&bounds);
    let pose = Camera::orbit_pose(&bounds, 1.3);

    assert_eq!(pose.position, camera.eye(1.3));
    assert_eq!(pose.yaw, 1.3);
    assert_eq!(pose.pitch, camera.pitch);
}

// The hand-off promise: the very first free-camera frame, seeded from the
// orbit camera's own pose, must render identically to one more orbit frame.
#[test]
fn switching_to_free_mode_does_not_jump_the_view() {
    let bounds = bounds_from(Vec3::new(-5.0, 0.0, -5.0), Vec3::new(5.0, 3.0, 5.0));
    let camera = Camera::framing(&bounds);
    let yaw = 0.9;
    let aspect = 16.0 / 9.0;

    let orbit_view = camera.view_projection(orbit(yaw), aspect);
    let orbit_sky = camera.view_rotation_projection(orbit(yaw), aspect);

    let free = Viewpoint::Free(Camera::orbit_pose(&bounds, yaw));
    let free_view = camera.view_projection(free, aspect);
    let free_sky = camera.view_rotation_projection(free, aspect);

    for (a, b) in orbit_view
        .to_cols_array()
        .iter()
        .zip(free_view.to_cols_array().iter())
    {
        assert!((a - b).abs() < 1e-4, "{orbit_view:?} vs {free_view:?}");
    }
    for (a, b) in orbit_sky
        .to_cols_array()
        .iter()
        .zip(free_sky.to_cols_array().iter())
    {
        assert!((a - b).abs() < 1e-4, "{orbit_sky:?} vs {free_sky:?}");
    }
}

// A free pose stands where it says it stands, whatever the orbit framing would
// have picked for the same scene.
#[test]
fn a_free_viewpoint_puts_the_eye_at_its_own_position() {
    let camera = Camera::framing(&bounds_from(Vec3::ZERO, Vec3::splat(50.0)));
    let pose = Pose {
        position: Vec3::new(-3.0, 7.0, 11.0),
        yaw: 0.4,
        pitch: 0.1,
        fov_degrees: 70.0,
    };

    assert_eq!(camera.eye_position(Viewpoint::Free(pose)), pose.position);
    assert_eq!(camera.eye_position(orbit(0.4)), camera.eye(0.4));
}

// A free pose's own field of view must reach the projection matrix, and a pose
// carrying the crate's default must render exactly like the pre-existing constant
// did — no behavioral change for the orbit path or any caller that doesn't set one.
#[test]
fn a_free_pose_field_of_view_changes_the_projection_but_the_default_does_not() {
    let camera = Camera::framing(&bounds_from(Vec3::ZERO, Vec3::splat(20.0)));
    let aspect = 16.0 / 9.0;
    let default_pose = Pose {
        position: Vec3::new(1.0, 2.0, 3.0),
        yaw: 0.2,
        pitch: 0.1,
        fov_degrees: FIELD_OF_VIEW_DEGREES,
    };
    let narrow_pose = Pose {
        fov_degrees: 40.0,
        ..default_pose
    };

    let default_matrix = camera.view_projection(Viewpoint::Free(default_pose), aspect);
    let narrow_matrix = camera.view_projection(Viewpoint::Free(narrow_pose), aspect);

    assert_ne!(default_matrix, narrow_matrix);
    assert_eq!(
        default_matrix,
        camera.projection(
            aspect,
            FIELD_OF_VIEW_DEGREES,
            camera.orthographic_distance(Viewpoint::Free(default_pose)),
        ) * look_to_mat4(
            default_pose.position,
            direction(default_pose.yaw, default_pose.pitch),
            Vec3::Y
        )
    );
}

#[test]
fn spawn_pose_stands_over_the_center_looking_flat_down_minus_z() {
    let bounds = bounds_from(Vec3::new(-10.0, -2.0, -10.0), Vec3::new(10.0, 6.0, 10.0));
    let pose = Camera::spawn_pose(&bounds);

    assert_eq!(pose.yaw, 0.0);
    assert_eq!(pose.pitch, 0.0);
    assert_eq!(pose.position.x, bounds.center().x);
    assert_eq!(pose.position.z, bounds.center().z);
    assert!(pose.position.y > bounds.center().y);
}

#[test]
fn spawn_elevation_has_a_floor_for_a_tiny_scene() {
    let bounds = bounds_from(Vec3::ZERO, Vec3::splat(1.0));
    let pose = Camera::spawn_pose(&bounds);

    assert!((pose.position.y - bounds.center().y - MIN_SPAWN_ELEVATION).abs() < 1e-4);
}

#[test]
fn spawn_elevation_scales_with_radius_for_a_big_scene() {
    let bounds = bounds_from(Vec3::splat(-1000.0), Vec3::splat(1000.0));
    let pose = Camera::spawn_pose(&bounds);

    let expected = bounds.radius() * SPAWN_ELEVATION_FRACTION;
    assert!(expected > MIN_SPAWN_ELEVATION);
    assert!((pose.position.y - bounds.center().y - expected).abs() < 1e-2);
}

// The depth-of-field term in `post.wgsl` reconstructs a distance from the depth
// buffer with nothing but the near plane, so the inverse has to hold against the
// very matrix the frame was drawn with — not against an algebraic rearrangement
// of it, which is exactly where a sign or a flipped near/far would hide.
#[test]
fn a_depth_buffer_value_reconstructs_the_distance_it_was_written_from() {
    let camera = Camera::framing(&bounds_from(Vec3::ZERO, Vec3::splat(20.0)));
    let pose = look_at_pose(Vec3::new(0.0, 0.0, 100.0), Vec3::ZERO);
    let view_projection = camera.view_projection(Viewpoint::Free(pose), 16.0 / 9.0);

    // Straight down the view axis (-Z from the eye), which is what the
    // reconstruction answers: the distance to the plane a pixel sits on, not the
    // radial distance to the eye.
    for studs in [NEAR_PLANE, 0.5, 1.0, 12.5, 100.0, 5_000.0] {
        let point = pose.position - Vec3::Z * studs;
        let clip = view_projection * point.extend(1.0);
        let depth = clip.z / clip.w;

        assert!(
            (view_distance(depth) - studs).abs() < studs * 1e-4,
            "{studs} studs landed on depth {depth}, read back as {}",
            view_distance(depth)
        );
    }
}

// Both ends of the range the projection maps: the near plane is depth 1, and an
// infinite far plane is depth 0.
#[test]
fn the_near_plane_is_depth_one_and_the_horizon_is_depth_zero() {
    assert!((reversed_depth(NEAR_PLANE) - 1.0).abs() < 1e-6);
    assert!(reversed_depth(1.0e9) < 1e-6);
    assert!((view_distance(1.0) - NEAR_PLANE).abs() < 1e-9);
}

// A pixel nothing was drawn into keeps the depth clear value of 0, and reading
// that back as "at the eye" would blur the sky as if it were in front of the
// focus plane. It has to come out maximally far instead.
#[test]
fn a_cleared_pixel_reads_as_maximally_far_rather_than_as_touching_the_eye() {
    assert_eq!(view_distance(0.0), BACKGROUND_DISTANCE);
    assert_eq!(view_distance(-1.0), BACKGROUND_DISTANCE);
    assert!(view_distance(1e-7) > 100_000.0);
}

#[test]
fn with_orthographic_only_flips_the_projection_flag() {
    let camera = Camera::framing(&bounds_from(Vec3::ZERO, Vec3::splat(20.0)));
    assert!(!camera.orthographic);

    let toggled = camera.with_orthographic(true);
    assert!(toggled.orthographic);
    // The framing itself (what the orbit/free controllers actually fly
    // around) must survive the toggle untouched — the whole point of reusing
    // it rather than inventing a second, ortho-specific notion of zoom.
    assert_eq!(toggled.target, camera.target);
    assert_eq!(toggled.distance, camera.distance);
    assert_eq!(toggled.pitch, camera.pitch);
}

#[test]
fn orthographic_far_plane_is_none_until_orthographic_is_on() {
    let camera = Camera::framing(&bounds_from(Vec3::ZERO, Vec3::splat(20.0)));
    assert_eq!(camera.orthographic_far_plane(orbit(0.0)), None);

    let far = camera
        .with_orthographic(true)
        .orthographic_far_plane(orbit(0.0))
        .expect("an orthographic camera reports a finite far plane");
    assert!((far - camera.distance * ORTHOGRAPHIC_FAR_MULTIPLIER).abs() < 1e-3);
}

// The orbit camera already has an exact distance to its own target by
// construction — `orthographic_distance` must reuse it verbatim rather than
// deriving a second, possibly-different number.
#[test]
fn orthographic_distance_for_orbit_is_exactly_the_framing_distance() {
    let camera = Camera::framing(&bounds_from(Vec3::ZERO, Vec3::splat(20.0)));
    assert_eq!(camera.orthographic_distance(orbit(0.5)), camera.distance);
}

// A real regression: `orthographic_distance` used to read a fixed field
// (`self.distance`, set once from the whole scene's bounding sphere at load
// time) regardless of a free pose's actual position, so orthographic mode's
// apparent scale never changed as the free camera flew closer to or farther
// from anything — the root cause behind both "the view is kinda far" and
// "flying closer cuts through parts with no warning" (see this method's own
// doc comment). A free pose must instead track its own current distance from
// the scene's framing centre.
#[test]
fn orthographic_distance_tracks_the_free_camera_not_the_scenes_framing_distance() {
    let bounds = bounds_from(Vec3::splat(-500.0), Vec3::splat(500.0));
    let camera = Camera::framing(&bounds);
    let base_pose = Pose {
        position: bounds.center(),
        yaw: 0.0,
        pitch: 0.0,
        fov_degrees: FIELD_OF_VIEW_DEGREES,
    };

    let close = Pose {
        position: bounds.center() + Vec3::new(0.0, 0.0, 12.0),
        ..base_pose
    };
    let far = Pose {
        position: bounds.center() + Vec3::new(0.0, 0.0, 300.0),
        ..base_pose
    };

    let close_distance = camera.orthographic_distance(Viewpoint::Free(close));
    let far_distance = camera.orthographic_distance(Viewpoint::Free(far));

    assert!((close_distance - 12.0).abs() < 1e-3, "{close_distance}");
    assert!((far_distance - 300.0).abs() < 1e-3, "{far_distance}");
    // Neither matches the scene's own framing distance, which a stale
    // implementation would have returned for both regardless of pose.
    assert_ne!(close_distance, camera.distance);
    assert_ne!(far_distance, camera.distance);
}

// Same floor the orbit camera already applies to its own distance (see
// `MIN_DISTANCE`'s doc comment): a free pose sitting exactly on the framing
// centre must not collapse the orthographic view volume to zero size.
#[test]
fn orthographic_distance_floors_at_min_distance_for_a_free_pose_on_the_target() {
    let camera = Camera::framing(&bounds_from(Vec3::ZERO, Vec3::splat(20.0)));
    let at_target = Pose {
        position: camera.target,
        yaw: 0.0,
        pitch: 0.0,
        fov_degrees: FIELD_OF_VIEW_DEGREES,
    };

    assert_eq!(
        camera.orthographic_distance(Viewpoint::Free(at_target)),
        MIN_DISTANCE
    );
}

// The end-to-end promise `orthographic_distance` exists to keep: a fixed
// world-space offset from the scene's centre must cover a very different
// fraction of the frame depending on whether the free camera is actually
// close to it or far away — not the same fraction either way, which is what
// a stale, scene-wide framing distance would have produced regardless of
// where the camera flew.
#[test]
fn the_orthographic_view_volume_shrinks_as_the_free_camera_flies_closer() {
    let bounds = bounds_from(Vec3::splat(-500.0), Vec3::splat(500.0));
    let camera = Camera::framing(&bounds).with_orthographic(true);
    let aspect = 16.0 / 9.0;
    let base_pose = Pose {
        position: bounds.center(),
        yaw: 0.0,
        pitch: 0.0,
        fov_degrees: FIELD_OF_VIEW_DEGREES,
    };

    let near_pose = Pose {
        position: bounds.center() + Vec3::new(0.0, 0.0, 10.0),
        ..base_pose
    };
    let far_pose = Pose {
        position: bounds.center() + Vec3::new(0.0, 0.0, 200.0),
        ..base_pose
    };

    let probe = bounds.center() + Vec3::new(5.0, 0.0, 0.0);
    let ndc_x = |pose: Pose| {
        let clip = camera.view_projection(Viewpoint::Free(pose), aspect) * probe.extend(1.0);
        (clip.x / clip.w).abs()
    };

    let near_ndc = ndc_x(near_pose);
    let far_ndc = ndc_x(far_pose);
    assert!(
        near_ndc > far_ndc * 5.0,
        "close: {near_ndc}, far: {far_ndc}"
    );
}

// Parallel projection's whole point: a lateral offset maps to the same NDC
// position however far down the view axis it sits, unlike perspective's
// foreshortening. `w` staying fixed at 1 (no perspective divide) is what
// makes that true.
#[test]
fn orthographic_projection_does_not_foreshorten_with_distance() {
    let camera =
        Camera::framing(&bounds_from(Vec3::ZERO, Vec3::splat(20.0))).with_orthographic(true);
    let pose = look_at_pose(Vec3::new(0.0, 0.0, 100.0), Vec3::ZERO);
    let view_projection = camera.view_projection(Viewpoint::Free(pose), 16.0 / 9.0);

    let near_point = Vec3::new(5.0, 0.0, 90.0);
    let far_point = Vec3::new(5.0, 0.0, 0.0);

    let near_clip = view_projection * near_point.extend(1.0);
    let far_clip = view_projection * far_point.extend(1.0);

    assert!((near_clip.w - 1.0).abs() < 1e-5, "{near_clip}");
    assert!((far_clip.w - 1.0).abs() < 1e-5, "{far_clip}");
    assert!(
        ((near_clip.x / near_clip.w) - (far_clip.x / far_clip.w)).abs() < 1e-4,
        "{near_clip} vs {far_clip}"
    );
}

// The orthographic mirror of `the_near_plane_is_depth_one_and_the_horizon_is_depth_zero`:
// linear rather than hyperbolic, and a finite far plane instead of an infinite one.
#[test]
fn the_orthographic_near_plane_is_depth_one_and_the_far_plane_is_depth_zero() {
    let far = 500.0;
    assert!((orthographic_reversed_depth(NEAR_PLANE, NEAR_PLANE, far) - 1.0).abs() < 1e-6);
    assert!(orthographic_reversed_depth(far, NEAR_PLANE, far).abs() < 1e-6);
}

// The orthographic mirror of `a_depth_buffer_value_reconstructs_the_distance_it_was_written_from`.
#[test]
fn an_orthographic_depth_buffer_value_reconstructs_the_distance_it_was_written_from() {
    let camera =
        Camera::framing(&bounds_from(Vec3::ZERO, Vec3::splat(20.0))).with_orthographic(true);
    let pose = look_at_pose(Vec3::new(0.0, 0.0, 100.0), Vec3::ZERO);
    let view_projection = camera.view_projection(Viewpoint::Free(pose), 16.0 / 9.0);
    let far = camera
        .orthographic_far_plane(Viewpoint::Free(pose))
        .expect("camera is orthographic");

    for studs in [NEAR_PLANE, 0.5, 1.0, 12.5, 100.0] {
        let point = pose.position - Vec3::Z * studs;
        let clip = view_projection * point.extend(1.0);
        let depth = clip.z / clip.w;
        let reconstructed = orthographic_view_distance(depth, NEAR_PLANE, far);

        assert!(
            (reconstructed - studs).abs() < studs.max(1.0) * 1e-3,
            "{studs} studs landed on depth {depth}, read back as {reconstructed}"
        );
    }
}

// The orthographic mirror of `every_corner_stays_inside_the_frustum`: the same
// framing must still keep a wide, flat scene entirely on screen once the
// projection becomes parallel instead of converging.
#[test]
fn every_corner_stays_inside_the_orthographic_frustum() {
    let (min, max) = (
        Vec3::new(-1024.0, -16.0, -1024.0),
        Vec3::new(1024.0, 1.0, 1024.0),
    );
    let camera = Camera::framing(&bounds_from(min, max)).with_orthographic(true);

    for step in 0..8 {
        let yaw = Camera::orbit_yaw(Duration::from_secs(step));
        let view_projection = camera.view_projection(orbit(yaw), 16.0 / 9.0);

        for corner in [min, max] {
            let clip = view_projection * corner.extend(1.0);
            assert!(
                (clip.w - 1.0).abs() < 1e-4,
                "orthographic w must stay fixed at 1, got {} at step {step}",
                clip.w
            );

            let ndc = clip.truncate();
            assert!(
                ndc.x.abs() <= 1.001 && ndc.y.abs() <= 1.001,
                "{ndc} at {step}"
            );
            assert!((0.0..=1.0).contains(&ndc.z), "{ndc} at {step}");
        }
    }
}

// `frustum_corners` must pick the projection-matching depth formula for its far
// corners (see its own doc comment) — an orthographic main camera's shadow
// fitting would otherwise unproject them to the wrong world position. Checked
// against the actual requested distance down the view axis, not just against
// the matrix's own self-consistency: plugging the wrong (perspective) depth
// into the right inverse matrix still unprojects to *a* point, just not the
// one `distance` asked for, which reprojecting through the same matrix could
// never catch.
#[test]
fn frustum_corners_places_the_far_corners_at_the_requested_distance_when_orthographic() {
    let camera =
        Camera::framing(&bounds_from(Vec3::ZERO, Vec3::splat(20.0))).with_orthographic(true);
    let aspect = 16.0 / 9.0;
    let yaw = 0.4;
    let shadow_distance = 50.0;
    let eye = camera.eye_position(orbit(yaw));
    let forward = direction(yaw, camera.pitch);

    let corners = camera.frustum_corners(orbit(yaw), aspect, shadow_distance);

    // Indices 4..8 are the far corners (bit 2 set — see `frustum_corners`'s
    // own `index & 0b100` check).
    for corner in &corners[4..8] {
        let along_view_axis = (*corner - eye).dot(forward);
        assert!(
            (along_view_axis - shadow_distance).abs() < shadow_distance * 0.01,
            "{corner} landed {along_view_axis} studs down the view axis, expected {shadow_distance}"
        );
    }
}
