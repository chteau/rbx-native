// An `ImageHandleAdornment`: its image on a flat quad in the adornment's own
// frame, multiplied by `Color3` (as Studio draws it) and faded by the
// adornment's own `Transparency`, all in sRGB values.

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) single_sided: f32,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) single_sided: f32,
};

struct Frame {
    view_proj: mat4x4<f32>,
    viewport: vec4<f32>,
};
@group(0) @binding(0)
var<uniform> frame: Frame;

@group(1) @binding(0)
var image: texture_2d<f32>;
@group(1) @binding(1)
var image_sampler: sampler;

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = frame.view_proj * vec4<f32>(input.position, 1.0);
    out.uv = input.uv;
    out.color = input.color;
    out.single_sided = input.single_sided;
    return out;
}

fn encode(c: vec3<f32>) -> vec3<f32> {
    return select(
        1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055,
        c * 12.92,
        c <= vec3<f32>(0.0031308),
    );
}

fn shade(input: VertexOutput, front: bool) -> vec4<f32> {
    if !front && input.single_sided > 0.5 {
        discard;
    }
    // The image is uploaded sRGB, so the sampler hands it back linear.
    let texel = textureSample(image, image_sampler, input.uv);
    return vec4<f32>(encode(texel.rgb) * input.color.rgb, texel.a * input.color.a);
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
