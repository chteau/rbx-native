// An adornment's lines: `LineHandleAdornment`, whose `Thickness` is
// documented in pixels, and the circle a `SelectionSphere` outlines itself
// with. Each edge arrives as a screen-space quad and is pushed out to its
// own width in the vertex shader, the same way `renderer::outline`'s box
// edges are — with the width and the colour carried per vertex here,
// because every adornment picks both for itself.

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) other: vec3<f32>,
    @location(2) side: f32,
    @location(3) half_width: f32,
    @location(4) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

struct Frame {
    view_proj: mat4x4<f32>,
    viewport: vec4<f32>,
};
@group(0) @binding(0)
var<uniform> frame: Frame;

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    let clip_this = frame.view_proj * vec4<f32>(input.position, 1.0);
    let clip_other = frame.view_proj * vec4<f32>(input.other, 1.0);
    let half_vp = 0.5 * frame.viewport.xy;

    let px_this = clip_this.xy / clip_this.w * half_vp;
    let px_other = clip_other.xy / clip_other.w * half_vp;
    var dir = px_other - px_this;
    let len = length(dir);
    dir = select(vec2<f32>(1.0, 0.0), dir / len, len > 1e-5);
    let perp = vec2<f32>(-dir.y, dir.x);
    let px = px_this + perp * input.side * input.half_width;

    var out: VertexOutput;
    // Back to clip space keeping this end's own depth and w, so the line is
    // still depth-tested where the edge really is.
    out.clip_position = vec4<f32>(px / half_vp * clip_this.w, clip_this.z, clip_this.w);
    out.color = input.color;
    return out;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return input.color;
}
