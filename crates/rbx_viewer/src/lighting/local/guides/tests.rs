use rbx_dom::{Color3Data, Variant};

use super::super::tests::{
    cframe, close, database, Fixture, EPSILON, IDENTITY_ROTATION, YAW_90_ROTATION,
};
use super::super::{DEFAULT_RANGE, DEFAULT_SPOT_RANGE, MAX_RANGE};
use super::*;

const RIGHT: u32 = 0;
const TOP: u32 = 1;
const FRONT: u32 = 5;
/// Every `NormalId`, in its enum order.
const FACES: [u32; 6] = [0, 1, 2, 3, 4, 5];

/// The guide of one selected light of `class` on a part of `size` at
/// `position`, unrotated.
fn guide_of(
    class: &str,
    position: Vec3,
    size: Vec3,
    properties: &[(&str, Variant)],
) -> Vec<Segment> {
    let mut fixture = Fixture::new();
    let part = fixture.part(position, size, IDENTITY_ROTATION);
    let light = fixture.insert(class, Some(part), properties);
    light_guides(&fixture.dom, &database(), &[light])
}

fn face(face: u32) -> (&'static str, Variant) {
    ("Face", Variant::Enum(face))
}

fn angle(degrees: f32) -> (&'static str, Variant) {
    ("Angle", Variant::Float32(degrees))
}

fn range(studs: f32) -> (&'static str, Variant) {
    ("Range", Variant::Float32(studs))
}

/// Every end of every segment.
fn ends(segments: &[Segment]) -> Vec<Vec3> {
    segments
        .iter()
        .flat_map(|segment| [segment.from, segment.to])
        .collect()
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < EPSILON
}

#[test]
fn a_point_light_is_three_great_circles_of_its_range() {
    let centre = Vec3::new(1.0, 5.0, -2.0);
    let segments = guide_of("PointLight", centre, Vec3::splat(2.0), &[range(12.0)]);

    assert_eq!(segments.len(), 3 * CIRCLE_SEGMENTS);
    assert!(ends(&segments)
        .iter()
        .all(|end| near(end.distance(centre), 12.0)));
    // One circle per plane of the part's axes: each lies flat in one of them.
    for (circle, axis) in segments
        .chunks(CIRCLE_SEGMENTS)
        .zip([Vec3::Z, Vec3::X, Vec3::Y])
    {
        assert!(ends(circle)
            .iter()
            .all(|end| near((*end - centre).dot(axis), 0.0)));
    }
}

#[test]
fn a_guide_takes_the_lights_colour_at_a_fixed_opacity() {
    let red = (
        "Color",
        Variant::Color3(Color3Data {
            r: 1.0,
            g: 0.5,
            b: 0.0,
        }),
    );
    let segments = guide_of("PointLight", Vec3::ZERO, Vec3::ONE, &[red]);

    let expected = [
        crate::scene::srgb_to_linear(1.0) * SHADE,
        crate::scene::srgb_to_linear(0.5) * SHADE,
        0.0,
        ALPHA,
    ];
    assert!(segments.iter().all(|segment| segment.color == expected));
    // Depth-tested: Studio's own rings hide behind the part they circle.
    assert!(segments.iter().all(|segment| !segment.on_top));
}

#[test]
fn range_is_defaulted_and_clamped_like_the_light_itself() {
    let radius = |segments: &[Segment]| segments[0].from.length();
    let point = guide_of("PointLight", Vec3::ZERO, Vec3::ONE, &[]);
    assert!(near(radius(&point), DEFAULT_RANGE));

    // A spot's axis line is exactly its range long.
    let spot = guide_of("SpotLight", Vec3::ZERO, Vec3::ONE, &[face(TOP)]);
    assert!(close(spot[0].to, Vec3::Y * DEFAULT_SPOT_RANGE));

    let huge = guide_of("PointLight", Vec3::ZERO, Vec3::ONE, &[range(1000.0)]);
    assert!(near(radius(&huge), MAX_RANGE));

    for nothing in [0.0, -5.0] {
        assert!(guide_of("PointLight", Vec3::ZERO, Vec3::ONE, &[range(nothing)]).is_empty());
    }
}

