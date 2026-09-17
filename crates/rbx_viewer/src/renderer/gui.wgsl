// A `ScreenGui`'s rectangles (see `crate::scene::gui`), drawn straight over the
// finished frame.
//
// Every vertex arrives in viewport pixels with the origin at the top-left
// corner — the frame `UDim2` is written in — so the only transform here is the
// orthographic flip into clip space. There is no camera and no depth: the pass
// is a painter's-algorithm overlay, ordered entirely on the CPU.
//
// The fragment shapes each pixel by a signed distance to the element's
// rounded outline (`UICorner`), evaluated in the element's own unrotated
// frame carried per vertex, keeps only the band of distances the quad asked
// for (a fill or a `UIStroke`), and multiplies in the `UIGradient` ramp baked
// into bind group 2.

struct Viewport {
    // Width and height in pixels; the trailing pair only pads to the 16-byte
    // uniform alignment.
    size: vec2<f32>,
    padding: vec2<f32>,
}

@group(0) @binding(0) var<uniform> viewport: Viewport;
@group(1) @binding(0) var image: texture_2d<f32>;
@group(1) @binding(1) var image_sampler: sampler;
@group(2) @binding(0) var gradients: texture_2d<f32>;
@group(2) @binding(1) var gradient_sampler: sampler;

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec3<f32>,
    @location(3) alpha: f32,
    @location(4) local: vec2<f32>,
    @location(5) half: vec2<f32>,
    @location(6) radii: vec4<f32>,
    @location(7) band: vec2<f32>,
    @location(8) gradient: vec4<f32>,
    @location(9) gradient_row: f32,
    @location(10) mode: u32,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec3<f32>,
    @location(2) alpha: f32,
    @location(3) local: vec2<f32>,
    @location(4) @interpolate(flat) half: vec2<f32>,
    @location(5) @interpolate(flat) radii: vec4<f32>,
    @location(6) @interpolate(flat) band: vec2<f32>,
    @location(7) @interpolate(flat) gradient: vec4<f32>,
    @location(8) @interpolate(flat) gradient_row: f32,
    @location(9) @interpolate(flat) mode: u32,
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    let normalized = in.position / viewport.size;
    out.clip_position = vec4<f32>(
        normalized.x * 2.0 - 1.0,
        1.0 - normalized.y * 2.0,
        0.0,
        1.0,
    );
    out.uv = in.uv;
    out.color = in.color;
    out.alpha = in.alpha;
    out.local = in.local;
    out.half = in.half;
    out.radii = in.radii;
    out.band = in.band;
    out.gradient = in.gradient;
    out.gradient_row = in.gradient_row;
    out.mode = in.mode;
    return out;
}

const TAU: f32 = 6.283185307;
const RAMP_WIDTH: f32 = 256.0;

// The radius of whichever corner `p` (relative to the centre, y down) is
// nearest: `radii` runs top-left, top-right, bottom-right, bottom-left.
fn corner_radius(p: vec2<f32>, radii: vec4<f32>) -> f32 {
    if p.x >= 0.0 {
        return select(radii.y, radii.z, p.y >= 0.0);
    }
    return select(radii.x, radii.w, p.y >= 0.0);
}

// Signed distance from `p` to a box of `half` extents whose corner has
// radius `r`, negative inside. Outside a *sharp* corner the metric follows
// `LineJoinMode` (0 round, 1 bevel, 2 miter), which is what shapes a stroke
// band's outer corner; a rounded corner is round whatever the join.
fn box_distance(p: vec2<f32>, half: vec2<f32>, r: f32, join: u32) -> f32 {
    let q = abs(p) - half + vec2<f32>(r);
    let inside = min(max(q.x, q.y), 0.0);
    let o = max(q, vec2<f32>(0.0));
    var outside = length(o);
    if r <= 0.0 {
        if join == 2u {
            outside = max(o.x, o.y);
        } else if join == 1u {
            outside = max(max(o.x, o.y), (o.x + o.y) * 0.70710678);
        }
    }
    return inside + outside - r;
}

// Where `p` falls along the ramp, per `GradientType` (0 linear, 1 radial,
// 2 conical); see `scene::gui::layout::GradientPx::axis` for what `g` holds.
fn ramp_position(p: vec2<f32>, g: vec4<f32>, kind: u32) -> f32 {
    let d = p - g.xy;
    if kind == 1u {
        return length(d) * g.z;
    }
    if kind == 2u {
        // y down makes a growing angle a clockwise sweep, as the docs say.
        let angle = atan2(d.y, d.x) - g.z;
        return fract(angle / TAU) * g.w;
    }
    return 0.5 + dot(d, g.zw);
}

// `GradientTileMode`: 0 clamp, 1 repeat, 2 mirror.
fn tiled(t: f32, tile: u32) -> f32 {
    if tile == 1u {
        return fract(t);
    }
    if tile == 2u {
        return 1.0 - abs(fract(t * 0.5) * 2.0 - 1.0);
    }
    return clamp(t, 0.0, 1.0);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let join = in.mode & 3u;
    let kind = (in.mode >> 2u) & 3u;
    let tile = (in.mode >> 4u) & 3u;

    let r = corner_radius(in.local, in.radii);
    let d = box_distance(in.local, in.half, r, join);
    // One pixel of linear ramp at each edge of the band: the edge itself
    // lands on half coverage, so a pixel-aligned sharp box stays crisp.
    let coverage = clamp(d - in.band.x + 0.5, 0.0, 1.0) * clamp(in.band.y - d + 0.5, 0.0, 1.0);

    var color = in.color;
    var alpha = in.alpha * coverage;
    if in.gradient_row >= 0.0 {
        let t = tiled(ramp_position(in.local, in.gradient, kind), tile);
        // Texel centres: the first and last texel are the ramp's exact ends.
        let u = (t * (RAMP_WIDTH - 1.0) + 0.5) / RAMP_WIDTH;
        let rows = f32(textureDimensions(gradients).y);
        let v = (in.gradient_row + 0.5) / rows;
        // `textureSampleLevel` rather than `textureSample`: inside a branch,
        // which forbids the implicit derivatives the latter needs.
        let ramp = textureSampleLevel(gradients, gradient_sampler, vec2<f32>(u, v), 0.0);
        color *= ramp.rgb;
        alpha *= ramp.a;
    }

    let sampled = textureSample(image, image_sampler, in.uv);
    // Straight (non-premultiplied) alpha: the pipeline's blend state is
    // (SrcAlpha, OneMinusSrcAlpha), which is what Roblox's own
    // `BackgroundTransparency`/`ImageTransparency` compose as.
    return vec4<f32>(sampled.rgb * color, sampled.a * alpha);
}
