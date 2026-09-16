use rbx_assets::AssetRef;
use rbx_dom::{
    Color3Data, ColorSequence, ColorSequenceKeypoint, NumberSequence, NumberSequenceKeypoint,
};

use super::*;
use crate::scene::Curve;

const EPSILON: f32 = 1e-4;

fn straight_beam(segments: u32, width0: f32, width1: f32) -> Beam {
    Beam {
        curve: Curve::new(
            Vec3::ZERO,
            Vec3::X,
            0.0,
            Vec3::new(0.0, 0.0, -10.0),
            Vec3::X,
            0.0,
        ),
        width0,
        width1,
        color: ColorSequence {
            keypoints: vec![
                ColorSequenceKeypoint {
                    time: 0.0,
                    color: Color3Data {
                        r: 1.0,
                        g: 0.0,
                        b: 0.0,
                    },
                    envelope: 0.0,
                },
                ColorSequenceKeypoint {
                    time: 1.0,
                    color: Color3Data {
                        r: 0.0,
                        g: 0.0,
                        b: 1.0,
                    },
                    envelope: 0.0,
                },
            ],
        },
        transparency: NumberSequence {
            keypoints: vec![NumberSequenceKeypoint {
                time: 0.0,
                value: 0.0,
                envelope: 0.0,
            }],
        },
        texture: AssetRef::Empty,
        texture_length: 2.0,
        texture_mode: TextureMode::Stretch,
        texture_speed: 0.0,
        light_emission: 0.0,
        face_camera: false,
        secondary_axis0: Vec3::Y,
        secondary_axis1: Vec3::Y,
        segments,
        z_offset: 0.0,
    }
}

#[test]
fn ribbon_vertex_count_is_two_per_cross_section() {
    let beam = straight_beam(10, 1.0, 1.0);
    let verts = vertices(&beam, Vec3::new(0.0, 20.0, 0.0), 0.0);
    assert_eq!(verts.len(), 2 * (10 + 1));
}

#[test]
fn width_interpolates_linearly_from_width0_to_width1() {
    let beam = straight_beam(2, 2.0, 6.0);
    let verts = vertices(&beam, Vec3::new(0.0, 20.0, 0.0), 0.0);
    // Cross-section 0 (t=0): top/bottom straddle the centreline by width0/2.
    let width_at_section = |section: usize| {
        (Vec3::from(verts[section * 2].position) - Vec3::from(verts[section * 2 + 1].position))
            .length()
    };
    assert!((width_at_section(0) - 2.0).abs() < EPSILON);
    assert!(
        (width_at_section(1) - 4.0).abs() < EPSILON,
        "t=0.5 midpoint"
    );
    assert!((width_at_section(2) - 6.0).abs() < EPSILON);
}

#[test]
fn colour_and_transparency_are_sampled_from_the_sequences_at_each_cross_sections_t() {
    let beam = straight_beam(1, 1.0, 1.0);
    let verts = vertices(&beam, Vec3::new(0.0, 20.0, 0.0), 0.0);
    assert_eq!(verts[0].color, eval_color(&beam.color, 0.0));
    assert_eq!(verts[0].color, [1.0, 0.0, 0.0], "t=0 keypoint is pure red");
    let last = verts.len() - 2;
    assert_eq!(verts[last].color, eval_color(&beam.color, 1.0));
    assert_eq!(
        verts[last].color,
        [0.0, 0.0, 1.0],
        "t=1 keypoint is pure blue"
    );
    assert_eq!(verts[0].alpha, 1.0 - eval_number(&beam.transparency, 0.0));
}

#[test]
fn width_maps_to_u_unwrapped_and_length_maps_to_v_tiled() {
    // Roblox samples a beam's length along the texture's row axis (`v`), not
    // its column axis (`u`) — see `ribbon::vertices`'s doc comment. `u` is
    // therefore always 0.0/1.0 (one side of the ribbon each), never scaled by
    // `TextureLength`.
    let beam = straight_beam(4, 1.0, 1.0);
    let verts = vertices(&beam, Vec3::new(0.0, 20.0, 0.0), 0.0);
    for section in verts.as_chunks::<2>().0 {
        assert_eq!(section[0].uv[0], 0.0, "one edge of the ribbon is u=0");
        assert_eq!(section[1].uv[0], 1.0, "the other edge is u=1");
    }
}

#[test]
fn stretch_mode_repeats_texture_length_times_across_v() {
    let beam = Beam {
        texture_mode: TextureMode::Stretch,
        texture_length: 3.0,
        ..straight_beam(4, 1.0, 1.0)
    };
    let verts = vertices(&beam, Vec3::new(0.0, 20.0, 0.0), 0.0);
    assert!((verts[0].uv[1] - 0.0).abs() < EPSILON, "t=0 starts at v=0");
    let last = verts.len() - 2;
    assert!(
        (verts[last].uv[1] - 3.0).abs() < EPSILON,
        "t=1 reaches v=TextureLength, i.e. 3 repetitions over the whole beam"
    );
}

#[test]
fn wrap_mode_repeats_once_per_texture_length_studs() {
    let beam = Beam {
        texture_mode: TextureMode::Wrap,
        texture_length: 2.0,
        ..straight_beam(4, 1.0, 1.0)
    };
    // `straight_beam` runs 10 studs along -Z (see its `Curve::new` call).
    let verts = vertices(&beam, Vec3::new(0.0, 20.0, 0.0), 0.0);
    let last = verts.len() - 2;
    assert!(
        (verts[last].uv[1] - 5.0).abs() < EPSILON,
        "10-stud beam / 2-stud TextureLength = 5 repetitions"
    );
}

