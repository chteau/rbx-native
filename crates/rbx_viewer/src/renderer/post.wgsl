// The resolve: everything that happens to the frame after the last triangle.
//
// The scene is drawn into an HDR target with nothing clamped, so this is where
// the range is closed: a BlurEffect (if enabled) replaces the sharp frame
// outright, the bloom is built from whatever cleared the effect's Threshold and
// added on top, a DepthOfFieldEffect mixes that same blurred copy back in by how
// far each pixel's reconstructed depth falls from the focus plane,
// SunRaysEffect's god-rays are marched in and added likewise, the result is
// tone mapped into the displayable range, and the ColorCorrectionEffect grades
// that displayable frame before an `*Srgb` target encodes it. Roblox orders
// bloom, tone map and colour correction the same way; its own tone map is a
// clamp and a square root into a gamma-2 target.

struct PostUniform {
    // x: BloomEffect.Intensity, y: its Threshold, z: the tent radius in texels
    // of the level being sampled (see renderer::post::bloom).
    bloom: vec4<f32>,
    // x: Brightness, y: Contrast, z: Saturation, w: 1 where a
    // ColorCorrectionEffect is enabled and 0 where the place has none.
    correction: vec4<f32>,
    // TintColor, as the raw components Studio shows: a gain, not a radiance.
    tint: vec4<f32>,
    // x: the tone curve: 0 a clamp (Default under Voxel or Compatibility, and
    // a ColorGradingEffect's TonemapperPreset.Retro under any), 2 `filmic`
    // (Default under ShadowMap or Future). y: 1 where a BlurEffect is enabled, 0
    // otherwise. z: the camera's near plane in studs, which is the whole of what
    // view_distance needs. w: 1 where a DepthOfFieldEffect is enabled.
    misc: vec4<f32>,
    // x/y: the sun's screen-space UV this frame (see
    // renderer::sun::sun_screen_position). z: SunRaysEffect.Intensity, 0 skips
    // the whole effect, whichever of "no effect", "moon lit" or "off-screen"
    // that came from. w: Spread.
    sun_rays: vec4<f32>,
    // DepthOfFieldEffect: x: FocusDistance and y: InFocusRadius, both in studs,
    // z: NearIntensity and w: FarIntensity, both 0-1 mixes. Whether any of it
    // applies at all is misc.w, not a zero in here.
    depth_of_field: vec4<f32>,
    // x: 1 on an orthographic frame, 0 on a perspective one, which
    // view_distance formula the depth buffer needs. y/z: the orthographic far
    // and near planes in studs (near is negative: the volume runs behind the
    // eye plane too, see camera::orthographic_range), meaningless when x is
    // 0. w unused.
    camera: vec4<f32>,
}

// Rec. 709 luma, which the saturation term rotates around. Mirrors `LUMA` in
// `lighting::effects`, whose `apply` is this grade in Rust.
const LUMA: vec3<f32> = vec3<f32>(0.2126, 0.7152, 0.0722);
// The `*Srgb` target's encode, near enough, so the grade below can work on the
// same values the screen shows and undo itself before the target re-encodes.
const DISPLAY_GAMMA: f32 = 2.2;

@group(0) @binding(0) var<uniform> post: PostUniform;
@group(1) @binding(0) var source: texture_2d<f32>;
@group(1) @binding(1) var source_sampler: sampler;
@group(2) @binding(0) var bloom: texture_2d<f32>;
@group(2) @binding(1) var bloom_sampler: sampler;
@group(3) @binding(0) var blur: texture_2d<f32>;
@group(3) @binding(1) var blur_sampler: sampler;
// The scene's own depth buffer, the one the opaque geometry was tested against,
// bound for reading now that the scene pass is over, in this group rather than a
// fifth one because a WebGPU device is only guaranteed four. Loaded, never
// sampled: a depth filtered across a silhouette is a depth no surface is at, and
// above one sample WGSL has no textureSample for a multisampled texture at all.
// That is also why `renderer::post` builds this shader twice, retyping this one
// line, see its DEPTH_BINDING.
@group(3) @binding(2) var scene_depth: texture_depth_2d;
// The depth-tested adornments, premultiplied, before the tone curve (see
// renderer::adornment and `composite_adornments`).
@group(3) @binding(3) var adornments: texture_2d<f32>;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

