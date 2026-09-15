
// `BasePart.Material`: Roblox's own texture packs, projected onto the surface
// and folded into the `Surface` that `lighting.wgsl` (concatenated in front of
// this file) shades.
//
// One array per map kind, one layer per material the scene actually uses; a
// material missing a map gets a neutral layer, so every fragment can sample all
// four unconditionally and keep the samples in uniform control flow.

@group(1) @binding(0) var material_color: texture_2d_array<f32>;
@group(1) @binding(1) var material_normal: texture_2d_array<f32>;
@group(1) @binding(2) var material_metalness: texture_2d_array<f32>;
@group(1) @binding(3) var material_roughness: texture_2d_array<f32>;
@group(1) @binding(4) var material_sampler: sampler;

// `scene::material::Kind`'s discriminants, which the instance buffer carries.
const KIND_PLASTIC: u32 = 0u;
const KIND_TEXTURED: u32 = 1u;
const KIND_NEON: u32 = 2u;
const KIND_FORCE_FIELD: u32 = 3u;
const KIND_GLASS: u32 = 4u;

// Neon is unlit and over-bright. Roblox writes the part's own colour into the
// frame and a far brighter value into the glow it blooms from — two outputs one
// render target cannot carry — so this is the glow, and the frame gets whatever
// is left after the tone map clamps it.
//
// Calibrated to clear a bloom threshold and lift adjacent surfaces by ~18 sRGB
// steps. Trades off neon brightness (255) against surface clipping at threshold.
const NEON_HDR: f32 = 6.0;
// A ForceField is a tinted shell rather than a surface; the tint is the part's
// colour and the reflection is what sells the "energy" look.
const FORCE_FIELD_REFLECTANCE: f32 = 0.2;
// Glass is barely rougher than a mirror in Studio however its roughness map
// reads, so it keeps a floor on the environment term.
const GLASS_REFLECTANCE: f32 = 0.3;

// Blinn-Phong from a roughness map: the exponent spans a very broad highlight
// (2) to a tight one (2048), which is the useful range of
// `exp2(shininess * (dot(n, h) - 1))`.
const SHININESS_MIN: f32 = 1.0;
const SHININESS_MAX: f32 = 11.0;

/// Everything a surface shader knows before its material is applied.
struct MaterialInput {
    // Object-space position in studs, i.e. the unit mesh scaled by the part.
    object_studs: vec3<f32>,
    object_normal: vec3<f32>,
    // The model matrix's three axes, normalized: what turns the object-space
    // projection frame into the world-space tangent frame.
    rotation: mat3x3<f32>,
    world_normal: vec3<f32>,
    world_position: vec3<f32>,
    albedo: vec3<f32>,
    reflectance: f32,
    layer: u32,
    studs_per_tile: f32,
    kind: u32,
}

/// The extent the model matrix scales its unit mesh to, i.e. the part's own
/// size in studs — its columns are a rotation times that scale, and the only
/// scale a part or a placed mesh carries is its own.
fn model_extent(model: mat4x4<f32>) -> vec3<f32> {
    return vec3<f32>(length(model[0].xyz), length(model[1].xyz), length(model[2].xyz));
}

/// The object-space axes a face's texture runs along: image right, then image
/// *down*, matching `textures::face`'s basis table so a material and a decal
/// on the same face are laid out the same way round.
struct Frame {
    u: vec3<f32>,
    v: vec3<f32>,
}

fn face_frame(axis: vec3<f32>) -> Frame {
    var frame: Frame;
    if axis.y != 0.0 {
        frame.u = vec3<f32>(1.0, 0.0, 0.0);
        frame.v = vec3<f32>(0.0, 0.0, sign_of(axis.y));
    } else if axis.x != 0.0 {
        frame.u = vec3<f32>(0.0, 0.0, -sign_of(axis.x));
        frame.v = vec3<f32>(0.0, -1.0, 0.0);
    } else {
        frame.u = vec3<f32>(sign_of(axis.z), 0.0, 0.0);
        frame.v = vec3<f32>(0.0, -1.0, 0.0);
    }
    return frame;
}

/// Rebuilds the tangent frame around the real surface normal.
///
/// The projection axes are exact on a box but only approximate on a sphere, a
/// cylinder or a CSG-carved facet, where the normal turns away from its
/// dominant axis. Gram-Schmidting `tangent` and `bitangent` independently
/// against `normal` (as this used to) keeps each of them individually
/// perpendicular to `normal` but not to *each other* once the axis is well off
/// `normal`, shearing the frame; deriving `bitangent` from `cross(normal,
/// tangent)` instead keeps all three mutually orthogonal, at any tilt.
fn tangent_normal(sample: vec3<f32>, frame: Frame, input: MaterialInput) -> vec3<f32> {
    let normal = normalize(input.world_normal);
    var tangent = normalize(input.rotation * frame.u);
    tangent = normalize(tangent - normal * dot(normal, tangent));
    // -v, not v: v points down the image while a normal map's green channel
    // points up it. The sign pick keeps that handedness after the cross
    // product, which on its own is only defined up to sign.
    let bitangent_axis = input.rotation * -frame.v;
    let bitangent = cross(normal, tangent) * sign_of(dot(cross(normal, tangent), bitangent_axis));

    let tangent_space = sample * 2.0 - 1.0;
    return normalize(
        tangent * tangent_space.x + bitangent * tangent_space.y + normal * tangent_space.z,
    );
}

