// Roblox's shading model, concatenated into every surface shader (WGSL has no
// #include). Owns bind group 0; passes only need to declare vertex data and call
// `shade`. Two directional lamps, ambient term, Blinn-Phong specular, environment
// probe and fog on linear values; HDR output survives bloom.

struct Uniforms {
    view_projection: mat4x4<f32>,
    // xy: the frame's pixel size, which is what turns a fragment's own
    // `@builtin(position)` into the screen UV a refracting surface samples
    // the scene behind itself at (see `material.wgsl`). The outline passes
    // declare the same pair under their own `Frame` struct. z: where the
    // `ForceField` shimmer is in its cycle, 0 to 1 (`pipeline::shimmer_phase`).
    viewport: vec4<f32>,
}

struct LightingUniform {
    // xyz: unit vector pointing at the sun. The second lamp sits at -xyz, which
    // is where the moon is once the sun has set.
    sun_direction: vec4<f32>,
    sun_color: vec4<f32>,
    fill_color: vec4<f32>,
    ambient: vec4<f32>,
    // xyz: classic fog colour. w: 0 for classic fog, 1 for an Atmosphere.
    fog_color: vec4<f32>,
    // x: fog start, y: fog end, z: atmosphere density, w: atmosphere offset.
    fog_range: vec4<f32>,
    atmosphere_color: vec4<f32>,
    atmosphere_decay: vec4<f32>,
    // xyz: the eye, in world space.
    camera: vec4<f32>,
    // x: exp2(ExposureCompensation), y: EnvironmentSpecularScale,
    // z: EnvironmentDiffuseScale, already weighted (see renderer::lighting),
    // w: highest mip level of the cube map.
    tuning: vec4<f32>,
    // xyz: what the sky, its bounce and its haze are multiplied by at this time
    // of day — white by day, a dim blue at night. w: how much of the star field
    // shows, 0 by day to 1 at night.
    sky_tint: vec4<f32>,
    // x: Atmosphere.Glare, y: Atmosphere.Haze.
    atmosphere_extra: vec4<f32>,
    // Cosine-weighted sky irradiance at +X, -X, +Y, -Y, +Z, -Z, which is what
    // the diffuse environment term is rebuilt from.
    sky_irradiance: array<vec4<f32>, 6>,
    // World space to the shadow map's own clip space.
    light_view_projection: mat4x4<f32>,
    // x: PCF kernel radius in texels, y: one texel in UV, z: one texel in studs,
    // w: the receiver's constant depth bias, already in [0, 1] depth units.
    shadow_params: vec4<f32>,
    // x: 0 with GlobalShadows off, +1 when the sun casts, -1 when the moon does
    // (which is the fill lamp at -L, see `crate::lighting`).
    shadow_lamp: vec4<f32>,
    // x: how many entries of `local_lights` are real. The buffer always holds at
    // least one, so this count is the only thing that says a scene has none.
    locals: vec4<f32>,
    // x: how far geometry is drawn at full strength, in studs, 0 being no limit.
    // y: 1 where the environment terms sample the probe, 0 where they fall back
    // to one flat sky colour. Both come from the graphics quality level (see
    // `crate::quality`), not from the place.
    quality: vec4<f32>,
    // `Clouds.Color`, linear.
    clouds_color: vec4<f32>,
    // x: Cover, 0 drawing nothing. y: Density.
    clouds_extra: vec4<f32>,
}

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(0) @binding(1) var<uniform> lighting: LightingUniform;
@group(0) @binding(2) var env_cube: texture_cube<f32>;
@group(0) @binding(3) var env_sampler: sampler;
@group(0) @binding(4) var shadow_map: texture_depth_2d;
@group(0) @binding(5) var shadow_sampler: sampler_comparison;

// Plastic, the default part material. Roblox derives the exponent from one
// per-material parameter `p` as `p * 0.5036`; `PLASTIC_SHININESS` is that at
// the `p` near 60 a plain plastic part lands on. The strength has no such
// primary-source derivation — Roblox publishes no formula for it — so it is
// empirically tuned instead: low enough that an upward face reflecting the
// sky never reads as a mirror, high enough that a curved part shows a real
// highlight against the sun the way Studio's does. This is the highlight's
// strength at full EnvironmentSpecularScale; the term is scaled by that dial
// in `shade`, so a place that sets it to 0 is matte here whatever this value
// is (see `shade` for why). Verified against the material-sample fixture
// (scale 1, glossy) and marked.rbxl (scale 0, matte).
const PLASTIC_SHININESS: f32 = 30.0;
const PLASTIC_SPEC_STRENGTH: f32 = 0.28;
const PLASTIC_ROUGHNESS: f32 = 0.25;