/// The real client's tone curve under ShadowMap and Future, a rational
/// function fitted to its frames of a grey ramp lit at six known strengths
/// (see `crate::lighting`'s `FILMIC_SUN`). It lifts nothing below about 0.1,
/// rolls the highlights off and only reaches white near 2.
fn filmic(x: vec3<f32>) -> vec3<f32> {
    let curve = x * (FILMIC_A * x + FILMIC_B) / (x * (FILMIC_C * x + FILMIC_D) + FILMIC_E);
    return min(curve, vec3<f32>(1.0));
}

const FILMIC_A: f32 = 2.4536;
const FILMIC_B: f32 = 0.0353;
const FILMIC_C: f32 = 2.1029;
const FILMIC_D: f32 = 0.7171;
const FILMIC_E: f32 = 0.1414;

/// One oversized triangle rather than two: no vertex buffer, and no seam down
/// the diagonal where two triangles would meet.
@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));

    var out: VertexOutput;
    out.clip_position = vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0);
    out.uv = uv;
    return out;
}

fn source_texel() -> vec2<f32> {
    return 1.0 / vec2<f32>(textureDimensions(source));
}

/// Four bilinear taps at the corners of one destination texel, i.e. a 4x4 box
/// of the source: what keeps a halved image from shimmering on the bright
/// pixels bloom exists to find.
fn box_sample(uv: vec2<f32>) -> vec3<f32> {
    let offset = source_texel();
    var sum = vec3<f32>(0.0);
    for (var y = -1; y <= 1; y += 2) {
        for (var x = -1; x <= 1; x += 2) {
            sum += textureSample(source, source_sampler, uv + vec2<f32>(f32(x), f32(y)) * offset).rgb;
        }
    }
    return sum * 0.25;
}

/// What glows: the part of a pixel that clears the Threshold, with its hue kept.
///
/// The cut is on the brightest channel rather than on luma, which is what makes
/// a saturated neon glow as readily as a white one.
@fragment
fn fs_threshold(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = box_sample(in.uv);
    let brightest = max(color.r, max(color.g, color.b));
    let excess = max(brightest - post.bloom.y, 0.0);

    return vec4<f32>(color * (excess / max(brightest, 1e-4)), 1.0);
}

@fragment
fn fs_downsample(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(box_sample(in.uv), 1.0);
}

/// A 3x3 tent, spread by the radius the chain was sized for, added into the
/// level below. Repeated up the chain it is a very good Gaussian for a tenth of
/// the taps one would cost.
@fragment
fn fs_upsample(in: VertexOutput) -> @location(0) vec4<f32> {
    let offset = source_texel() * post.bloom.z;
    var sum = vec3<f32>(0.0);
    var total = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let weight = f32(2 - abs(x)) * f32(2 - abs(y));
            sum += textureSample(source, source_sampler, in.uv + vec2<f32>(f32(x), f32(y)) * offset).rgb * weight;
            total += weight;
        }
    }
    return vec4<f32>(sum / total, 1.0);
}

const SUN_RAY_TAPS: i32 = 24;
/// The glow's colour at Intensity 1 and how fast it fades, as a fraction of
/// the frame's height per unit of Spread. Measured on the real client (the
/// `sun_rays` fidelity scene against the same view without the effect): at
/// Intensity 0.3 and Spread 0.8 it adds (0.46, 0.17, 0) in linear light a
/// third of the frame from the sun and still (0.19, 0.06, 0) at the far
/// edge, washing the sky and the floor alike, darker only where something
/// stands between the pixel and the sun. Refitted once the sky panels stopped
/// being mirrored (fidelity pass 5): the near floor takes less of the glow
/// than the sky, so only what is past 250 studs counts as open.
const SUN_RAY_TINT: vec3<f32> = vec3<f32>(4.1, 1.5, 0.0);
const SUN_RAY_REACH: f32 = 0.81;
const SUN_RAY_OPEN_STUDS: f32 = 250.0;  // fitted on `sun_rays`

