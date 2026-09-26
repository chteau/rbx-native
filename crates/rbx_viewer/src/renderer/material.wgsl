
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

// The scene as it stood after the opaque pass — what a `Glass` surface bends
// out of shape behind itself. One unread texel where the place holds no
// glass at all (see `renderer::post::Post::refraction`), which is why
// nothing but the glass branch ever samples it.
@group(0) @binding(11) var refraction_source: texture_2d<f32>;

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
// How far a pane displaces what is behind it, as a fraction of the frame,
// at a surface turned fully away from the eye. Roblox documents that glass
// refracts ("refraction of light through this material is not supported on
// mobile devices due to computational limitations") but publishes no index
// of refraction or displacement, so the figure is this renderer's own:
// enough to read as bent glass at arm's length, small enough that a pane
// never drags in something from the far side of the frame.
const GLASS_REFRACTION: f32 = 0.06;

// A ForceField is a shell of energy rather than a surface. Roblox's own
// announcement of the current material ("New material - ForceField (v2.0)",
// DevForum, 2019) describes it as "Fresnel ... driven transparency which
// makes the force field visible near edges", plus, on a mesh with its own
// `TextureID`, an animation where "the 'r' channel of texture is used to
// calculate visibility of force field parts over time (there's a sliding
// window of currently 'visible' range of values). The alpha channel controls
// how visible the texture pattern is." Its motion was described by the same
// engineer as "some pseudorandom smooth change of values with pretty big
// period". A plain part does not animate and carries no pattern — only the
// mesh-with-texture case does.
//
// What is not published, and is therefore this renderer's own: how opaque
// the shell is face-on versus edge-on, how bright the rim and the pattern
// glow, the window's width (community measurements put the opaque share
// near 10% at any moment, which a window 0.1 wide gives on an image with
// evenly spread red values) and the path the window takes. The depth-driven
// half of that transparency — the glow where the shell cuts through other
// geometry — is not drawn at all.
const FORCE_FIELD_RIM_GLOW: f32 = 2.2;
const FORCE_FIELD_PATTERN_GLOW: f32 = 1.6;
// Opacity multipliers on the part's own alpha (itself capped at one half,
// see `scene::alpha`): faint face-on, fully solid at a grazing edge.
const FORCE_FIELD_FACE_OPACITY: f32 = 0.4;
const FORCE_FIELD_EDGE_OPACITY: f32 = 2.0;
// Where the pattern shows, the shell is solid.
const FORCE_FIELD_PATTERN_OPACITY: f32 = 2.0;
const FORCE_FIELD_WINDOW: f32 = 0.1;

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
    // How much of a `ForceField` mesh's own image is showing here, from
    // `force_field_window`. Left zeroed — no pattern — by every pass with no
    // image of its own, which is every plain part.
    pattern: f32,
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
    // Towards the eye, for the rim a `ForceField` brightens at.
    to_eye: vec3<f32>,
    // `MaterialInput`'s, passed through.
    pattern: f32,
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
            return shade(surface) + force_field_energy(mapped);
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

/// The shell's own light: a rim that brightens where it is seen edge-on,
/// and on a textured mesh whatever of its image is showing. Added to the
/// shaded tint rather than replacing it, so a `ForceField` keeps its colour.
fn force_field_energy(mapped: Mapped) -> vec3<f32> {
    let rim = force_field_rim(mapped.geometric_normal, mapped.to_eye);
    return mapped.base_albedo
        * (mapped.pattern * FORCE_FIELD_PATTERN_GLOW + rim * FORCE_FIELD_RIM_GLOW);
}

/// 0 face-on, 1 edge-on.
fn force_field_rim(normal: vec3<f32>, to_eye: vec3<f32>) -> f32 {
    let facing = 1.0 - abs(dot(normalize(normal), to_eye));
    return facing * facing;
}

/// How opaque a `ForceField` fragment is: the Fresnel-driven transparency
/// Roblox describes, faint face-on and solid at the edge, with the image's
/// visible pattern solid wherever it shows.
fn force_field_alpha(alpha: f32, rim: f32, pattern: f32) -> f32 {
    let opacity = mix(FORCE_FIELD_FACE_OPACITY, FORCE_FIELD_EDGE_OPACITY, rim)
        + pattern * FORCE_FIELD_PATTERN_OPACITY;
    return clamp(alpha * opacity, 0.0, 1.0);
}

