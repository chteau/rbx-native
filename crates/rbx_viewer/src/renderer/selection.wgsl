// The Explorer selection's outline: unlit `LineList` edges in Studio's
// selection-box blue, tested against the depth buffer the rest of the scene
// pass already wrote.

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

// Studio's selection-box blue, sRGB (0.36, 0.71, 0.96) linearized the same way
// `scene::srgb_to_linear` linearizes every other part color, so it reads as
// the same blue once this renderer's HDR target is tonemapped.
const COLOR: vec4<f32> = vec4<f32>(0.106, 0.462, 0.911, 1.0);

@fragment
fn fs_main(_input: VertexOutput) -> @location(0) vec4<f32> {
    return COLOR;
}