// Roblox samples its environment probe at `(0.089 + roughness * 0.911) * 5`,
// the 5 being the top mip of its own probe. That fraction is only meaningful
// alongside the size the probe starts from: Studio's reflections are famously
// soft — a Reflectance 1 plastic part shows a sky gradient blurred over some
// ten degrees, not a mirror — which puts its base face at about 32 texels.
// Reading the curve as an absolute resolution instead of a fraction of *our*
// chain is what keeps a 1024-texel sky from turning every reflective part into
// a mirror.
const ENV_LOD_BIAS: f32 = 0.089;
const ENV_LOD_SLOPE: f32 = 0.911;
const PROBE_TOP_MIP: f32 = 5.0;
const PROBE_LOG2_FACE: f32 = 5.0;

// Half-width of the PCF grid: 5x5 taps, each already hardware-filtered over
// 2x2 texels, which is as far as a kernel can be stretched before the gaps in it
// start showing up as banding along a shadow edge.
const PCF_HALF: i32 = 2;

// Below this the kernel is narrower than the bilinear footprint of a single tap,
// so the other twenty-four would return exactly the same value: ShadowSoftness 0
// is a hard edge and costs one tap.
const PCF_MIN_RADIUS: f32 = 0.5;

// Local lights are implemented but do not cast shadows: `Light.Shadows` is
// ignored, so a light shines through a wall. The `Technology` property is also
// not implemented; only `ShadowMap` is available.
struct Surface {
    albedo: vec3<f32>,
    normal: vec3<f32>,
    world_position: vec3<f32>,
    /// How much of the environment probe replaces the shaded colour outright:
    /// `BasePart.Reflectance`, or a metal's own metalness (see material.wgsl).
    reflectance: f32,
    roughness: f32,
    shininess: f32,
    spec_strength: f32,
    /// White on a dielectric; the albedo on a metal, which is the whole of what
    /// makes gold look like gold and not like painted plastic.
    spec_tint: vec3<f32>,
}

/// A surface with Plastic's material constants, which is also what every
/// material without a texture pack falls back to.
fn plastic(
    albedo: vec3<f32>,
    normal: vec3<f32>,
    world_position: vec3<f32>,
    reflectance: f32,
) -> Surface {
    var surface: Surface;
    surface.albedo = albedo;
    surface.normal = normal;
    surface.world_position = world_position;
    surface.reflectance = reflectance;
    surface.roughness = PLASTIC_ROUGHNESS;
    surface.shininess = PLASTIC_SHININESS;
    surface.spec_strength = PLASTIC_SPEC_STRENGTH;
    surface.spec_tint = vec3<f32>(1.0);
    return surface;
}

// sign() answers 0 for 0, which would leave a flat component belonging to no
// face at all; the positive face claims it, as it does on the CPU.
fn sign_of(component: f32) -> f32 {
    return select(-1.0, 1.0, component >= 0.0);
}

// The face a surface point belongs to, as the signed unit axis its normal leans
// on most: what wraps a decal round a cylinder instead of leaving it standing
// out as a flat plate, and what picks the plane a material is projected along.
// Ties go to Y, then X, so a wedge's 45-degree slope reads as Top, which is the
// plane its material is projected along; `textured.wgsl` sends a wedge's decals
// to Front instead, the face Roblox paints a slope with.
//
// Mirrors `dominant` in `textures/face.rs`, where the rule is unit-tested; the
// two must agree or a decal lands on the wrong face.
fn dominant_axis(normal: vec3<f32>) -> vec3<f32> {
    let axis = abs(normal);
    if axis.y >= axis.x && axis.y >= axis.z {
        return vec3<f32>(0.0, sign_of(normal.y), 0.0);
    }
    if axis.x >= axis.z {
        return vec3<f32>(sign_of(normal.x), 0.0, 0.0);
    }
    return vec3<f32>(0.0, 0.0, sign_of(normal.z));
}

