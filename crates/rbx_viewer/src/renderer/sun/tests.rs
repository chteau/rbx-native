use super::*;
use crate::assets::Image;
use crate::textures::Body;

fn celestial(angular_size: f32, toward_sun: bool) -> Celestial {
    Celestial {
        reference: rbx_assets::AssetRef::Id(1),
        image: std::sync::Arc::new(Image {
            width: 1,
            height: 1,
            pixels: vec![255; 4],
        }),
        body: Body {
            angular_size,
            toward_sun,
        },
    }
}

#[test]
fn a_disc_spans_the_tangent_of_half_its_angular_size() {
    // Roblox's default sun is 21 degrees across, which is a much bigger disc
    // than the real thing — 0.185 either side of a unit direction.
    let corners = quad(&celestial(21.0, true));

    assert!((corners[0].extent[0] - 0.1853).abs() < 1e-3);
    assert!(corners
        .iter()
        .all(|vertex| vertex.corner[0].abs() == 1.0 && vertex.corner[1].abs() == 1.0));
}

#[test]
fn the_moon_sits_on_the_other_end_of_the_light_direction() {
    assert_eq!(quad(&celestial(11.0, true))[0].extent[1], 1.0);
    assert_eq!(quad(&celestial(11.0, false))[0].extent[1], -1.0);
}

#[test]
fn the_corners_come_out_in_the_order_the_shared_winding_expects() {
    let corners = quad(&celestial(21.0, true));

    // Top-left, top-right, bottom-right, bottom-left, matching every other
    // quad in the renderer, with v running down the image.
    assert_eq!(corners[0].uv, [0.0, 0.0]);
    assert_eq!(corners[1].corner, [1.0, 1.0]);
    assert_eq!(corners[2].uv, [1.0, 1.0]);
    assert_eq!(corners[3].corner, [-1.0, -1.0]);
}

#[test]
fn every_vertex_fits_inside_its_own_stride() {
    let stride = std::mem::size_of::<Vertex>() as wgpu::BufferAddress;

    assert_eq!(stride, 24);
    for attribute in ATTRIBUTES {
        assert!(attribute.offset + attribute.format.size() <= stride);
    }
}

/// Built with the exact same two glam functions `Camera::view_rotation_projection`
/// composes, at yaw 0 / pitch 0 (forward is -Z there — see `camera::direction`),
/// so a test here stays honest about what the real matrix looks like without
/// reaching into `camera.rs`'s own private fields.
fn forward_looking_matrix() -> Mat4 {
    use glam::camera::rh::proj::directx::perspective_infinite_reverse;
    use glam::camera::rh::view::look_to_mat4;

    let view = look_to_mat4(Vec3::ZERO, Vec3::new(0.0, 0.0, -1.0), Vec3::Y);
    perspective_infinite_reverse(70f32.to_radians(), 1.0, 0.05) * view
}

#[test]
fn a_direction_the_camera_is_looking_straight_at_lands_in_the_middle_of_the_frame() {
    let matrix = forward_looking_matrix();

    let uv = project_direction(&matrix, Vec3::new(0.0, 0.0, -1.0)).expect("in front");
    assert!((uv - Vec2::new(0.5, 0.5)).length() < 1e-5, "{uv:?}");
}

#[test]
fn a_direction_behind_the_camera_projects_to_nothing() {
    let matrix = forward_looking_matrix();

    assert_eq!(project_direction(&matrix, Vec3::new(0.0, 0.0, 1.0)), None);
}

#[test]
fn a_direction_off_to_the_side_still_projects_somewhere_in_front() {
    let matrix = forward_looking_matrix();

    // 30 degrees off dead-ahead, well inside the 70 degree field of view.
    let direction = Vec3::new(30f32.to_radians().sin(), 0.0, -30f32.to_radians().cos());
    let uv = project_direction(&matrix, direction).expect("in front");
    assert!(uv.x > 0.5, "{uv:?}");
}

#[test]
fn the_sun_below_the_horizon_never_gets_a_screen_position() {
    let matrix = forward_looking_matrix();

    // Straight ahead in every axis but Y, which is what would normally
    // land dead center — it is only the sign of Y that must matter here.
    let below_horizon = Vec3::new(0.0, -0.1, -1.0);
    assert_eq!(sun_screen_position(below_horizon, &matrix), None);
}

#[test]
fn the_sun_far_outside_the_frame_gets_no_screen_position_either() {
    let matrix = forward_looking_matrix();

    // 89 degrees off dead-ahead: still technically in front of the
    // camera (a positive `w`), but nowhere near what a 70 degree field of
    // view actually shows.
    let grazing = Vec3::new(89f32.to_radians().sin(), 0.0, -89f32.to_radians().cos());
    assert_eq!(sun_screen_position(grazing, &matrix), None);
}

#[test]
fn the_sun_in_front_and_above_the_horizon_gets_its_real_screen_position() {
    let matrix = forward_looking_matrix();

    let direction = Vec3::new(0.0, 0.1, -1.0);
    assert_eq!(
        sun_screen_position(direction, &matrix),
        project_direction(&matrix, direction),
    );
}

// The rebuild decision for the discs: the same images at the same sizes are
// the same pass, and a `SunAngularSize` edit alone — no new download — still
// has to rebuild the quads, since the size is baked into their vertices.
#[test]
fn the_discs_identity_is_their_images_and_their_angular_sizes() {
    let sun_and_moon = [celestial(21.0, true), celestial(11.0, false)];
    let same_again = [celestial(21.0, true), celestial(11.0, false)];
    let bigger_sun = [celestial(45.0, true), celestial(11.0, false)];

    assert_eq!(bodies_key(&sun_and_moon), bodies_key(&same_again));
    assert_ne!(bodies_key(&sun_and_moon), bodies_key(&bigger_sun));
    assert_ne!(bodies_key(&sun_and_moon), bodies_key(&sun_and_moon[..1]));
}
