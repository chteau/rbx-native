// `ParticleEmitter2D` particles, drawn inside the GUI pass at their parent's
// place in the paint order (see `renderer::gui::particles`).
//
// Same viewport pixels and encoded-space compositing as `gui.wgsl`. What
// differs is the blend, measured in Studio: `Brightness` multiplies the
// encoded colour, and `Blend` slides from normal (0) to additive (1), the
// result being `colour * alpha + under * (1 - alpha * (1 - Blend))`, which
// a premultiplied source with `alpha * (1 - Blend)` in its alpha gives.

struct Viewport {
    size: vec2<f32>,
    padding: vec2<f32>,
}

@group(0) @binding(0) var<uniform> viewport: Viewport;
@group(1) @binding(0) var image: texture_2d<f32>;
@group(1) @binding(1) var image_sampler: sampler;

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) uv_next: vec2<f32>,
    @location(3) between: f32,
    @location(4) color: vec3<f32>,
    @location(5) alpha: f32,
    @location(6) brightness: f32,
    @location(7) blend: f32,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) uv_next: vec2<f32>,
    @location(2) between: f32,
    @location(3) color: vec3<f32>,
    @location(4) alpha: f32,
    @location(5) brightness: f32,
    @location(6) blend: f32,
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    let normalized = in.position / viewport.size;
    out.clip_position = vec4<f32>(normalized.x * 2.0 - 1.0, 1.0 - normalized.y * 2.0, 0.0, 1.0);
    out.uv = in.uv;
    out.uv_next = in.uv_next;
    out.between = in.between;
    out.color = in.color;
    out.alpha = in.alpha;
    out.brightness = in.brightness;
    out.blend = in.blend;
    return out;
}

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let low = c * 12.92;
    let high = 1.055 * pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(high, low, c <= vec3<f32>(0.04045 / 12.92));
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // A flipbook crossfading from one frame to the next.
    let sampled = mix(
        textureSample(image, image_sampler, in.uv),
        textureSample(image, image_sampler, in.uv_next),
        in.between,
    );
    let color = min(linear_to_srgb(sampled.rgb * in.color) * in.brightness, vec3<f32>(1.0));
    let alpha = sampled.a * in.alpha;
    return vec4<f32>(color * alpha, alpha * (1.0 - in.blend));
}
