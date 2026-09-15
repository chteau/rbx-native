// A `ScreenGui`'s rectangles (see `crate::scene::gui`), drawn straight over the
// finished frame.
//
// Every vertex arrives in viewport pixels with the origin at the top-left
// corner — the frame `UDim2` is written in — so the only transform here is the
// orthographic flip into clip space. There is no camera and no depth: the pass
// is a painter's-algorithm overlay, ordered entirely on the CPU.

struct Viewport {
    // Width and height in pixels; the trailing pair only pads to the 16-byte
    // uniform alignment.
    size: vec2<f32>,
    padding: vec2<f32>,
}

@group(0) @binding(0) var<uniform> viewport: Viewport;
@group(1) @binding(0) var image: texture_2d<f32>;
@group(1) @binding(1) var image_sampler: sampler;

struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec3<f32>,
    @location(3) alpha: f32,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec3<f32>,
    @location(2) alpha: f32,
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
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let sampled = textureSample(image, image_sampler, in.uv);
    // Straight (non-premultiplied) alpha: the pipeline's blend state is
    // (SrcAlpha, OneMinusSrcAlpha), which is what Roblox's own
    // `BackgroundTransparency`/`ImageTransparency` compose as.
    return vec4<f32>(sampled.rgb * in.color, sampled.a * in.alpha);
}