/// Whether a texel of a `ForceField` mesh's image shows at `phase`: its red
/// channel against the window of "visible" values Roblox describes, the
/// window's centre wandering up and down through 0..1 as the cycle runs.
///
/// Three harmonics of the cycle rather than a noise texture: smooth, never
/// periodic-looking within one cycle, and exactly the same at phase 1 as at
/// phase 0, so the loop never jumps. The weights sum to 1, so the centre
/// spans the whole range at its extremes.
fn force_field_window(red: f32, phase: f32) -> f32 {
    let t = 6.2831855 * phase;
    let wander = 0.5 * sin(t) + 0.3 * sin(2.0 * t + 1.7) + 0.2 * sin(3.0 * t + 4.1);
    let centre = 0.5 + 0.5 * wander;
    let half = FORCE_FIELD_WINDOW * 0.5;
    return 1.0 - smoothstep(half * 0.5, half, abs(red - centre));
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
    mapped.to_eye = normalize(lighting.camera.xyz - input.world_position);
    mapped.pattern = input.pattern;
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

/// What one surface fragment writes: its shaded colour and the alpha it
/// blends with — except for `Glass`, which replaces the pixel outright with
/// the scene behind it displaced, plus its own shading over the top.
///
/// Only the *opaque* scene is there to bend: the copy is taken when the
/// opaque pass ends (see `post::Targets::capture_refraction`), so a
/// translucent part standing behind a pane, or a second pane behind the
/// first, is not in what the pane refracts. `screen_uv` is the fragment's
/// own place in the frame, which is where that copy is read from before the
/// surface's normal pushes the lookup aside.
fn material_output(input: MaterialInput, in_alpha: f32, screen_uv: vec2<f32>) -> vec4<f32> {
    let shaded = material_shade_with_normal(input);
    var alpha = in_alpha;
    if input.kind == KIND_FORCE_FIELD {
        let to_eye = normalize(lighting.camera.xyz - input.world_position);
        alpha = force_field_alpha(alpha, force_field_rim(input.world_normal, to_eye), input.pattern);
    }
    // A pass with no copy behind it binds one unread texel (a
    // `ViewportFrame`'s own, a place with no glass in it at all): there is
    // nothing to bend, so the pane blends the ordinary way instead.
    let source = vec2<i32>(textureDimensions(refraction_source));
    if input.kind != KIND_GLASS || alpha >= 1.0 || source.x <= 1 {
        return vec4<f32>(shaded.color, alpha);
    }

    // The *mapped* normal, not the geometric one: a flat pane facing the eye
    // bends nothing at all (light through it is not displaced), and what
    // gives real glass its wobble is the surface itself — which for this
    // material is its own normal map.
    let clip_normal = uniforms.view_projection * vec4<f32>(shaded.normal, 0.0);
    let offset = vec2<f32>(clip_normal.x, -clip_normal.y) * GLASS_REFRACTION;
    let uv = clamp(screen_uv + offset, vec2<f32>(0.0), vec2<f32>(1.0));
    let texel = clamp(vec2<i32>(uv * vec2<f32>(source)), vec2<i32>(0), source - vec2<i32>(1));
    let behind = textureLoad(refraction_source, texel, 0).rgb;

    // Exactly the blend the pipeline would have done — `dst * (1 - alpha) +
    // src * alpha` — with the bent copy standing in for `dst`, which is what
    // makes this refraction rather than a second layer of haze.
    return vec4<f32>(behind * (1.0 - alpha) + shaded.color * alpha, 1.0);
}

/// What a shaded fragment is, where the caller needs the surface as well as
/// the colour: `Glass` bends the scene behind it along its own mapped
/// normal.
struct Shaded {
    color: vec3<f32>,
    normal: vec3<f32>,
}

/// The whole material model: project the pack onto the face (or, on a tilted
/// facet, all three it straddles), sample it, shade.
fn material_shade(input: MaterialInput) -> vec3<f32> {
    return material_shade_with_normal(input).color;
}

/// [`material_shade`] keeping the surface normal it shaded with.
fn material_shade_with_normal(input: MaterialInput) -> Shaded {
    let weights = triplanar_weights(input.object_normal);
    let max_weight = max(weights.x, max(weights.y, weights.z));

    // Axis-aligned face — every ordinary `Part` — stays on the original
    // single-sample path: same axis, same UV, same cost and output as before
    // triplanar blending existed.
    if max_weight >= TRIPLANAR_FAST_PATH {
        let axis = dominant_axis(input.object_normal);
        let mapped = sample_axis(axis, input);
        return Shaded(mapped_shade(mapped), mapped.normal);
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
    mapped.to_eye = normalize(lighting.camera.xyz - input.world_position);
    mapped.pattern = input.pattern;

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

    return Shaded(mapped_shade(mapped), mapped.normal);
}