/// A tangent-space normal sample resolved against a per-vertex tangent frame,
/// which is what a mesh carrying a `SurfaceAppearance` has and a projected
/// material does not.
///
/// `tangent.w` is the handedness the generator worked out (see
/// `renderer::filemesh::vertex`), already flipped so the bitangent it yields
/// points the way a normal map's green channel does.
fn vertex_tangent_normal(sample: vec3<f32>, world_normal: vec3<f32>, tangent: vec4<f32>) -> vec3<f32> {
    let normal = normalize(world_normal);
    // Gram-Schmidt again: interpolating a frame across a triangle leaves its two
    // axes only approximately perpendicular.
    let along_u = normalize(tangent.xyz - normal * dot(normal, tangent.xyz));
    let bitangent = cross(normal, along_u) * tangent.w;

    let tangent_space = sample * 2.0 - 1.0;
    return normalize(
        along_u * tangent_space.x + bitangent * tangent_space.y + normal * tangent_space.z,
    );
}

/// A surface whose maps have already been sampled, wherever they came from: a
/// material pack projected onto a face, or a `SurfaceAppearance` read in the
/// mesh's own UVs.
struct Mapped {
    /// The part's own colour before any colour map, which is what the kinds
    /// that read no texture at all shade from.
    base_albedo: vec3<f32>,
    /// The same with the colour map already folded in.
    albedo: vec3<f32>,
    /// The geometric normal, world space, before the normal map.
    geometric_normal: vec3<f32>,
    /// And after it.
    normal: vec3<f32>,
    world_position: vec3<f32>,
    reflectance: f32,
    metalness: f32,
    roughness: f32,
    kind: u32,
}

/// Whether a material shades procedurally whatever maps it is handed: Neon is
/// unlit and a ForceField is a tinted shell, so neither reads a texture.
fn is_procedural(kind: u32) -> bool {
    return kind == KIND_NEON || kind == KIND_FORCE_FIELD;
}

/// The shading model both map sources share, so the roughness-to-shininess
/// curve and the metalness rules exist in exactly one place.
fn mapped_shade(mapped: Mapped) -> vec3<f32> {
    if mapped.kind == KIND_NEON {
        // Exposed like every other surface in the frame: the bloom threshold is
        // compared against exposed radiance, so an unexposed neon would glow by
        // a different rule than everything around it.
        return mapped.base_albedo * NEON_HDR * lighting.tuning.x;
    }

    if mapped.kind == KIND_PLASTIC || mapped.kind == KIND_FORCE_FIELD {
        // The mapped normal, not the geometric one: `Plastic` carries a normal
        // map and no colour map, so this branch is the only place its studs can
        // show. Every other material here has the neutral flat map, which
        // `tangent_normal` hands straight back.
        var surface = plastic(
            mapped.base_albedo,
            mapped.normal,
            mapped.world_position,
            mapped.reflectance,
        );
        if mapped.kind == KIND_FORCE_FIELD {
            surface.reflectance = max(surface.reflectance, FORCE_FIELD_REFLECTANCE);
        }
        return shade(surface);
    }

    var surface: Surface;
    surface.albedo = mapped.albedo;
    surface.normal = mapped.normal;
    surface.world_position = mapped.world_position;
    surface.roughness = mapped.roughness;
    surface.shininess = exp2(mix(SHININESS_MAX, SHININESS_MIN, mapped.roughness));
    // `PLASTIC_SPEC_STRENGTH`, from `lighting.wgsl` (concatenated ahead of this
    // file): every textured material is scaled from Plastic's own strength so
    // the two never look like they belong to different renderers. The two used
    // to drift — this file kept its own stale copy of the number.
    surface.spec_strength = PLASTIC_SPEC_STRENGTH * (1.0 - mapped.roughness);
    // A metal reflects its own colour where a dielectric reflects white, and
    // the smoother it is the more of the sky it shows.
    surface.spec_tint = mix(vec3<f32>(1.0), surface.albedo, mapped.metalness);
    surface.reflectance = max(mapped.reflectance, mapped.metalness * (1.0 - mapped.roughness));
    if mapped.kind == KIND_GLASS {
        surface.reflectance = max(surface.reflectance, GLASS_REFLECTANCE);
    }

    return shade(surface);
}