#[test]
fn a_spot_is_an_axis_four_slant_lines_and_a_rim_on_its_sphere() {
    let segments = guide_of(
        "SpotLight",
        Vec3::ZERO,
        Vec3::new(4.0, 1.0, 2.0),
        &[face(RIGHT), angle(60.0), range(10.0)],
    );

    assert_eq!(segments.len(), 1 + 4 + CIRCLE_SEGMENTS);
    // From the part's centre, not its face: where Studio's lines meet.
    assert!(close(segments[0].from, Vec3::ZERO));
    assert!(close(segments[0].to, Vec3::X * 10.0));
    for slant in &segments[1..5] {
        assert!(close(slant.from, Vec3::ZERO));
        // Out to the sphere, 30 degrees off the axis.
        assert!(near(slant.to.length(), 10.0));
        assert!(near(
            slant.to.normalize().dot(Vec3::X),
            30f32.to_radians().cos()
        ));
    }
    let rim = ends(&segments[5..]);
    assert!(rim
        .iter()
        .all(|end| near(end.x, 10.0 * 30f32.to_radians().cos()) && near(end.length(), 10.0)));
}

#[test]
fn a_zero_angle_spot_is_its_axis_alone() {
    let segments = guide_of(
        "SpotLight",
        Vec3::ZERO,
        Vec3::ONE,
        &[face(TOP), angle(0.0), range(10.0)],
    );

    assert_eq!(segments.len(), 1);
    assert!(close(segments[0].to, Vec3::Y * 10.0));
}

#[test]
fn a_half_sphere_spot_lays_its_rim_flat_through_the_apex() {
    for degrees in [180.0, 400.0] {
        let segments = guide_of(
            "SpotLight",
            Vec3::ZERO,
            Vec3::ONE,
            &[face(TOP), angle(degrees), range(10.0)],
        );

        let rim = ends(&segments[5..]);
        assert!(
            rim.iter()
                .all(|end| near(end.y, 0.0) && near(end.length(), 10.0)),
            "Angle {degrees} is a half sphere"
        );
    }
}

#[test]
fn a_spots_face_is_read_through_its_parts_rotation() {
    let mut fixture = Fixture::new();
    let part = fixture.part(Vec3::new(0.0, 3.0, 0.0), Vec3::ONE, YAW_90_ROTATION);
    let light = fixture.insert("SpotLight", Some(part), &[face(FRONT), range(5.0)]);

    let segments = light_guides(&fixture.dom, &database(), &[light]);

    // The quarter turn sends the part's Front (-Z) to -X.
    assert!(close(segments[0].to, Vec3::new(-5.0, 3.0, 0.0)));
}

#[test]
fn a_light_on_an_attachment_shines_from_it_along_its_axes() {
    let mut fixture = Fixture::new();
    let part = fixture.part(Vec3::new(0.0, 10.0, 0.0), Vec3::splat(2.0), YAW_90_ROTATION);
    let attachment = fixture.insert(
        "Attachment",
        Some(part),
        &[(
            "CFrame",
            cframe(Vec3::new(0.0, 0.0, 4.0), IDENTITY_ROTATION),
        )],
    );
    let spot = fixture.insert("SpotLight", Some(attachment), &[face(FRONT), range(5.0)]);
    // No face to stretch over: the class docs make it a spot here.
    let surface = fixture.insert("SurfaceLight", Some(attachment), &[face(FRONT), range(5.0)]);

    let database = database();
    let spot = light_guides(&fixture.dom, &database, &[spot]);
    let surface = light_guides(&fixture.dom, &database, &[surface]);

    // The quarter turn sends the attachment's +Z offset to +X, and Front to -X.
    let apex = Vec3::new(4.0, 10.0, 0.0);
    assert!(close(spot[0].from, apex));
    assert!(close(spot[0].to, apex - Vec3::X * 5.0));
    assert_eq!(spot, surface);
}

