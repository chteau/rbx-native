// A preview's ghost boxes: the same screen-space edge quads the selection
// and hover outlines are drawn from (see `renderer::outline`), in a colour
// of their own.

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) other: vec3<f32>,
    @location(2) side: f32,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
};

struct Frame {
    view_proj: mat4x4<f32>,
    viewport: vec4<f32>,
};
@group(0) @binding(0)
var<uniform> frame: Frame;

// Thinner than the selection's own ~3px box: a preview is a suggestion
// standing beside the real thing, not the thing itself.
const HALF_WIDTH: f32 = 1.0;

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
    let px = px_this + perp * input.side * HALF_WIDTH;

    var out: VertexOutput;
    out.clip_position = vec4<f32>(px / half_vp * clip_this.w, clip_this.z, clip_this.w);
    return out;
}

// A pale green, linearized the same way `scene::srgb_to_linear` linearizes
// every other colour here, at half alpha so the geometry it is drawn over
// still reads through it. Roblox publishes no colour for Studio's own align
// preview, so this is chosen to sit apart from the selection's blue and the
// hover cue's amber rather than to match one.
const COLOR: vec4<f32> = vec4<f32>(0.278, 0.760, 0.354, 0.5);

@fragment
fn fs_main(_input: VertexOutput) -> @location(0) vec4<f32> {
    return COLOR;
}
