// The "about to click" cue: unlit `LineList` edges around whatever `BasePart`
// is under the cursor, tested against the depth buffer the rest of the scene
// pass already wrote — the same approach `selection.wgsl` draws the Explorer
// selection outline with, but deliberately not the same colour: an amber,
// alpha-blended line reads as a distinct, dimmer cue rather than a second
// selection box.

struct VertexInput {
    @location(0) position: vec3<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
};

// Bind group 0 is the renderer's shared frame uniform (see
// `renderer::pipeline::frame_layout`); the outline only reads the
// view-projection matrix at binding 0 and leaves the lighting/env/shadow
// bindings the shaded surfaces use at the others untouched.
@group(0) @binding(0)
var<uniform> view_proj: mat4x4<f32>;

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = view_proj * vec4<f32>(input.position, 1.0);
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
