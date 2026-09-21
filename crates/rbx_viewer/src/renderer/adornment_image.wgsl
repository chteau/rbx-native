// An `ImageHandleAdornment`: its image on a flat quad in the adornment's own
// frame, tinted by nothing — `GuiBase3d.Color3` colours the shapes, and
// nothing documents it as tinting an image — and faded by the adornment's
// own `Transparency`.

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) alpha: f32,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) alpha: f32,
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
    out.alpha = input.alpha;
    return out;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let texel = textureSample(image, image_sampler, input.uv);
    return vec4<f32>(texel.rgb, texel.a * input.alpha);
}