// The sky as one colour: the mean of the six cardinal irradiance values, which
// for a sky of uniform radiance is exactly that radiance.
fn sky_flat() -> vec3<f32> {
    var sum = vec3<f32>(0.0);
    for (var face = 0; face < 6; face++) {
        sum += lighting.sky_irradiance[face].rgb;
    }
    return sum / 6.0;
}

// Tinted like the sky it is: the probe holds one set of daylight panels, so
// without this a night scene would still show a bright blue reflection.
//
// Below quality level 8 the probe is not sampled at all: a `Reflectance` part or
// a metal takes the flat sky colour instead, which is what turns a chrome sphere
// into a plain tinted ball the way a low graphics level does.
fn env_sample(direction: vec3<f32>, level: f32) -> vec3<f32> {
    if lighting.quality.y < 0.5 {
        return sky_flat() * lighting.sky_tint.rgb;
    }
    return textureSampleLevel(env_cube, env_sampler, direction, level).rgb * lighting.sky_tint.rgb;
}

/// Mip level a surface of this roughness reflects from, as the level whose
/// texels are as coarse as Roblox's own probe would be.
fn env_level(roughness: f32) -> f32 {
    let top_mip = lighting.tuning.w;
    let probe = PROBE_TOP_MIP * (ENV_LOD_BIAS + roughness * ENV_LOD_SLOPE);
    return clamp(top_mip - PROBE_LOG2_FACE + probe, 0.0, top_mip);
}

/// The sky's own irradiance on a surface, rebuilt from the six cardinal values
/// the uniform carries: an ambient cube, whose squared weights sum to one for
/// any unit normal and are exact on the axes themselves.
fn sky_ambient(normal: vec3<f32>) -> vec3<f32> {
    let weight = normal * normal;
    let x = select(lighting.sky_irradiance[1], lighting.sky_irradiance[0], normal.x >= 0.0);
    let y = select(lighting.sky_irradiance[3], lighting.sky_irradiance[2], normal.y >= 0.0);
    let z = select(lighting.sky_irradiance[5], lighting.sky_irradiance[4], normal.z >= 0.0);
    let irradiance = x.rgb * weight.x + y.rgb * weight.y + z.rgb * weight.z;
    return irradiance * lighting.tuning.z * lighting.sky_tint.rgb;
}

/// How much of the casting lamp reaches this point, 0 (fully shadowed) to 1.
///
/// The receiver is pushed along its own normal before being projected, by about
/// the world width of the kernel and only where the lamp grazes it. That offset
/// is what keeps a surface from shadowing itself once the kernel reaches into
/// neighbouring texels — and, unlike a bigger depth bias, it slides the lookup
/// *along* the surface instead of lifting the shadow off the foot of its caster.
fn lamp_visibility(world_position: vec3<f32>, normal: vec3<f32>, facing: f32) -> f32 {
    if abs(lighting.shadow_lamp.x) < 0.5 {
        return 1.0;
    }

    let radius = lighting.shadow_params.x;
    let grazing = saturate(1.0 - abs(facing));
    let offset = normal * lighting.shadow_params.z * (1.0 + radius) * grazing;
    let clip = lighting.light_view_projection * vec4<f32>(world_position + offset, 1.0);
    let ndc = clip.xyz / clip.w;
    // Outside the map is lit, not shadowed: the map only covers the near few
    // hundred studs, and anything past it has no caster on record.
    let uv = vec2<f32>(ndc.x, -ndc.y) * 0.5 + vec2<f32>(0.5);
    if any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) || ndc.z < 0.0 || ndc.z > 1.0 {
        return 1.0;
    }

    let depth = ndc.z - lighting.shadow_params.w;
    if radius < PCF_MIN_RADIUS {
        return textureSampleCompareLevel(shadow_map, shadow_sampler, uv, depth);
    }

    // A plain grid rather than a rotated disc: the taps are already spread over
    // whole texels by the comparison sampler's own bilinear filter, so the
    // banding a disc is meant to break up never forms.
    let step = radius * lighting.shadow_params.y / f32(PCF_HALF);
    var lit = 0.0;
    for (var y = -PCF_HALF; y <= PCF_HALF; y++) {
        for (var x = -PCF_HALF; x <= PCF_HALF; x++) {
            let tap = uv + vec2<f32>(f32(x), f32(y)) * step;
            lit += textureSampleCompareLevel(shadow_map, shadow_sampler, tap, depth);
        }
    }
    let taps = f32(2 * PCF_HALF + 1);
    return lit / (taps * taps);
}

