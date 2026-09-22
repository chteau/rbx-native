use super::*;

/// A `SpotLight`-shaped light: a real cone, `Shadows` on, at some `position`.
fn spot(position: Vec3, shadows: bool) -> LocalLight {
    LocalLight {
        position,
        color: Vec3::ONE,
        range: 20.0,
        face_u: Vec3::ZERO,
        face_v: 0.0,
        direction: -Vec3::Y,
        // A 90 degree cone, same as `crate::lighting::local::cone`'s default.
        cos_outer: 45f32.to_radians().cos(),
        cos_inner: 40f32.to_radians().cos(),
        shadows,
    }
}

/// A `PointLight`-shaped light: no cone axis at all, whatever `shadows` says.
fn point(position: Vec3) -> LocalLight {
    LocalLight {
        position,
        color: Vec3::ONE,
        range: 20.0,
        face_u: Vec3::ZERO,
        face_v: 0.0,
        direction: Vec3::ZERO,
        cos_outer: -2.0,
        cos_inner: -1.0,
        shadows: true,
    }
}

#[test]
fn a_point_light_never_gets_selected_however_high_the_cap() {
    let lights = [point(Vec3::ZERO)];
    assert!(select(&lights, Vec3::ZERO, 16).is_empty());
}

#[test]
fn shadows_false_keeps_a_cone_light_out_of_the_selection() {
    let lights = [spot(Vec3::ZERO, false)];
    assert!(select(&lights, Vec3::ZERO, 16).is_empty());
}

#[test]
fn a_zero_cap_selects_nothing_even_with_eligible_lights() {
    let lights = [spot(Vec3::ZERO, true)];
    assert!(select(&lights, Vec3::ZERO, 0).is_empty());
}

// The whole point of recomputing every frame: the nearest lights to the
// camera get a map, not the first ones in the place's own light list.
#[test]
fn only_the_nearest_lights_survive_the_cap_in_distance_order() {
    let lights = [
        spot(Vec3::new(30.0, 0.0, 0.0), true), // index 0, farthest
        spot(Vec3::new(10.0, 0.0, 0.0), true), // index 1, nearest
        spot(Vec3::new(20.0, 0.0, 0.0), true), // index 2, middle
    ];

    let selected = select(&lights, Vec3::ZERO, 2);

    assert_eq!(selected.len(), 2);
    assert_eq!(selected[0].index, 1);
    assert_eq!(selected[1].index, 2);
}

/// `clip = view_projection * point`, as `local_light_visibility` in
/// lights.wgsl reads it: `xyz / w` is the NDC this map's own `[0, 1]` depth and
/// `[-1, 1]` xy live in.
fn ndc(view_projection: Mat4, point: Vec3) -> Option<Vec3> {
    let clip = view_projection * point.extend(1.0);
    (clip.w > 0.0).then(|| clip.truncate() / clip.w)
}

#[test]
fn a_point_inside_the_cone_and_range_lands_inside_the_clip_volume() {
    let light = spot(Vec3::new(0.0, 10.0, 0.0), true);
    let projection = view_projection(&light);

    // Straight down the axis, halfway to Range: squarely inside the cone.
    let inside = light.position + light.direction * 10.0;
    let point = ndc(projection, inside).expect("in front of the light");

    assert!(point.x.abs() <= 1.0 && point.y.abs() <= 1.0);
    assert!((0.0..=1.0).contains(&point.z));
}

// Behind the light is the one direction `Selected::view_projection` must never
// show anything for — a caster there would otherwise wrap around and darken
// the light's own front.
#[test]
fn a_point_behind_the_light_is_culled() {
    let light = spot(Vec3::new(0.0, 10.0, 0.0), true);
    let projection = view_projection(&light);

    let behind = light.position - light.direction * 5.0;
    let clip = projection * behind.extend(1.0);

    // `local_light_visibility` in lights.wgsl guards on exactly this: a
    // non-positive `w` is what a perspective divide behind the eye produces.
    assert!(clip.w <= 0.0);
}

