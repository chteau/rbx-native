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

/// Every shape with a length runs along its frame's own -Z, see
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
/// one pixel, a zero-width quad would draw nothing at all.
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
        double_sided: true,
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

/// Every closed shape winds each triangle counter-clockwise from outside,
/// what a single-sided adornment's back-face discard relies on.
#[test]
fn closed_shapes_wind_every_triangle_outward() {
    let frame = Mat4::from_rotation_translation(
        glam::Quat::from_euler(glam::EulerRot::XYZ, 0.4, 0.6, 0.1),
        Vec3::new(3.0, -2.0, 5.0),
    );
    let centre = |local: Vec3| frame.transform_point3(local);
    for (mesh, inside) in [
        (
            AdornMesh::Box {
                size: Vec3::new(2.0, 4.0, 6.0),
            },
            centre(Vec3::ZERO),
        ),
        (AdornMesh::Sphere { radius: 2.0 }, centre(Vec3::ZERO)),
        (
            AdornMesh::Cone {
                radius: 1.0,
                height: 3.0,
            },
            centre(Vec3::new(0.0, 0.0, -0.75)),
        ),
        (
            AdornMesh::Cylinder {
                radius: 1.0,
                inner: 0.0,
                height: 2.0,
                sweep: 360.0,
            },
            centre(Vec3::new(0.0, 0.0, -1.0)),
        ),
    ] {
        let mut out = Vec::new();
        solid(mesh, frame, WHITE, &mut out);
        for corners in positions(&out).chunks(3) {
            let [a, b, c] = [corners[0], corners[1], corners[2]];
            let normal = (b - a).cross(c - a);
            // A sphere's pole quads each hold one zero-area triangle.
            if normal.length() < 1e-6 {
                continue;
            }
            assert!(
                normal.dot((a + b + c) / 3.0 - inside) > 0.0,
                "{mesh:?} has an inward triangle"
            );
        }
    }
}

/// A hollow sector's inner wall faces the axis and its cut faces face out
/// of the slice: each triangle's normal points away from a point inside the
/// solid ring beside it.
#[test]
fn a_hollow_sector_winds_outward_too() {
    let mut out = Vec::new();
    solid(
        AdornMesh::Cylinder {
            radius: 2.0,
            inner: 1.0,
            height: 2.0,
            sweep: 270.0,
        },
        Mat4::IDENTITY,
        WHITE,
        &mut out,
    );
    for corners in positions(&out).chunks(3) {
        let [a, b, c] = [corners[0], corners[1], corners[2]];
        let centroid = (a + b + c) / 3.0;
        // The nearest point of the solid's core: mid-radius, mid-height, at
        // the centroid's own angle, clamped into the sweep.
        let angle = centroid
            .y
            .atan2(centroid.x)
            .rem_euclid(std::f32::consts::TAU)
            .clamp(0.05, 270f32.to_radians() - 0.05);
        let core = Vec3::new(angle.cos() * 1.5, angle.sin() * 1.5, -1.0);
        assert!((b - a).cross(c - a).dot(centroid - core) > 0.0);
    }
}

/// The image's front is the side its frame's `LookVector` (-Z) points to.
#[test]
fn a_picture_faces_along_its_look_vector() {
    let mut out = Vec::new();
    let picture = AdornPicture {
        frame: Mat4::IDENTITY,
        size: Vec2::new(4.0, 2.0),
        texture: rbx_assets::AssetRef::Id(1),
        alpha: 1.0,
        double_sided: false,
    };
    picture_quad(&picture, 1.0, &mut out);
    for corners in out.chunks(3) {
        let [a, b, c] = [0, 1, 2].map(|i| Vec3::from(corners[i].position));
        assert!((b - a).cross(c - a).z < 0.0);
        assert!(corners.iter().all(|vertex| vertex.single_sided == 1.0));
    }
}

fn pyramid(sides: u32, size: f32, height: f32) -> Vec<Vec3> {
    let mut out = Vec::new();
    solid(
        AdornMesh::Pyramid {
            sides,
            size,
            height,
        },
        Mat4::IDENTITY,
        WHITE,
        &mut out,
    );
    positions(&out)
}

/// What Studio 0.748 drew at `Size = 4`: a square with half-side 2, a
/// hexagon and an octagon with inradius 2, a triangle and a pentagon `Size`
/// tall from their flat bottom edge to the top corner.
#[test]
fn a_pyramid_base_is_size_tall_with_a_flat_bottom_edge() {
    for sides in [3, 4, 5, 6, 8, 32] {
        let points = pyramid(sides, 4.0, 3.0);
        let base: Vec<Vec3> = points.into_iter().filter(|p| p.z.abs() < 1e-4).collect();
        let low = base.iter().map(|p| p.y).fold(f32::MAX, f32::min);
        let high = base.iter().map(|p| p.y).fold(f32::MIN, f32::max);
        assert!(
            (high - low - 4.0).abs() < 1e-4,
            "{sides} sides span {}",
            high - low
        );
        let bottom = base.iter().filter(|p| (p.y - low).abs() < 1e-4);
        assert!(
            bottom.map(|p| p.x).fold(f32::MIN, f32::max) > 0.1,
            "{sides}: no flat edge"
        );
    }
    let square = pyramid(4, 4.0, 3.0);
    assert!(square
        .iter()
        .any(|p| (*p - Vec3::new(2.0, -2.0, 0.0)).length() < 1e-4));
    let hexagon = pyramid(6, 4.0, 3.0);
    assert!(hexagon
        .iter()
        .all(|p| p.y >= -2.0 - 1e-4 && p.y <= 2.0 + 1e-4));
}

#[test]
fn a_pyramid_points_along_negative_z_unless_its_height_is_negative() {
    let up = pyramid(5, 3.0, 3.0);
    assert!(up
        .iter()
        .any(|p| (*p - Vec3::new(0.0, 0.0, -3.0)).length() < 1e-4));
    assert!(up.iter().all(|p| p.z <= 1e-4));
    let down = pyramid(5, 3.0, -3.0);
    assert!(down
        .iter()
        .any(|p| (*p - Vec3::new(0.0, 0.0, 3.0)).length() < 1e-4));
    assert_eq!(
        pyramid(5, 3.0, 3.0).len(),
        5 * 6,
        "a side and a base slice per edge"
    );
}

#[test]
fn a_pyramid_winds_outward_whichever_way_it_points() {
    for (size, height) in [(4.0, 3.0), (4.0, -3.0), (-4.0, 3.0)] {
        let inside = Vec3::new(0.0, 0.0, -height * 0.25);
        for corners in pyramid(5, size, height).chunks(3) {
            let [a, b, c] = [corners[0], corners[1], corners[2]];
            assert!(
                (b - a).cross(c - a).dot((a + b + c) / 3.0 - inside) > 0.0,
                "size {size} height {height}"
            );
        }
    }
}

/// Flat, a pyramid is its base twice over, facing both ways, as Studio
/// shows it from either side.
#[test]
fn a_flat_pyramid_faces_both_ways() {
    let points = pyramid(4, 2.0, 0.0);
    let normals: Vec<f32> = points
        .chunks(3)
        .map(|t| (t[1] - t[0]).cross(t[2] - t[0]).z)
        .filter(|z| z.abs() > 1e-6)
        .collect();
    assert!(normals.iter().any(|z| *z > 0.0) && normals.iter().any(|z| *z < 0.0));
}
