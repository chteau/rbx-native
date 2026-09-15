use super::*;

const ALL: [NormalId; 6] = [
    NormalId::Right,
    NormalId::Top,
    NormalId::Back,
    NormalId::Left,
    NormalId::Bottom,
    NormalId::Front,
];

/// The UV a box corner takes, `su`/`sv` being ±1 along the image's right
/// and down axes; on a box those four corners are the whole face.
fn corner_uv(projection: &Projection, su: f32, sv: f32) -> [f32; 2] {
    let point = (projection.u * su + projection.v * sv) * 0.5;
    [
        (point.dot(projection.u) + 0.5) * projection.uv_scale[0] + projection.uv_offset[0],
        (point.dot(projection.v) + 0.5) * projection.uv_scale[1] + projection.uv_offset[1],
    ]
}

#[test]
fn ordinals_match_the_enum_normalid_order() {
    for (ordinal, expected) in ALL.iter().enumerate() {
        let raw = u32::try_from(ordinal).unwrap();
        assert_eq!(NormalId::from_ordinal(raw), Some(*expected));
    }
    assert_eq!(NormalId::from_ordinal(6), None);
}

// The handedness rule the whole face table rests on: an unmirrored image has
// right × down pointing away from its viewer, who stands outside the part.
#[test]
fn every_face_basis_is_unmirrored_and_orthogonal() {
    for face in ALL {
        let basis = face.basis();
        assert_eq!(basis.u.cross(basis.v), -basis.normal, "{face:?}");
        assert_eq!(basis.u.dot(basis.v), 0.0, "{face:?}");
        assert_eq!(basis.u.dot(basis.normal), 0.0, "{face:?}");
    }
}

#[test]
fn the_six_faces_cover_the_six_axes_exactly_once() {
    let mut normals: Vec<[f32; 3]> = ALL.iter().map(|f| f.basis().normal.to_array()).collect();
    normals.sort_by(|a, b| a.partial_cmp(b).unwrap());
    normals.dedup();
    assert_eq!(normals.len(), 6);
}

#[test]
fn a_face_normal_classifies_as_its_own_face() {
    for face in ALL {
        assert_eq!(dominant(face.basis().normal), face, "{face:?}");
    }
}

#[test]
fn a_surface_normal_belongs_to_the_axis_it_leans_on_most() {
    // The upper quarter of a cylinder's side, and the ring around it.
    assert_eq!(dominant(Vec3::new(0.3, 0.9, 0.0)), NormalId::Top);
    assert_eq!(dominant(Vec3::new(0.9, 0.3, 0.0)), NormalId::Right);
    assert_eq!(dominant(Vec3::new(-0.9, 0.3, 0.0)), NormalId::Left);
    assert_eq!(dominant(Vec3::new(0.0, -0.2, -0.98)), NormalId::Front);
}

// A WedgePart's slope is the tie that matters: 45 degrees between Top and a
// side. Roblox paints it with the Front decal, which the wedge remap in
// `renderer/textured.wgsl` turns this Top answer back into.
#[test]
fn ties_go_to_y_then_x() {
    let slope = Vec3::new(0.0, 1.0, 1.0).normalize();
    assert_eq!(dominant(slope), NormalId::Top);
    assert_eq!(
        dominant(Vec3::new(1.0, 1.0, 0.0).normalize()),
        NormalId::Top
    );
    assert_eq!(
        dominant(Vec3::new(1.0, 0.0, -1.0).normalize()),
        NormalId::Right
    );
}

#[test]
fn a_stretched_decal_spans_the_face_once() {
    let projection = projection(
        NormalId::Top,
        ShapeKind::Box,
        Vec3::new(4.0, 2.0, 6.0),
        Mapping::Stretched,
    );

    assert_eq!(projection.uv_scale, [1.0, 1.0]);
    assert_eq!(projection.uv_offset, [0.0, 0.0]);
    // A stretched image covers the face exactly once, corner to corner.
    assert_eq!(corner_uv(&projection, -1.0, -1.0), [0.0, 0.0]);
    assert_eq!(corner_uv(&projection, 1.0, -1.0), [1.0, 0.0]);
    assert_eq!(corner_uv(&projection, 1.0, 1.0), [1.0, 1.0]);
    assert_eq!(corner_uv(&projection, -1.0, 1.0), [0.0, 1.0]);
}

