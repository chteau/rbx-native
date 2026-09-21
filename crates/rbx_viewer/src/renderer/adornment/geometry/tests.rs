use std::f32::consts::FRAC_PI_2;

use super::*;

const WHITE: [f32; 4] = [1.0, 1.0, 1.0, 1.0];

fn positions(vertices: &[Vertex]) -> Vec<Vec3> {
    vertices.iter().map(|v| Vec3::from(v.position)).collect()
}

#[test]
fn a_box_is_six_quads_inside_its_own_extent() {
    let mut out = Vec::new();
    solid(
        AdornMesh::Box {
            size: Vec3::new(2.0, 4.0, 6.0),
        },
        Mat4::from_translation(Vec3::new(10.0, 0.0, 0.0)),
        WHITE,
        &mut out,
    );
    assert_eq!(out.len(), 36, "six quads, two triangles each");
    for point in positions(&out) {
        let local = point - Vec3::new(10.0, 0.0, 0.0);
        assert!(local.x.abs() <= 1.0 + 1e-5);
        assert!(local.y.abs() <= 2.0 + 1e-5);
        assert!(local.z.abs() <= 3.0 + 1e-5);
    }
}

/// Every shape with a length runs along its frame's own -Z — see
/// `scene::adornment::Mesh`.
#[test]
fn a_cone_points_along_the_frames_negative_z() {
    let mut out = Vec::new();
    solid(
        AdornMesh::Cone {
            radius: 1.0,
            height: 5.0,
        },
        Mat4::IDENTITY,
        WHITE,
        &mut out,
    );
    let points = positions(&out);
    let apex = Vec3::new(0.0, 0.0, -5.0);
    assert!(points.iter().any(|point| (*point - apex).length() < 1e-4));
    // And the base ring sits on the frame's own origin plane.
    assert!(points.iter().any(|point| point.z.abs() < 1e-4));
    assert!(points.iter().all(|point| point.z <= 1e-4));
}

/// A frame that turns also turns the shape: a quarter turn about Y sends
/// the frame's own -Z along +X, and the apex with it.
#[test]
fn a_turned_frame_turns_the_shape_with_it() {
    let mut out = Vec::new();
    solid(
        AdornMesh::Cone {
            radius: 1.0,
            height: 3.0,
        },
        Mat4::from_rotation_y(-FRAC_PI_2),
        WHITE,
        &mut out,
    );
    let apex = Vec3::new(3.0, 0.0, 0.0);
    assert!(positions(&out)
        .iter()
        .any(|point| (*point - apex).length() < 1e-4));
}

/// `InnerRadius` and `Angle`, both documented on
/// `CylinderHandleAdornment`: a hollow quarter cylinder keeps points at
/// both radii and never leaves its own sector.
#[test]
fn a_hollow_cylinder_sector_stays_inside_its_sweep() {
    let mut out = Vec::new();
    solid(
        AdornMesh::Cylinder {
            radius: 2.0,
            inner: 1.0,
            height: 4.0,
            sweep: 90.0,
        },
        Mat4::IDENTITY,
        WHITE,
        &mut out,
    );
    let points = positions(&out);
    assert!(!points.is_empty());
    for point in &points {
        let radius = point.truncate().length();
        assert!((0.99..=2.01).contains(&radius), "{radius}");
        // A 90 degree sweep from +X stays in the first quadrant.
        assert!(point.x >= -1e-4 && point.y >= -1e-4);
        assert!((-4.001..=1e-4).contains(&point.z));
    }
}

#[test]
fn a_full_cylinder_closes_all_the_way_round() {
    let mut out = Vec::new();
    solid(
        AdornMesh::Cylinder {
            radius: 1.0,
            inner: 0.0,
            height: 1.0,
            sweep: 360.0,
        },
        Mat4::IDENTITY,
        WHITE,
        &mut out,
    );
    let points = positions(&out);
    assert!(points.iter().any(|point| point.x < -0.9), "the far side");
    assert!(points.iter().any(|point| point.y < -0.9));
}

#[test]
fn an_arc_rides_its_own_radius_in_the_frames_xy_plane() {
    let mut out = Vec::new();
    solid(
        AdornMesh::Arc {
            radius: 5.0,
            tube: 0.5,
            sweep: 360.0,
        },
        Mat4::IDENTITY,
        WHITE,
        &mut out,
    );
    for point in positions(&out) {
        // Every point is within one tube radius of the ring itself.
        let ring = point.truncate().normalize_or_zero() * 5.0;
        let offset = point - ring.extend(0.0);
        assert!(offset.length() <= 0.5 + 1e-4, "{offset}");
    }
}

#[test]
fn a_line_is_one_quad_with_both_ends_named_from_each_corner() {
    let mut out = Vec::new();
    line(Vec3::ZERO, Vec3::X, 4.0, WHITE, &mut out);
    assert_eq!(out.len(), 6);
    for vertex in &out {
        assert_eq!(vertex.half_width, 2.0);
        assert_ne!(vertex.position, vertex.other);
        assert!(vertex.side == 1.0 || vertex.side == -1.0);
    }
    // Both ends appear, each naming the other.
    assert!(out.iter().any(|v| v.position == [0.0, 0.0, 0.0]));
    assert!(out.iter().any(|v| v.position == [1.0, 0.0, 0.0]));
}

/// However thin an adornment asks for, a line still has to land on at least
/// one pixel — a zero-width quad would draw nothing at all.
#[test]
fn a_hairline_still_covers_a_pixel() {
    let mut out = Vec::new();
    line(Vec3::ZERO, Vec3::X, 0.0, WHITE, &mut out);
    assert_eq!(out[0].half_width, 0.5);
}

#[test]
fn a_ring_faces_the_eye_at_its_own_radius() {
    let mut out = Vec::new();
    let centre = Vec3::new(1.0, 2.0, 3.0);
    let eye = centre + Vec3::new(0.0, 0.0, 20.0);
    ring(centre, 4.0, 2.0, WHITE, eye, &mut out);
    assert!(!out.is_empty());
    for vertex in &out {
        let offset = Vec3::from(vertex.position) - centre;
        assert!((offset.length() - 4.0).abs() < 1e-3, "on the circle");
        assert!(offset.z.abs() < 1e-3, "in the plane facing the eye");
    }
}

/// An eye exactly at the centre has no direction to face; the ring still
/// has to be geometry rather than a pile of zero-length lines.
#[test]
fn a_ring_seen_from_its_own_centre_still_draws() {
    let mut out = Vec::new();
    ring(Vec3::ZERO, 2.0, 1.0, WHITE, Vec3::ZERO, &mut out);
    assert!(!out.is_empty());
    for vertex in &out {
        assert!((Vec3::from(vertex.position).length() - 2.0).abs() < 1e-3);
    }
}

#[test]
fn a_picture_is_one_quad_the_size_it_asks_for() {
    let mut out = Vec::new();
    let picture = AdornPicture {
        frame: Mat4::IDENTITY,
        size: Vec2::new(4.0, 2.0),
        texture: rbx_assets::AssetRef::Id(1),
        alpha: 0.5,
    };
    picture_quad(&picture, 0.5, &mut out);
    assert_eq!(out.len(), 6);
    for vertex in &out {
        assert_eq!(vertex.alpha, 0.5);
        assert!(vertex.position[0].abs() <= 2.0 + 1e-5);
        assert!(vertex.position[1].abs() <= 1.0 + 1e-5);
        assert_eq!(vertex.position[2], 0.0);
    }
}