/// A glow from the sun over the whole frame, fading with the distance to it,
/// times how much of the straight line from the pixel to the sun is open in
/// the depth buffer, which is what leaves the shafts behind whatever stands in
/// the way. Open is sky or anything past [`SUN_RAY_OPEN_STUDS`]: the real
/// client washes a far floor as it does the sky, and only what stands near
/// casts a shaft (the `sun_rays` scene). Each pixel starts its march at its own fraction of a
/// step (interleaved gradient noise), so the taps' fixed spacing never lines
/// up into bands across the frame; what is left is a fine grain.
fn sun_rays(uv: vec2<f32>) -> vec3<f32> {
    let size = vec2<f32>(textureDimensions(scene_depth));
    let to_sun = post.sun_rays.xy - uv;
    let distance = length(to_sun * vec2<f32>(size.x / size.y, 1.0));
    let last = vec2<i32>(size) - vec2<i32>(1);

    let pixel = uv * size;
    let jitter = fract(52.9829189 * fract(dot(pixel, vec2<f32>(0.06711056, 0.00583715))));
    var open = 0.0;
    for (var tap = 1; tap <= SUN_RAY_TAPS; tap++) {
        let at = uv + to_sun * ((f32(tap) - jitter) / f32(SUN_RAY_TAPS));
        let texel = clamp(vec2<i32>(at * size), vec2<i32>(0), last);
        open += select(0.0, 1.0, view_distance(textureLoad(scene_depth, texel, 0)) > SUN_RAY_OPEN_STUDS);
    }
    let reach = SUN_RAY_REACH * max(post.sun_rays.w, 0.05);
    return SUN_RAY_TINT * exp(-distance / reach) * (open / f32(SUN_RAY_TAPS));
}

/// Studs the depth-of-field blur ramps in over past the sharp zone's own edge, as
/// a multiple of InFocusRadius, and the floor on that length so an InFocusRadius
/// of 0 is a hard step rather than a division by zero. Both mirror
/// `lighting::effects`, which is where the choice is argued.
const DOF_FALLOFF_RADII: f32 = 1.0;
const DOF_MIN_FALLOFF: f32 = 1.0e-3;
/// What a pixel nothing was drawn into counts as, in studs: finite and absurdly
/// large rather than an infinity, since the ramp above subtracts distances and
/// inf - inf is a NaN. Mirrors BACKGROUND_DISTANCE in `camera`.
const BACKGROUND_DISTANCE: f32 = 1.0e9;

/// How far down the view axis the pixel at this depth stands, in studs.
///
/// Inverts the reversed-Z, infinite-far-plane projection the frame was drawn with
/// (`perspective_infinite_reverse`, see camera.rs): its only z terms are
/// clip.z = near and clip.w = -view_z, so depth is exactly near / distance and
/// the near plane in post.misc.z is the whole of what the inverse needs. Mirrors
/// `view_distance` in `camera`, where it is unit-tested against that very matrix.
fn view_distance(depth: f32) -> f32 {
    // Depth 0 is the clear value, i.e. a pixel no triangle was drawn into: the
    // sky, which sits behind everything. Reading it back as "at the eye" would
    // blur it as if it were in front of the focus plane.
    if depth <= 0.0 {
        return BACKGROUND_DISTANCE;
    }
    // Orthographic depth is linear in view-space Z (mirrors
    // camera.rs::orthographic_reversed_depth), not the hyperbolic perspective
    // mapping below, the two projections write depth by different formulas.
    // Its near plane is its own (negative) one, not post.misc.z: the answer
    // can legitimately come out negative for geometry behind the eye plane,
    // which dof_blur_factor already treats as "nearer than the focus plane".
    if post.camera.x > 0.5 {
        let far = post.camera.y;
        let near = post.camera.z;
        return far - depth * (far - near);
    }
    return post.misc.z / depth;
}