#[test]
fn tiling_counts_one_uv_per_studs_per_tile() {
    // A 16x?x24 top face at 8 studs per tile: two tiles across, three along.
    let projection = projection(
        NormalId::Top,
        ShapeKind::Box,
        Vec3::new(16.0, 2.0, 24.0),
        Mapping::Tiled {
            studs: [8.0, 8.0],
            offset: [0.0, 0.0],
        },
    );

    assert_eq!(corner_uv(&projection, -1.0, -1.0), [0.0, 0.0]);
    assert_eq!(corner_uv(&projection, 1.0, 1.0), [2.0, 3.0]);
}

#[test]
fn a_stud_offset_shifts_the_uvs_by_a_fraction_of_a_tile() {
    let projection = projection(
        NormalId::Front,
        ShapeKind::Box,
        Vec3::new(8.0, 8.0, 1.0),
        Mapping::Tiled {
            studs: [4.0, 4.0],
            offset: [2.0, -1.0],
        },
    );

    assert_eq!(corner_uv(&projection, -1.0, -1.0), [0.5, -0.25]);
    assert_eq!(corner_uv(&projection, 1.0, 1.0), [2.5, 1.75]);
}

#[test]
fn a_degenerate_tile_size_falls_back_to_stretching() {
    for studs in [0.0, -8.0, f32::NAN] {
        let projection = projection(
            NormalId::Right,
            ShapeKind::Box,
            Vec3::splat(4.0),
            Mapping::Tiled {
                studs: [studs, studs],
                offset: [0.0, 0.0],
            },
        );
        assert_eq!(
            corner_uv(&projection, 1.0, 1.0),
            [1.0, 1.0],
            "studs {studs}"
        );
    }
}

// The fixture wedge: 10 wide, 13 high, 11 deep, tiled every 8 studs. Studio
// fits about 1.25 tiles across it and 2.1 along the slope, because the slope
// measures hypot(13, 11) studs and not the 13 the part is tall.
#[test]
fn a_tiled_texture_counts_studs_along_a_wedge_slope() {
    let projection = projection(
        NormalId::Front,
        ShapeKind::Wedge,
        Vec3::new(10.0, 13.0, 11.0),
        Mapping::Tiled {
            studs: [8.0, 8.0],
            offset: [0.0, 0.0],
        },
    );

    assert_eq!(projection.uv_scale[0], 10.0 / 8.0);
    assert!((projection.uv_scale[1] - 17.029_386 / 8.0).abs() < 1e-5);
}

// The slope's own length is the wedge's alone: the same face on a box still
// spans the part's depth, and every other wedge face its own axis.
#[test]
fn only_a_wedge_front_measures_the_slope() {
    let size = Vec3::new(10.0, 13.0, 11.0);
    let tiled = Mapping::Tiled {
        studs: [1.0, 1.0],
        offset: [0.0, 0.0],
    };

    assert_eq!(
        projection(NormalId::Front, ShapeKind::Box, size, tiled).uv_scale[1],
        13.0
    );
    assert_eq!(
        projection(NormalId::Back, ShapeKind::Wedge, size, tiled).uv_scale[1],
        13.0
    );
    assert_eq!(
        projection(NormalId::Bottom, ShapeKind::Wedge, size, tiled).uv_scale[1],
        11.0
    );
}

// A stretched Decal covers the whole slope whatever its length, so the wedge
// has to leave its UV span alone.
#[test]
fn a_stretched_decal_still_spans_a_wedge_slope_once() {
    let projection = projection(
        NormalId::Front,
        ShapeKind::Wedge,
        Vec3::new(10.0, 13.0, 11.0),
        Mapping::Stretched,
    );

    // Down the slope from the top-back edge, since v runs along -Y: the
    // image reads upright on it, as it does on any other face.
    assert_eq!(corner_uv(&projection, -1.0, -1.0), [0.0, 0.0]);
    assert_eq!(corner_uv(&projection, 1.0, 1.0), [1.0, 1.0]);
}
