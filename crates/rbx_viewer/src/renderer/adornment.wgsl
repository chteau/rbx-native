// Solid adornment geometry: unlit, vertex-coloured triangles drawn into the
// scene's HDR target, so an adornment tone maps with everything else. The
// colours arrive linearized (see `scene::srgb_to_linear`), like every other
// colour this renderer is handed.

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

// Bind group 0 is the renderer's shared frame uniform: the view-projection
// matrix, then the viewport's pixel size.
struct Frame {
    view_proj: mat4x4<f32>,
    viewport: vec4<f32>,
};
@group(0) @binding(0)
var<uniform> frame: Frame;

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = frame.view_proj * vec4<f32>(input.position, 1.0);
    out.color = input.color;
    return out;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return input.color;
}
