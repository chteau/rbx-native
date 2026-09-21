// The place's own PointLights, SpotLights and SurfaceLights, looped over per
// fragment by `shade` in lighting.wgsl, which this snippet is concatenated in
// front of (WGSL has no #include; see `renderer::pipeline`).
//
// Roblox splats these into a voxel light grid instead, so nothing here matches
// how the engine computes them: the falloff is analytic, and no light is ever
// occluded by anything.

// One PointLight, SpotLight or SurfaceLight, already in world space and linear
// radiance (see `crate::lighting::local`). A point light is spelled as a cone
// wider than any direction, so the loop below needs no branch for it.
struct LocalLight {
    // xyz: position. w: Range, where the falloff reaches zero.
    position_range: vec4<f32>,
    // rgb: radiance. w: half the emitting face along its second axis,
    // cross(direction_cone.xyz, cone_face.yzw).
    color_face: vec4<f32>,
    // xyz: cone axis. w: cosine of the cone's half-angle.
    direction_cone: vec4<f32>,
    // x: cosine of the inner half-angle, strictly above direction_cone.w.
    // yzw: half the emitting face along its first axis, as a vector. Zero,
    // with color_face.w, for anything but a SurfaceLight on a part.
    cone_face: vec4<f32>,
}

@group(0) @binding(6) var<storage, read> local_lights: array<LocalLight>;

// One local light's own shadow map, if `renderer::shadow::local::select` chose
// it this frame — a `LocalLight` and its `LightShadow` share an index, the way
// `local_lights` and `light_shadows` share a length.
struct LightShadow {
    // World space to this light's own clip space. A `PointLight` has six of
    // these instead, in `point_faces` — this one is then unused.
    view_projection: mat4x4<f32>,
    // x: which layer of `local_shadow_map` holds this cone light's map,
    // negative when it casts none this frame (not selected, or
    // `Shadows = false`).
    // y: which cube of `point_shadow_map` holds this point light's six
    // faces, negative the same way. At most one of the two is ever set: a
    // light is one kind or the other.
    layer: vec4<f32>,
}

// One array, sized to the quality level's own cap rather than to the place's
// light count (see `QualityProfile::local_shadow_lights_max`); read through
// `shadow_sampler` above — a `sampler` is not tied to any one texture, so the
// sun's own comparison sampler serves this array too.
@group(0) @binding(7) var local_shadow_map: texture_depth_2d_array;
@group(0) @binding(8) var<storage, read> light_shadows: array<LightShadow>;

// The `PointLight` cubes: six layers per cube, and the matrix each layer was
// drawn with — see `renderer::shadow::point`, which explains why these are
// six perspectives rather than a cube map.
@group(0) @binding(9) var point_shadow_map: texture_depth_2d_array;
@group(0) @binding(10) var<storage, read> point_faces: array<mat4x4<f32>>;

const POINT_FACES: i32 = 6;
// Reciprocal of `renderer::shadow::POINT_SIZE`.
const POINT_SHADOW_TEXEL: f32 = 1.0 / 512.0;

// 3x3, one texel step each way: enough to soften a nearby lantern's edge
// without the sun map's wider (and costlier) kernel, which a light this close
// to what it lights does not need.
const LOCAL_PCF_HALF: i32 = 1;
// Reciprocal of `renderer::shadow::LOCAL_SIZE`, which — unlike the sun's map —
// is not a quality knob: the light count the level allows a map at all is (see
// `local_shadow_lights_max`), so one fixed size covers every level.
const LOCAL_SHADOW_TEXEL: f32 = 1.0 / 1024.0;
// Depth-buffer units. Scaled up at grazing angles below, the same reasoning
// `lamp_visibility`'s own bias uses.
const LOCAL_DEPTH_BIAS: f32 = 0.0015;

/// Which of a cube's six faces a direction out of the light belongs to: the
/// major axis, negative side second — the order `renderer::shadow::point`'s
/// own `AXES` builds the matrices in.
fn point_shadow_face(direction: vec3<f32>) -> i32 {
    let absolute = abs(direction);
    if absolute.x >= absolute.y && absolute.x >= absolute.z {
        return select(0, 1, direction.x < 0.0);
    }
    if absolute.y >= absolute.z {
        return select(2, 3, direction.y < 0.0);
    }
    return select(4, 5, direction.z < 0.0);
}