/// How much of the blurred frame a pixel `distance` studs out takes: 0 inside the
/// sharp zone, ramping to NearIntensity or FarIntensity either side of it.
///
/// Mirrors `DepthOfField::blur_factor` in `lighting::effects`, where the ramp is
/// unit-tested, Roblox publishes the four properties and nothing about the
/// falloff between them, so the ramp is this renderer's own (see
/// DOF_FALLOFF_RADII).
fn dof_blur_factor(distance: f32) -> f32 {
    let offset = distance - post.depth_of_field.x;
    // InFocusRadius is the sharp zone's extent on EACH side of FocusDistance
    // (Roblox's own docs: "the distance away from FocusDistance, on both
    // sides, where no blur is applied"), not a diameter to halve.
    let beyond = max(abs(offset) - post.depth_of_field.y, 0.0);
    let falloff = max(post.depth_of_field.y * DOF_FALLOFF_RADII, DOF_MIN_FALLOFF);
    let intensity = select(post.depth_of_field.w, post.depth_of_field.z, offset < 0.0);

    return min(beyond / falloff, 1.0) * intensity;
}

@fragment
fn fs_resolve(in: VertexOutput) -> @location(0) vec4<f32> {
    // A BlurEffect replaces the sharp frame outright, Roblox's own has no
    // partial-strength mix, so bloom (below) still adds its glow on top of
    // the now-blurred base rather than on top of the sharp geometry pass.
    let blurred = textureSample(blur, blur_sampler, in.uv).rgb;
    var color = select(
        textureSample(source, source_sampler, in.uv).rgb,
        blurred,
        post.misc.y > 0.5,
    );

    // DepthOfFieldEffect: the same single blurred copy of the frame, mixed back
    // in per pixel by how far the depth buffer says that pixel is from the focus
    // plane. One fixed radius rather than a per-pixel variable one (see
    // renderer::post's DOF_BLUR_SIZE), which is what makes it a mix instead of a
    // gather. Skipped where a BlurEffect already replaced the whole frame, since
    // mixing the blurred copy into itself cannot change a pixel.
    if post.misc.w > 0.5 && post.misc.y <= 0.5 {
        // clip_position is in pixels of this frame, which is exactly the depth
        // buffer's own resolution: no UV round trip to get half a texel wrong.
        let depth = textureLoad(scene_depth, vec2<i32>(in.clip_position.xy), 0);
        color = mix(color, blurred, dof_blur_factor(view_distance(depth)));
    }
    // Guarded, not just multiplied by zero: below quality level 5 the chain is
    // never drawn, and what that texture holds is then no business of ours.
    if post.bloom.x > 0.0 {
        color += textureSample(bloom, bloom_sampler, in.uv).rgb * post.bloom.x;
    }

    // Zero here covers every reason the effect must not draw this frame at
    // once: no SunRaysEffect, the moon lit instead of the sun, or the
    // projection falling behind the camera or off-screen (see
    // renderer::sun::sun_screen_position), none of that is this shader's
    // business to re-derive.
    if post.sun_rays.z > 0.0 {
        color += sun_rays(in.uv) * post.sun_rays.z;
    }

    // A plain clamp under Voxel and Compatibility, and under Retro whatever
    // the Technology (the real client's Retro ramps match its Voxel ones to
    // the step); the fitted filmic curve under ShadowMap and Future.
    let hdr = max(color, vec3<f32>(0.0));
    var mapped = clamp(hdr, vec3<f32>(0.0), vec3<f32>(1.0));
    if post.misc.x > 1.5 {
        mapped = filmic(hdr);
    }

    let layer = textureLoad(adornments, vec2<i32>(in.clip_position.xy), 0);
    if layer.a > 0.0 {
        mapped = composite_adornments(mapped, layer);
    }

    if post.correction.w > 0.5 {
        // Graded after the tone map, on gamma-encoded values: Roblox's grade
        // runs on the displayable frame, and a +0.1 Brightness has to lift
        // black by a tenth of the screen's range, done in linear light it
        // lifted it to sRGB 89 and greyed whole places out. The order (tint,
        // brightness, contrast about middle grey, saturation) is this
        // renderer's own choice; `ColorCorrection::apply` is the same grade
        // in Rust, where it is unit-tested.
        var display = pow(mapped, vec3<f32>(1.0 / DISPLAY_GAMMA));
        display = display * post.tint.rgb + vec3<f32>(post.correction.x);
        display = (display - vec3<f32>(0.5)) * (1.0 + post.correction.y) + vec3<f32>(0.5);
        display = mix(vec3<f32>(dot(display, LUMA)), display, 1.0 + post.correction.z);
        mapped = pow(saturate(display), vec3<f32>(DISPLAY_GAMMA));
    }

    return vec4<f32>(mapped, 1.0);
}

