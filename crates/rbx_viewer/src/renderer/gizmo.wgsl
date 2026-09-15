// The transform gizmo's draggers: unlit, vertex-coloured triangles drawn over
// whatever the scene already put there, so a handle buried inside the part it
// belongs to is still grabbable.

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) color: vec3<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec3<f32>,
};

// Bind group 0 is the renderer's shared frame uniform (see
// `renderer::pipeline::frame_layout`); like the selection outline, this only
// reads the view-projection matrix at binding 0 and leaves the lighting/env/
// shadow bindings alone.
@group(0) @binding(0)
var<uniform> view_proj: mat4x4<f32>;

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = view_proj * vec4<f32>(input.position, 1.0);
    out.color = input.color;
    return out;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(input.color, 1.0);
}