#[test]
fn positive_texture_speed_scrolls_v_backwards_over_time() {
    let beam = Beam {
        texture_speed: 1.0,
        ..straight_beam(1, 1.0, 1.0)
    };
    let at_rest = vertices(&beam, Vec3::new(0.0, 20.0, 0.0), 0.0);
    let after_a_second = vertices(&beam, Vec3::new(0.0, 20.0, 0.0), 1.0);
    assert!(
        (after_a_second[0].uv[1] - (at_rest[0].uv[1] - 1.0)).abs() < EPSILON,
        "one second at TextureSpeed=1 subtracts a full cycle from v, which \
         reads as the pattern advancing toward Attachment1"
    );
}

#[test]
fn face_camera_width_is_orthogonal_to_both_the_tangent_and_the_view_vector() {
    let cases = [
        (Vec3::Z, Vec3::new(1.0, 1.0, 1.0).normalize()),
        (Vec3::X, Vec3::Y),
        (
            Vec3::new(1.0, 1.0, 0.0).normalize(),
            Vec3::new(-1.0, 0.5, 2.0).normalize(),
        ),
    ];
    for (tangent, to_camera) in cases {
        let width_dir = face_camera_width(tangent, to_camera);
        assert!((width_dir.length() - 1.0).abs() < EPSILON, "unit length");
        assert!(
            width_dir.dot(tangent).abs() < EPSILON,
            "orthogonal to tangent"
        );
        assert!(
            width_dir.dot(to_camera).abs() < EPSILON,
            "orthogonal to the view vector"
        );
    }
}

#[test]
fn fixed_width_matches_secondary_axis_when_it_is_already_perpendicular_to_the_tangent() {
    // The common case (a `SecondaryAxis` deliberately set across the beam's
    // own direction, e.g. world-up on a horizontal beam) must render exactly
    // as before: the projection in `fixed_width` is then a no-op.
    let cases = [
        (Vec3::Y, Vec3::X),
        (Vec3::Z, Vec3::new(1.0, 1.0, 0.0).normalize()),
    ];
    for (secondary, tangent) in cases {
        let width_dir = fixed_width(secondary, secondary, tangent, 0.5);
        assert!(
            (width_dir - secondary).length() < EPSILON,
            "already-orthogonal axis should pass through unchanged"
        );
    }
}

#[test]
fn fixed_width_drops_the_component_of_secondary_axis_along_the_tangent() {
    // `SecondaryAxis` is only guaranteed orthogonal to its own attachment's
    // `Axis`, never to the chord between two attachments — see `fixed_width`'s
    // doc comment. An axis with a component *along* the beam's own direction
    // must not leak into the width vector, which has to stay perpendicular to
    // the ribbon's own length to read as a proper (non-sheared) width.
    let tangent = Vec3::X;
    let secondary = Vec3::new(1.0, 1.0, 0.0).normalize(); // 45° off tangent
    let width_dir = fixed_width(secondary, secondary, tangent, 0.5);
    assert!((width_dir.length() - 1.0).abs() < EPSILON, "unit length");
    assert!(
        width_dir.dot(tangent).abs() < EPSILON,
        "orthogonal to tangent"
    );
    assert!(
        (width_dir - Vec3::Y).length() < EPSILON,
        "reduces to +Y here"
    );
}

#[test]
fn fixed_width_falls_back_when_the_blended_axis_is_parallel_to_the_tangent() {
    let width_dir = fixed_width(Vec3::Z, Vec3::Z, Vec3::Z, 0.5);
    assert!(width_dir.is_finite());
    assert!((width_dir.length() - 1.0).abs() < EPSILON);
}

#[test]
fn fixed_width_falls_back_when_secondary_axes_cancel_at_the_blend_point() {
    // Opposite endpoint axes lerp through exactly zero at t=0.5.
    let width_dir = fixed_width(Vec3::Y, -Vec3::Y, Vec3::X, 0.5);
    assert!(width_dir.is_finite());
    assert!((width_dir.length() - 1.0).abs() < EPSILON);
}

#[test]
fn face_camera_width_falls_back_when_looking_straight_down_the_beam() {
    let width_dir = face_camera_width(Vec3::Z, Vec3::Z);
    assert!(width_dir.is_finite());
    assert!((width_dir.length() - 1.0).abs() < EPSILON);
}

#[test]
fn appended_strips_bridge_with_two_degenerate_vertices() {
    let a = vec![VertexRaw {
        position: [0.0; 3],
        uv: [0.0; 2],
        color: [0.0; 3],
        alpha: 1.0,
        light_emission: 0.0,
    }];
    let b = vec![VertexRaw {
        position: [1.0; 3],
        uv: [0.0; 2],
        color: [0.0; 3],
        alpha: 1.0,
        light_emission: 0.0,
    }];
    let mut out = a.clone();
    append(&mut out, &b);
    assert_eq!(out.len(), a.len() + 2 + b.len());
    assert_eq!(
        out[1], a[0],
        "bridges with a repeat of the previous strip's last vertex"
    );
    assert_eq!(
        out[2], b[0],
        "then a repeat of the next strip's first vertex"
    );
}