/// A 16 by 2 ceiling panel shines its cone from every point of its face, so
/// its map has to take in the whole frustum that face lights — out to the
/// far corner of the floor it reaches — and nothing of the part behind it.
#[test]
fn a_face_lights_map_covers_the_whole_frustum_it_lights() {
    let range = 16.0;
    for angle in [0.0f32, 30.0, 90.0, 150.0] {
        let half = (0.5 * angle).to_radians();
        let light = LocalLight {
            face_u: Vec3::X * 8.0,
            face_v: 1.0,
            // The outer edge `crate::lighting::local::cone` gives this Angle.
            cos_outer: half.cos().min(1.0 - 1e-3),
            ..spot(Vec3::new(0.0, 10.0, 0.0), true)
        };
        let projection = view_projection(&light);
        let inside = |point: Vec3| {
            ndc(projection, point).is_some_and(|ndc| {
                ndc.x.abs() <= 1.0 && ndc.y.abs() <= 1.0 && (0.0..=1.0).contains(&ndc.z)
            })
        };

        // The face's own corners, just in front of it, and the far corner of
        // the lit frustum, a little inside its reach and its edge.
        let depth = 0.9 * range * half.cos();
        let grow = depth * half.tan().min(1e3) * 0.99;
        for (x, z) in [(8.0, 1.0), (-8.0, -1.0)] {
            assert!(inside(Vec3::new(x, 9.8, z)), "Angle {angle}: face corner");
            let far = Vec3::new(x + grow.copysign(x), 10.0 - depth, z + grow.copysign(z));
            assert!(inside(far), "Angle {angle}: far corner {far}");
        }
        assert!(
            !inside(Vec3::new(0.0, 10.5, 0.0)),
            "the part behind the face"
        );
    }
}

#[test]
fn packing_fills_only_the_selected_indices_with_their_own_layer() {
    let lights = [
        spot(Vec3::new(5.0, 0.0, 0.0), true),
        spot(Vec3::new(1.0, 0.0, 0.0), true),
        point(Vec3::ZERO),
    ];
    let selected = select(&lights, Vec3::ZERO, 8);
    assert_eq!(selected.len(), 2);

    let packed = pack(lights.len(), &selected, &[]);

    assert_eq!(packed.len(), 3);
    // Nearest first: index 1 (distance 1) is layer 0, index 0 is layer 1.
    assert_eq!(packed[1].layer[0], 0.0);
    assert_eq!(packed[0].layer[0], 1.0);
    assert_eq!(packed[2].layer[0], -1.0);
    assert_eq!(
        packed[1].view_projection,
        selected[0].view_projection.to_cols_array_2d()
    );
}

/// A point light takes a cube of the other array, and says so in the second
/// slot rather than the first — the two kinds never share a record.
#[test]
fn packing_gives_a_point_light_a_cube_and_no_layer() {
    let lights = [spot(Vec3::new(5.0, 0.0, 0.0), true), point(Vec3::ZERO)];
    let cones = select(&lights, Vec3::ZERO, 8);
    let cubes = super::super::point::select(&lights, Vec3::ZERO, 8);
    assert_eq!(cubes.len(), 1);

    let packed = pack(lights.len(), &cones, &cubes);
    assert_eq!(packed[0].layer[0], 0.0, "the cone keeps its own layer");
    assert_eq!(packed[0].layer[1], -1.0, "and no cube");
    assert_eq!(packed[1].layer[0], -1.0, "the point light has no layer");
    assert_eq!(packed[1].layer[1], 0.0, "and the first cube");
}

#[test]
fn packing_never_produces_an_empty_buffer() {
    assert_eq!(pack(0, &[], &[]).len(), 1);
    assert_eq!(pack(0, &[], &[])[0].layer[0], -1.0);
    assert_eq!(pack(0, &[], &[])[0].layer[1], -1.0);
}

// What `LightShadow` in lights.wgsl declares: a `mat4x4` and one `vec4` behind
// it, both already 16-byte aligned so nothing pads between them.
#[test]
fn the_packed_record_is_a_matrix_and_one_vec4() {
    assert_eq!(std::mem::size_of::<LightShadowRaw>(), 4 * 16 + 16);
    assert_eq!(std::mem::size_of::<LightShadowRaw>() % 16, 0);
}

// Nothing but this checks the two declarations against each other: a field
// inserted on one side alone silently shifts every field after it, and the
// shader reads the wrong bytes with no error anywhere (see the same check for
// `LightingRaw` in `renderer::lighting::tests`).
#[test]
fn the_packed_record_and_the_shader_declare_the_same_fields_in_the_same_order() {
    let shader = include_str!("../../lights.wgsl");
    let start = shader.find("struct LightShadow {").expect("declared");
    let block = &shader[start..start + shader[start..].find('}').expect("closed")];
    let fields: Vec<&str> = block
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with("//"))
        .filter_map(|line| line.split_once(':'))
        .map(|(field, _)| field.trim())
        .collect();

    assert_eq!(fields, ["view_projection", "layer"]);
}