#[test]
fn a_surface_light_spreads_from_each_face_of_a_long_part() {
    let size = Vec3::new(2.0, 4.0, 6.0);
    let centre = Vec3::new(0.0, 8.0, 0.0);
    let (reach, half) = (10.0, 45f32.to_radians());
    for raw in FACES {
        let normal_id = NormalId::from_ordinal(raw).unwrap();
        let normal = normal_id.axis();
        let segments = guide_of(
            "SurfaceLight",
            centre,
            size,
            &[face(raw), angle(90.0), range(reach)],
        );
        assert_eq!(segments.len(), 1 + 8, "{normal_id:?}");

        let face_centre = centre + normal * (0.5 * size.dot(normal.abs()));
        assert!(close(segments[0].from, face_centre), "{normal_id:?}");
        assert!(close(segments[0].to, face_centre + normal * reach));

        let across = size - size * normal.abs();
        for corner_line in segments[1..].iter().step_by(2) {
            // Starts on a corner of that very face...
            let start = corner_line.from - face_centre;
            assert!(near(start.dot(normal), 0.0), "{normal_id:?}");
            assert!(close(start.abs(), 0.5 * across), "{normal_id:?}");
            // ...and ends as far out as the cone's rim, grown by its radius.
            let end = corner_line.to - face_centre;
            assert!(near(end.dot(normal), reach * half.cos()));
            let grown = 0.5 * across + (Vec3::ONE - normal.abs()) * (reach * half.sin());
            assert!(close(
                end.abs() - normal.abs() * end.dot(normal).abs(),
                grown
            ));
        }
    }
}

#[test]
fn a_surface_lights_far_rectangle_is_closed_edge_to_edge() {
    let segments = guide_of(
        "SurfaceLight",
        Vec3::ZERO,
        Vec3::new(2.0, 4.0, 6.0),
        &[face(TOP), range(10.0)],
    );

    let edges: Vec<&Segment> = segments[2..].iter().step_by(2).collect();
    for (edge, next) in edges.iter().zip(edges.iter().cycle().skip(1)) {
        assert!(close(edge.to, next.from));
        // Along a side, never across a diagonal.
        let run = edge.to - edge.from;
        assert!(near(run.x, 0.0) || near(run.z, 0.0));
    }
}

#[test]
fn surface_angles_zero_and_half_sphere_bound_the_frustum() {
    let size = Vec3::new(2.0, 4.0, 6.0);
    let straight = guide_of(
        "SurfaceLight",
        Vec3::ZERO,
        size,
        &[face(TOP), angle(0.0), range(10.0)],
    );
    // Angle 0: "light travels directly outward from the surface".
    for corner_line in straight[1..].iter().step_by(2) {
        assert!(close(corner_line.to - corner_line.from, Vec3::Y * 10.0));
    }

    let flat = guide_of(
        "SurfaceLight",
        Vec3::ZERO,
        size,
        &[face(TOP), angle(180.0), range(10.0)],
    );
    // Angle 180: "outward perpendicular to the surface" — the far rectangle
    // lies in the face's own plane, a range wider on every side.
    for corner_line in flat[1..].iter().step_by(2) {
        assert!(near(corner_line.to.y, 2.0));
        assert!(near(corner_line.to.x.abs(), 1.0 + 10.0));
        assert!(near(corner_line.to.z.abs(), 3.0 + 10.0));
    }
}

#[test]
fn only_an_enabled_light_that_is_itself_selected_has_a_guide() {
    let mut fixture = Fixture::new();
    let part = fixture.part(Vec3::ZERO, Vec3::ONE, IDENTITY_ROTATION);
    let lit = fixture.insert("PointLight", Some(part), &[]);
    let off = fixture.insert(
        "PointLight",
        Some(part),
        &[("Enabled", Variant::Bool(false))],
    );
    let database = database();

    assert!(light_guides(&fixture.dom, &database, &[off]).is_empty());
    // "Light Guides will not show when just selecting a light's parent".
    assert!(light_guides(&fixture.dom, &database, &[part]).is_empty());
    assert_eq!(
        light_guides(&fixture.dom, &database, &[part, lit, off]).len(),
        3 * CIRCLE_SEGMENTS
    );
}

#[test]
fn a_light_with_nowhere_to_shine_from_has_no_guide() {
    let mut fixture = Fixture::new();
    let folder = fixture.insert("Folder", Some(fixture.workspace), &[]);
    let loose = fixture.insert("PointLight", Some(folder), &[]);
    let part = fixture.part(Vec3::ZERO, Vec3::ONE, IDENTITY_ROTATION);
    let faceless = fixture.insert("SpotLight", Some(part), &[]);

    let guides = light_guides(&fixture.dom, &database(), &[loose, faceless]);

    assert!(guides.is_empty());
}
