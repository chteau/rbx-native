// The six panels of a Sky, pasted on the far plane.
//
// `lighting.wgsl` is concatenated in front of this file, which is where bind
// group 0 (the rotation-only camera and the lighting uniform) is declared and
// where the atmosphere the sky is hazed by lives.

@group(1) @binding(0) var image: texture_2d<f32>;
@group(1) @binding(1) var image_sampler: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    // The panels sit on a cube centred on the eye, so a vertex position *is*
    // the direction it is seen along — which is what the haze thickens with.
    @location(1) direction: vec3<f32>,
}

@vertex
fn vs_main(vertex: VertexInput) -> VertexOutput {
    let clip = uniforms.view_projection * vec4<f32>(vertex.position, 1.0);

    var out: VertexOutput;
    // z = w pins every corner to the far plane after the perspective divide, so
    // the cube the panels sit on can be any size at all and never clips.
    out.clip_position = clip.xyww;
    out.uv = vertex.uv;
    out.direction = vertex.position;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Unlit on purpose: a skybox is the light source, not something lit by it.
    // It still goes through the atmosphere and the exposure, or the horizon
    // would stay the panel's own blue while every distant surface hazes white.
    let sky = textureSample(image, image_sampler, in.uv).rgb;
    return vec4<f32>(shade_sky(sky, normalize(in.direction)), 1.0);
}
