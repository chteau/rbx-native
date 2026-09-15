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
    // rgb: radiance inside the near field. w: how far that field reaches.
    color_near: vec4<f32>,
    // xyz: cone axis. w: cosine of the cone's half-angle.
    direction_cone: vec4<f32>,
    // x: cosine of the inner half-angle, strictly above direction_cone.w.
    cone_inner: vec4<f32>,
}

@group(0) @binding(6) var<storage, read> local_lights: array<LocalLight>;

// One local light's own shadow map, if `renderer::shadow::local::select` chose
// it this frame — a `LocalLight` and its `LightShadow` share an index, the way
// `local_lights` and `light_shadows` share a length.
struct LightShadow {
    // World space to this light's own clip space.
    view_projection: mat4x4<f32>,
    // x: which layer of `local_shadow_map` holds the map, negative when this
    // light casts none this frame — not selected, `Shadows = false`, or a
    // `PointLight`, which never is (see `renderer::shadow::local::select`).
    layer: vec4<f32>,
}

// One array, sized to the quality level's own cap rather than to the place's
// light count (see `QualityProfile::local_shadow_lights_max`); read through
// `shadow_sampler` above — a `sampler` is not tied to any one texture, so the
// sun's own comparison sampler serves this array too.
@group(0) @binding(7) var local_shadow_map: texture_depth_2d_array;
@group(0) @binding(8) var<storage, read> light_shadows: array<LightShadow>;

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

/// How much of one local light reaches `world_position`, 0 (fully shadowed) to
/// 1. `facing` is `dot(normal, to_light)`, which the caller already has and
/// which the slope-scaled bias needs; every unselected light returns 1 before
/// touching the texture array at all.
fn local_light_visibility(world_position: vec3<f32>, facing: f32, index: u32) -> f32 {
    let record = light_shadows[index];
    let layer = record.layer.x;
    if layer < 0.0 {
        return 1.0;
    }

    let clip = record.view_projection * vec4<f32>(world_position, 1.0);
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
            let tap = uv + vec2<f32>(f32(x), f32(y)) * LOCAL_SHADOW_TEXEL;
            lit += textureSampleCompareLevel(
                local_shadow_map, shadow_sampler, tap, i32(layer), depth
            );
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
/// reads from the outside; a `SurfaceLight` holds full brightness across its own
/// face first, so a wide panel is not one hot spot in its middle.
fn local_terms(receiver: Receiver, count: u32) -> LocalTerms {
    var terms: LocalTerms;
    terms.diffuse = vec3<f32>(0.0);
    terms.specular = vec3<f32>(0.0);

    for (var index = 0u; index < count; index++) {
        let light = local_lights[index];
        let offset = light.position_range.xyz - receiver.position;
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

        let near = light.color_near.w;
        let falloff = saturate((range - distance) / max(range - near, 1e-3));
        let cone = smoothstep(
            light.direction_cone.w,
            light.cone_inner.x,
            dot(-to_light, light.direction_cone.xyz)
        );
        let shadow = local_light_visibility(receiver.position, facing, index);
        let radiance = light.color_near.rgb * (falloff * cone * shadow);

        terms.diffuse += radiance * facing;
        let half_vector = normalize(to_light + receiver.to_eye);
        terms.specular += radiance
            * (receiver.spec_strength
                * exp2(receiver.shininess * (dot(receiver.normal, half_vector) - 1.0))
                * facing);
    }
    return terms;
}
