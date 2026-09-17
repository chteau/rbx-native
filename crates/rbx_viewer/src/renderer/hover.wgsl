// The "about to click" cue: box edges around whatever a click would select
// under the cursor, tested against the depth buffer the rest of the scene pass
// already wrote — the same screen-space thick-edge approach `selection.wgsl`
// draws the Explorer selection with, but deliberately not the same colour: an
// amber, alpha-blended line reads as a distinct, dimmer cue rather than a
// second selection box.

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) other: vec3<f32>,
    @location(2) side: f32,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
};

// Bind group 0 is the renderer's shared frame uniform (see
// `renderer::pipeline::frame_layout`): the view-projection matrix, then the
// viewport's pixel size in `viewport.xy`.
struct Frame {
    view_proj: mat4x4<f32>,
    viewport: vec4<f32>,
};
@group(0) @binding(0)
var<uniform> frame: Frame;

// Half the outline's on-screen width, in pixels — a touch thinner than the
// selection box, so the two read as different cues where they overlap.
const HALF_WIDTH: f32 = 1.25;

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

    let ndc = px / half_vp;
    var out: VertexOutput;
    out.clip_position = vec4<f32>(ndc * clip_this.w, clip_this.z, clip_this.w);
    return out;
}

// A warm amber, sRGB (0.98, 0.65, 0.15) linearized the same way
// `scene::srgb_to_linear` linearizes every other part color — chosen for
// contrast against the selection's blue rather than to match any particular
// Studio convention (Studio itself has no separate hover outline). Drawn at
// less than full alpha, through this pipeline's own blend state (see
// `renderer::hover::Hover::new`), so it reads as a dimmer cue even where it
// happens to sit beside the selection box's harder edge.
const COLOR: vec4<f32> = vec4<f32>(0.955, 0.380, 0.020, 0.55);

@fragment
fn fs_main(_input: VertexOutput) -> @location(0) vec4<f32> {
    return COLOR;
}