/// Projects the map pack along one world axis and samples it, before any
/// blending: the single-axis case (`material_shade`'s fast path) and each leg
/// of the triplanar blend both funnel through here so they sample identically.
fn sample_axis(axis: vec3<f32>, input: MaterialInput) -> Mapped {
    let frame = face_frame(axis);
    // Studs per tile, so a part keeps its texel density whatever its size.
    let uv = vec2<f32>(
        dot(input.object_studs, frame.u),
        dot(input.object_studs, frame.v),
    ) / max(input.studs_per_tile, 0.001);

    // Sampled before any branch: implicit-derivative sampling is only defined
    // in uniform control flow, and `kind` varies per instance.
    let color = textureSample(material_color, material_sampler, uv, input.layer);
    let normal = textureSample(material_normal, material_sampler, uv, input.layer);
    let metalness = textureSample(material_metalness, material_sampler, uv, input.layer).r;
    let roughness = textureSample(material_roughness, material_sampler, uv, input.layer).r;

    var mapped: Mapped;
    mapped.base_albedo = input.albedo;
    mapped.albedo = input.albedo * color.rgb;
    mapped.geometric_normal = input.world_normal;
    mapped.normal = tangent_normal(normal.rgb, frame, input);
    mapped.world_position = input.world_position;
    mapped.reflectance = input.reflectance;
    mapped.metalness = metalness;
    mapped.roughness = roughness;
    mapped.kind = input.kind;
    return mapped;
}

// How sharply triplanar blending favours the more axis-aligned projection.
// Empirical: high enough that a face within ~8 degrees of axis-aligned is past
// the `TRIPLANAR_FAST_PATH` cutoff below (so ordinary `Part`s never blend),
// low enough that a 45-degree facet (a CSG bevel) blends its two axes smoothly
// instead of showing a hard seam.
const TRIPLANAR_SHARPNESS: f32 = 6.0;
// Above this weight, a single axis is close enough to the whole answer that
// sampling the other two is not worth it: both the fast-path cutoff and, below
// it, the per-axis skip for a triplanar leg with negligible contribution.
const TRIPLANAR_FAST_PATH: f32 = 0.99;
const TRIPLANAR_WEIGHT_EPSILON: f32 = 0.01;

/// How much of each world axis's box projection a fragment's surface belongs
/// to, normalized to sum to 1. Exact on a box face; a smooth blend elsewhere,
/// so a tilted facet doesn't snap between UV charts at the `dominant_axis`
/// boundary the way a single projection would.
fn triplanar_weights(normal: vec3<f32>) -> vec3<f32> {
    let w = pow(abs(normal), vec3<f32>(TRIPLANAR_SHARPNESS));
    return w / max(w.x + w.y + w.z, 0.0001);
}

/// The whole material model: project the pack onto the face (or, on a tilted
/// facet, all three it straddles), sample it, shade.
fn material_shade(input: MaterialInput) -> vec3<f32> {
    let weights = triplanar_weights(input.object_normal);
    let max_weight = max(weights.x, max(weights.y, weights.z));

    // Axis-aligned face — every ordinary `Part` — stays on the original
    // single-sample path: same axis, same UV, same cost and output as before
    // triplanar blending existed.
    if max_weight >= TRIPLANAR_FAST_PATH {
        let axis = dominant_axis(input.object_normal);
        return mapped_shade(sample_axis(axis, input));
    }

    // A facet tilted between axes (e.g. a CSG-carved rock): blend whichever of
    // the three projections actually contribute. Each leg decodes its normal
    // map in a frame Gram-Schmidt'd onto the true geometric normal
    // (`tangent_normal`), so the blended result stays near that normal instead
    // of snapping to one axis's.
    let n = input.object_normal;
    var mapped: Mapped;
    mapped.base_albedo = input.albedo;
    mapped.albedo = vec3<f32>(0.0);
    mapped.geometric_normal = input.world_normal;
    mapped.normal = vec3<f32>(0.0);
    mapped.world_position = input.world_position;
    mapped.reflectance = input.reflectance;
    mapped.metalness = 0.0;
    mapped.roughness = 0.0;
    mapped.kind = input.kind;

    if weights.x >= TRIPLANAR_WEIGHT_EPSILON {
        let leg = sample_axis(vec3<f32>(sign_of(n.x), 0.0, 0.0), input);
        mapped.albedo += leg.albedo * weights.x;
        mapped.normal += leg.normal * weights.x;
        mapped.metalness += leg.metalness * weights.x;
        mapped.roughness += leg.roughness * weights.x;
    }
    if weights.y >= TRIPLANAR_WEIGHT_EPSILON {
        let leg = sample_axis(vec3<f32>(0.0, sign_of(n.y), 0.0), input);
        mapped.albedo += leg.albedo * weights.y;
        mapped.normal += leg.normal * weights.y;
        mapped.metalness += leg.metalness * weights.y;
        mapped.roughness += leg.roughness * weights.y;
    }
    if weights.z >= TRIPLANAR_WEIGHT_EPSILON {
        let leg = sample_axis(vec3<f32>(0.0, 0.0, sign_of(n.z)), input);
        mapped.albedo += leg.albedo * weights.z;
        mapped.normal += leg.normal * weights.z;
        mapped.metalness += leg.metalness * weights.z;
        mapped.roughness += leg.roughness * weights.z;
    }
    mapped.normal = normalize(mapped.normal);

    return mapped_shade(mapped);
}