/// The whole model, per fragment: two lamps, ambient, specular, environment,
/// exposure and fog. Returns linear colour clamped to the displayable range —
/// Roblox squares it down into a gamma-2 target here, which an `*Srgb` render
/// target does for us on write.
fn shade(surface: Surface) -> vec3<f32> {
    let normal = normalize(surface.normal);
    let light = normalize(lighting.sun_direction.xyz);
    let offset = lighting.camera.xyz - surface.world_position;
    let distance = length(offset);
    let to_eye = offset / max(distance, 1e-4);

    // Two lamps, one at L and a dim one at -L: what stops a face turned away
    // from the sun reading as a flat silhouette.
    let facing = dot(normal, light);
    // Only the lamp above the horizon is occluded — and only its own term.
    // Ambient and sky irradiance stay whole, which is what makes a Studio
    // shadow on a grey baseplate read as pure blue sky rather than as black.
    let lamp = lighting.shadow_lamp.x;
    let toward_lamp = select(-facing, facing, lamp > 0.0);
    let visibility = lamp_visibility(surface.world_position, normal, toward_lamp);
    let sun_lit = select(1.0, visibility, lamp > 0.5);
    let fill_lit = select(1.0, visibility, lamp < -0.5);
    let direct = lighting.sun_color.rgb * saturate(facing) * sun_lit
        + lighting.fill_color.rgb * saturate(-facing) * fill_lit;

    // Roblox mixes indoor Ambient with OutdoorAmbient by a per-voxel skylight
    // factor. We have no voxel grid, so every surface is treated as open to the
    // sky (outdoor = 1) — an honest approximation, not the real thing.
    let ambient = lighting.ambient.rgb + sky_ambient(normal);

    let half_vector = normalize(light + to_eye);
    let specular = surface.spec_strength
        * exp2(surface.shininess * (dot(normal, half_vector) - 1.0))
        * saturate(facing)
        * sun_lit;

    let environment = env_sample(reflect(-to_eye, normal), env_level(surface.roughness));

    // The place's own lights, which reach every surface pass this file is
    // concatenated in front of.
    let locals = local_terms(
        Receiver(
            surface.world_position,
            normal,
            to_eye,
            surface.shininess,
            surface.spec_strength
        ),
        u32(max(lighting.locals.x, 0.0))
    );

    // The sun's own specular highlight is scaled by EnvironmentSpecularScale
    // (`tuning.y`), the same dial the environment reflection below is: a place
    // that turns it to 0 (as many low-poly/stylised places do) reads as matte
    // in Studio, its flat colours full and unwashed, and adding a broad sun
    // highlight there is exactly what greyed the greens out. A place that
    // leaves it at 1 keeps the full highlight. Roblox publishes no formula, so
    // this ties the direct highlight to the same scale as the reflection it is
    // physically the sharp end of — matched against Studio on a place with the
    // scale at 0 (matte) and one at 1 (glossy).
    var color = surface.albedo * (direct + ambient + locals.diffuse)
        + (specular * lighting.sun_color.rgb * lighting.tuning.y
            + locals.specular
            + environment * lighting.tuning.y * surface.spec_strength)
            * surface.spec_tint;
    // Reflectance is a straight crossfade to the reflection, with no Fresnel:
    // that is literally what Roblox does. Gated by EnvironmentSpecularScale
    // like every other use of `environment` above — Lighting's own docs describe
    // the property as "specular light derived from the environment" in general,
    // not specifically the roughness-driven term, and Reflectance's crossfade is
    // exactly that, just dialled per-part instead of per-material.
    color = mix(
        color,
        environment * lighting.tuning.y * surface.spec_tint,
        saturate(surface.reflectance)
    );
    color *= lighting.tuning.x;

    return max(fade(color, to_eye, distance), vec3<f32>(0.0));
}