// Studio's tone curve for an unlit depth-tested adornment, measured in Studio
// 2026-11-24 off a ramp of opaque grey `BoxHandleAdornment`s: Color3 k/14
// against the sRGB value on screen. White lands on 223, not 255. Past white
// it comes off `SelectionSphere` outlines, which are drawn at twice their
// colour (see `renderer::adornment`): 1.32 lands on 242 and 2 on 253. The
// last knot only carries that slope on to 255, so that every value on screen
// has an inverse.
const TONE_KNOTS: u32 = 18u;
const TONE_IN = array<f32, 18>(
    0.0, 0.0714286, 0.142857, 0.214286, 0.285714, 0.357143, 0.428571, 0.5,
    0.571429, 0.642857, 0.714286, 0.785714, 0.857143, 0.928571, 1.0, 1.321429,
    2.0, 2.125,
);
const TONE_OUT = array<f32, 18>(
    0.0, 4.0, 16.0, 35.0, 58.0, 82.0, 106.0, 130.0,
    150.0, 168.0, 183.0, 196.0, 207.0, 216.0, 223.0, 242.0,
    253.0, 255.0,
);

fn tone(g: f32) -> f32 {
    var x = TONE_IN;
    var y = TONE_OUT;
    for (var k = 1u; k < TONE_KNOTS; k++) {
        if g <= x[k] || k == TONE_KNOTS - 1u {
            let t = (g - x[k - 1u]) / (x[k] - x[k - 1u]);
            return mix(y[k - 1u], y[k], t) / 255.0;
        }
    }
    return 1.0;
}

fn untone(display: f32) -> f32 {
    var x = TONE_IN;
    var y = TONE_OUT;
    let d = display * 255.0;
    for (var k = 1u; k < TONE_KNOTS; k++) {
        if d <= y[k] || k == TONE_KNOTS - 1u {
            let t = (d - y[k - 1u]) / (y[k] - y[k - 1u]);
            return mix(x[k - 1u], x[k], t);
        }
    }
    return x[TONE_KNOTS - 1u];
}

fn encode_srgb(c: f32) -> f32 {
    return select(1.055 * pow(c, 1.0 / 2.4) - 0.055, c * 12.92, c <= 0.0031308);
}

fn decode_srgb(c: f32) -> f32 {
    return select(pow((c + 0.055) / 1.055, 2.4), c / 12.92, c <= 0.04045);
}

// Studio blends a depth-tested adornment over the scene in sRGB values
// *before* its tone curve, at exactly `1 - Transparency` (measured: 0.751,
// 0.500, 0.250, 0.098 for 0.25, 0.5, 0.75, 0.9, where blending on screen
// would scatter by 0.09). This frame was never through that curve, so the
// pixel under the layer is taken back through its inverse, the layer laid
// over it, and the result put through the curve again: exact where no
// adornment is, and Studio's own arithmetic where one is.
fn composite_adornments(mapped: vec3<f32>, layer: vec4<f32>) -> vec3<f32> {
    var out = mapped;
    for (var c = 0; c < 3; c++) {
        let under = untone(encode_srgb(clamp(mapped[c], 0.0, 1.0)));
        out[c] = decode_srgb(tone(layer[c] + (1.0 - layer.a) * under));
    }
    return out;
}