/// How much of one local light reaches `world_position`, 0 (fully shadowed) to
/// 1. `facing` is `dot(normal, to_light)`, which the caller already has and
/// which the slope-scaled bias needs; every unselected light returns 1 before
/// touching the texture array at all.
fn local_light_visibility(world_position: vec3<f32>, facing: f32, index: u32) -> f32 {
    let record = light_shadows[index];
    var layer = record.layer.x;
    var view_projection = record.view_projection;
    var texel = LOCAL_SHADOW_TEXEL;
    var cube = false;

    if record.layer.y >= 0.0 {
        // A point light: the face the fragment sits on decides both the
        // layer and the matrix, and every face is the same size.
        let direction = world_position - local_lights[index].position_range.xyz;
        let face = point_shadow_face(direction);
        let slot = i32(record.layer.y) * POINT_FACES + face;
        layer = f32(slot);
        view_projection = point_faces[slot];
        texel = POINT_SHADOW_TEXEL;
        cube = true;
    } else if layer < 0.0 {
        return 1.0;
    }

    let clip = view_projection * vec4<f32>(world_position, 1.0);
    if clip.w <= 0.0 {
        return 1.0;
    }
    let ndc = clip.xyz / clip.w;
    let uv = vec2<f32>(ndc.x, -ndc.y) * 0.5 + vec2<f32>(0.5);
    if any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) || ndc.z < 0.0 || ndc.z > 1.0 {
        return 1.0;
    }

    let depth = ndc.z - LOCAL_DEPTH_BIAS * (2.0 - facing);
    var lit = 0.0;
    for (var y = -LOCAL_PCF_HALF; y <= LOCAL_PCF_HALF; y++) {
        for (var x = -LOCAL_PCF_HALF; x <= LOCAL_PCF_HALF; x++) {
            let tap = uv + vec2<f32>(f32(x), f32(y)) * texel;
            if cube {
                lit += textureSampleCompareLevel(
                    point_shadow_map, shadow_sampler, tap, i32(layer), depth
                );
            } else {
                lit += textureSampleCompareLevel(
                    local_shadow_map, shadow_sampler, tap, i32(layer), depth
                );
            }
        }
    }
    let taps = f32(2 * LOCAL_PCF_HALF + 1);
    return lit / (taps * taps);
}

/// The surface one light is being evaluated against: what `Surface` in
/// lighting.wgsl carries that a light needs, and nothing else — this snippet
/// comes first, so `Surface` itself is not declared yet.
struct Receiver {
    position: vec3<f32>,
    normal: vec3<f32>,
    to_eye: vec3<f32>,
    shininess: f32,
    spec_strength: f32,
}

/// What the place's own lights add to a surface, split the way `shade` uses it.
struct LocalTerms {
    diffuse: vec3<f32>,
    specular: vec3<f32>,
}

/// Every local light, summed. No clustering and no culling beyond the range
/// test: a place is expected to have tens of these, not thousands, and the
/// count is capped on the CPU (see `crate::lighting::local`).
///
/// The falloff is linear to zero at `Range`, which is how Roblox's voxel grid
/// reads from the outside. A `SurfaceLight` "emits from the entire surface":
/// every point of its face shines the cone, which comes to measuring the
/// distance and the angle from the face's nearest point rather than from its
/// centre — the frustum its guide draws. A point source has no face, so that
/// nearest point is the light's own position and this is the plain cone.
fn local_terms(receiver: Receiver, count: u32) -> LocalTerms {
    var terms: LocalTerms;
    terms.diffuse = vec3<f32>(0.0);
    terms.specular = vec3<f32>(0.0);

    for (var index = 0u; index < count; index++) {
        let light = local_lights[index];
        let extent_u = length(light.cone_face.yzw);
        let across_u = light.cone_face.yzw / max(extent_u, 1e-6);
        let across_v = cross(light.direction_cone.xyz, across_u);
        let from_centre = receiver.position - light.position_range.xyz;
        let source = light.position_range.xyz
            + across_u * clamp(dot(from_centre, across_u), -extent_u, extent_u)
            + across_v * clamp(dot(from_centre, across_v), -light.color_face.w, light.color_face.w);
        let offset = source - receiver.position;
        let distance = length(offset);
        let range = light.position_range.w;
        if distance >= range {
            continue;
        }
        let to_light = offset / max(distance, 1e-4);
        let facing = dot(receiver.normal, to_light);
        if facing <= 0.0 {
            continue;
        }

        let falloff = saturate((range - distance) / max(range, 1e-3));
        let cone = smoothstep(
            light.direction_cone.w,
            light.cone_face.x,
            dot(-to_light, light.direction_cone.xyz)
        );
        let shadow = local_light_visibility(receiver.position, facing, index);
        let radiance = light.color_face.rgb * (falloff * cone * shadow);

        terms.diffuse += radiance * facing;
        let half_vector = normalize(to_light + receiver.to_eye);
        terms.specular += radiance
            * (receiver.spec_strength
                * exp2(receiver.shininess * (dot(receiver.normal, half_vector) - 1.0))
                * facing);
    }
    return terms;
}
