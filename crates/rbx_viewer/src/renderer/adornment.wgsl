// Solid adornment geometry: unlit, vertex-coloured triangles. The colours
// arrive sRGB-encoded, the values Studio blends adornments in (see
// `renderer::adornment`).

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) single_sided: f32,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    @location(1) single_sided: f32,
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
    out.single_sided = input.single_sided;
    return out;
}

// Every triangle is wound counter-clockwise from outside its shape, so a
// back face of a `DoubleSided = false` adornment is one turned away from
// the eye.
fn shade(input: VertexOutput, front: bool) -> vec4<f32> {
    if !front && input.single_sided > 0.5 {
        discard;
    }
    return input.color;
}

// Into the depth-tested layer: hidden by the scene's own depth (reversed-Z,
// so nearer is greater), and premultiplied for the layer's "over".
@fragment
fn fs_layer(input: VertexOutput, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    let color = shade(input, front);
    if input.clip_position.z < scene_depth(vec2<i32>(input.clip_position.xy)) {
        discard;
    }
    return vec4<f32>(color.rgb * color.a, color.a);
}

// Onto the finished frame, blended by the pipeline in sRGB values.
@fragment
fn fs_top(input: VertexOutput, @builtin(front_facing) front: bool) -> @location(0) vec4<f32> {
    return shade(input, front);
}
