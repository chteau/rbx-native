// The Explorer selection's outline: box edges in Studio's selection-box blue,
// tested against the depth buffer the rest of the scene pass already wrote.
//
// Each edge arrives as a screen-space quad (two triangles) rather than a
// one-pixel `LineList` line: the vertex shader pushes the two corners on each
// end out to either side of the line by a constant number of pixels, so the
// outline holds the same real thickness whatever the camera distance — the way
// Studio's own selection box does, and unlike a hairline that vanishes against
// busy geometry (see `renderer::outline` for the vertex layout).

struct VertexInput {
    // This corner's own end of the edge, the edge's far end (which fixes the
    // on-screen direction of the line), and which side of it to expand to.
    @location(0) position: vec3<f32>,
    @location(1) other: vec3<f32>,
    @location(2) side: f32,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
};

// Bind group 0 is the renderer's shared frame uniform (see
// `renderer::pipeline::frame_layout`): the view-projection matrix, then the
// viewport's pixel size in `viewport.xy`. The lighting/env/shadow bindings the
// shaded surfaces use at the other slots are left untouched.
struct Frame {
    view_proj: mat4x4<f32>,
    viewport: vec4<f32>,
};
@group(0) @binding(0)
var<uniform> frame: Frame;

// Half the outline's on-screen width, in pixels — a ~3px line, close to
// Studio's own selection box.
const HALF_WIDTH: f32 = 1.5;

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    let clip_this = frame.view_proj * vec4<f32>(input.position, 1.0);
    let clip_other = frame.view_proj * vec4<f32>(input.other, 1.0);
    let half_vp = 0.5 * frame.viewport.xy;

    // Both ends in pixel space, so the width is measured on screen rather than
    // in the world.
    let px_this = clip_this.xy / clip_this.w * half_vp;
    let px_other = clip_other.xy / clip_other.w * half_vp;
    var dir = px_other - px_this;
    let len = length(dir);
    dir = select(vec2<f32>(1.0, 0.0), dir / len, len > 1e-5);
    let perp = vec2<f32>(-dir.y, dir.x);
    let px = px_this + perp * input.side * HALF_WIDTH;

    // Back to clip space, keeping this end's own depth and w so the outline is
    // still depth-tested against the scene exactly where the edge really is.
    let ndc = px / half_vp;
    var out: VertexOutput;
    out.clip_position = vec4<f32>(ndc * clip_this.w, clip_this.z, clip_this.w);
    return out;
}

// Studio's selection-box blue, sRGB (0.36, 0.71, 0.96) linearized the same way
// `scene::srgb_to_linear` linearizes every other part color, so it reads as
// the same blue once this renderer's HDR target is tonemapped.
const COLOR: vec4<f32> = vec4<f32>(0.106, 0.462, 0.911, 1.0);

@fragment
fn fs_main(_input: VertexOutput) -> @location(0) vec4<f32> {
    return COLOR;
}
