use super::*;
use crate::gizmo::basis;

fn seen_from(eye: Vec3) -> Pose {
    crate::camera::look_at_pose(eye, Vec3::ZERO)
}

fn handles(skin: Skin) -> Handles {
    Handles::new(Vec3::ZERO, basis(None), 3.0).skinned(
        skin,
        seen_from(Vec3::new(10.0, 10.0, 10.0)),
        false,
        false,
    )
}

fn ray_to(from: Vec3, to: Vec3) -> Ray {
    Ray {
        origin: from,
        direction: (to - from).normalize(),
    }
}

#[test]
fn next_gen_handles_take_studios_measured_size() {
    // Studio's screenshot: a cube 28.1 studs off, arrows reaching 4.96
    // studs and rings 3.87 round.
    let pose = seen_from(Vec3::new(18.0, 12.0, 18.0));
    let at = |rings| {
        Handles::new(Vec3::ZERO, basis(None), 3.0)
            .skinned(Skin::NextGen, pose, false, rings)
            .arm()
    };
    assert!((at(false) - 4.96).abs() < 0.02, "{}", at(false));
    assert!((at(true) - 3.87).abs() < 0.02, "{}", at(true));
    assert_eq!(handles(Skin::Classic).arm(), 3.0);
    let flat = Handles::new(Vec3::ZERO, basis(None), 3.0).skinned(Skin::NextGen, pose, true, false);
    assert!((flat.arm() - 1.98).abs() < 1e-5);
}

#[test]
fn next_gen_grabs_the_positive_arrows_only() {
    let eye = Vec3::new(0.0, 0.0, 20.0);
    for skin in [Skin::Classic, Skin::NextGen] {
        let handles = handles(skin);
        let tip = handles.arm() * 0.8;
        let plus = handles.grab_arm(ray_to(eye, Vec3::new(tip, 0.0, 0.0)));
        let minus = handles.grab_arm(ray_to(eye, Vec3::new(-tip, 0.0, 0.0)));
        assert_eq!(plus, Some((Axis::X, 1.0)));
        assert_eq!(minus.is_some(), skin == Skin::Classic, "{skin:?}");
    }
}

#[test]
fn a_plane_square_is_grabbed_by_the_axis_square_to_it() {
    let handles = handles(Skin::NextGen);
    let (_, _, centre) = handles.plane(Axis::Z);
    let eye = Vec3::new(0.0, 0.0, 20.0);
    assert_eq!(handles.grab_plane(ray_to(eye, centre)), Some(Axis::Z));
    assert_eq!(
        Handles::new(Vec3::ZERO, basis(None), 3.0).grab_plane(ray_to(eye, centre)),
        None
    );
}

#[test]
fn next_gen_rings_are_grabbed_on_their_near_half_only() {
    let handles = handles(Skin::NextGen);
    let eye = Vec3::new(0.0, 20.0, 20.0);
    let radius = RING_RADIUS * handles.arm();
    // The Y ring lies flat: its near point faces the camera, its far one
    // is behind the centre.
    let near = handles.grab_ring(ray_to(eye, Vec3::new(0.0, 0.0, radius)));
    let far = handles.grab_ring(ray_to(eye, Vec3::new(0.0, 0.0, -radius)));
    assert_eq!(near, Some(Axis::Y));
    assert_eq!(far, None);
}

#[test]
fn the_view_ring_stands_outside_the_axis_rings_square_to_the_camera() {
    let handles = handles(Skin::NextGen);
    let (normal, ..) = handles.view_frame();
    assert!((normal - Vec3::splat(-1.0).normalize()).length() < 1e-5);
    let eye = Vec3::new(10.0, 10.0, 10.0);
    let (_, zero, _) = handles.view_frame();
    let on = zero * VIEW_RING_RADIUS * handles.arm();
    assert!(handles.grab_view_ring(ray_to(eye, on)));
    assert!(!handles.grab_view_ring(ray_to(eye, zero * 0.5 * handles.arm())));
}

#[test]
fn the_trackball_is_the_sphere_inside_the_rings() {
    let handles = handles(Skin::NextGen);
    let eye = Vec3::new(0.0, 0.0, 20.0);
    let hit = handles.grab_trackball(ray_to(eye, Vec3::ZERO)).unwrap();
    assert!((hit - Vec3::Z).length() < 1e-5);
    assert!(handles
        .grab_trackball(ray_to(eye, Vec3::new(5.0, 0.0, 0.0)))
        .is_none());
    let rim = trackball_point(Vec3::ZERO, 1.0, ray_to(eye, Vec3::new(5.0, 0.0, 0.0)));
    assert!((rim.length() - 1.0).abs() < 1e-5);
}

#[test]
fn next_gen_colours_are_studios_swatches() {
    assert_eq!(Skin::Classic.color(Axis::Y), Axis::Y.color());
    assert_eq!(Skin::NextGen.color(Axis::Z), [0.058, 0.212, 0.982]);
}
