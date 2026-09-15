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
    // x: 1 where a ColorGradingEffect resolved to TonemapperPreset.Retro, 0 for
    // Default (or no effect at all). y: 1 where a BlurEffect is enabled, 0
    // otherwise. z: the camera's near plane in studs, which is the whole of what
    // view_distance needs. w: 1 where a DepthOfFieldEffect is enabled.
    misc: vec4<f32>,
    // x/y: the sun's screen-space UV this frame (see
    // renderer::sun::sun_screen_position). z: SunRaysEffect.Intensity — 0 skips
    // the whole effect, whichever of "no effect", "moon lit" or "off-screen"
    // that came from. w: Spread.
    sun_rays: vec4<f32>,
    // DepthOfFieldEffect: x: FocusDistance and y: InFocusRadius, both in studs,
    // z: NearIntensity and w: FarIntensity, both 0-1 mixes. Whether any of it
    // applies at all is misc.w, not a zero in here.
    depth_of_field: vec4<f32>,
    // x: 1 on an orthographic frame, 0 on a perspective one — which
    // view_distance formula the depth buffer needs. y: the orthographic far
    // plane in studs, meaningless when x is 0. z/w unused.
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
// bound for reading now that the scene pass is over — in this group rather than a
// fifth one because a WebGPU device is only guaranteed four. Loaded, never
// sampled: a depth filtered across a silhouette is a depth no surface is at, and
// above one sample WGSL has no textureSample for a multisampled texture at all.
// That is also why `renderer::post` builds this shader twice, retyping this one
// line — see its DEPTH_BINDING.
@group(3) @binding(2) var scene_depth: texture_depth_2d;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

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

const SUN_RAY_TAPS: i32 = 12;
/// Per-tap falloff: the last of 12 taps carries about a sixth of the first
/// one's weight, which is soft enough that the march does not read as a row of
/// discrete dots even at the boosted `Intensity` a proof screenshot uses.
const SUN_RAY_DECAY: f32 = 0.85;

/// Standard god-rays: march toward sun with decreasing-weight average of HDR
/// color. Self-occlusion is free: dark pixels (occluded) contribute less than
/// bright sky. Spread is a fraction of pixel-to-sun distance, keeping mapping
/// in normalized units rather than inventing a pixel-radius conversion.
fn sun_rays(uv: vec2<f32>) -> vec3<f32> {
    let step = (post.sun_rays.xy - uv) * (post.sun_rays.w / f32(SUN_RAY_TAPS));

    var sample_uv = uv;
    var sum = vec3<f32>(0.0);
    var weight = 1.0;
    var total = 0.0;
    for (var tap = 0; tap < SUN_RAY_TAPS; tap++) {
        sample_uv += step;
        sum += textureSample(source, source_sampler, sample_uv).rgb * weight;
        total += weight;
        weight *= SUN_RAY_DECAY;
    }
    return sum / total;
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
    // mapping below — the two projections write depth by different formulas.
    if post.camera.x > 0.5 {
        let near = post.misc.z;
        let far = post.camera.y;
        return far - depth * (far - near);
    }
    return post.misc.z / depth;
}

/// How much of the blurred frame a pixel `distance` studs out takes: 0 inside the
/// sharp zone, ramping to NearIntensity or FarIntensity either side of it.
///
/// Mirrors `DepthOfField::blur_factor` in `lighting::effects`, where the ramp is
/// unit-tested — Roblox publishes the four properties and nothing about the
/// falloff between them, so the ramp is this renderer's own (see
/// DOF_FALLOFF_RADII).
fn dof_blur_factor(distance: f32) -> f32 {
    let offset = distance - post.depth_of_field.x;
    // InFocusRadius is the sharp zone's extent on EACH side of FocusDistance
    // (Roblox's own docs: "the distance away from FocusDistance, on both
    // sides, where no blur is applied") — not a diameter to halve.
    let beyond = max(abs(offset) - post.depth_of_field.y, 0.0);
    let falloff = max(post.depth_of_field.y * DOF_FALLOFF_RADII, DOF_MIN_FALLOFF);
    let intensity = select(post.depth_of_field.w, post.depth_of_field.z, offset < 0.0);

    return min(beyond / falloff, 1.0) * intensity;
}

@fragment
fn fs_resolve(in: VertexOutput) -> @location(0) vec4<f32> {
    // A BlurEffect replaces the sharp frame outright — Roblox's own has no
    // partial-strength mix — so bloom (below) still adds its glow on top of
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
    // renderer::sun::sun_screen_position) — none of that is this shader's
    // business to re-derive.
    if post.sun_rays.z > 0.0 {
        color += sun_rays(in.uv) * post.sun_rays.z;
    }

    // Roblox publishes no formula for either preset. `Default` is exactly the
    // plain clamp this renderer always used, so a place with no
    // ColorGradingEffect (or one that resolves to Default) is unchanged; the
    // only "curve" it has ever had is the `*Srgb` target's own gamma encode on
    // write, which happens after this shader and cannot be toggled per effect.
    // That rules out approximating `Retro` by "removing" a sqrt this pipeline
    // never applied — it would just collapse back to Default. A Reinhard
    // rolloff (`color / (1 + color)`) is used instead: a real, well-known
    // legacy tonemap that reads flatter than the clamp, which is the
    // principled approximation this renderer can actually justify.
    let hdr = max(color, vec3<f32>(0.0));
    var mapped = select(
        clamp(hdr, vec3<f32>(0.0), vec3<f32>(1.0)),
        hdr / (vec3<f32>(1.0) + hdr),
        post.misc.x > 0.5,
    );

    if post.correction.w > 0.5 {
        // Graded after the tone map, on gamma-encoded values: Roblox's grade
        // runs on the displayable frame, and a +0.1 Brightness has to lift
        // black by a tenth of the screen's range — done in linear light it
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
