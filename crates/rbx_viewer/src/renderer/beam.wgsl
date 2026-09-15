// A `Beam`'s ribbon (see `crate::scene::beam` and `renderer::beam::ribbon`).
//
// Unlike `particles.wgsl`, every vertex here already sits at its final world
// position: `FaceCamera`, the curve and the width taper are all resolved on
// the CPU each frame (see `ribbon::vertices`), so this shader only projects
// and shades. The `LightEmission` blend trick is identical to particles' —
// see that shader's `fs_main` for why one blend state covers both straight
// alpha and additive.

struct Camera {
    view_projection: mat4x4<f32>,
}

@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var image: texture_2d<f32>;
@group(1) @binding(1) var image_sampler: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec3<f32>,
    @location(3) alpha: f32,
    @location(4) light_emission: f32,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec3<f32>,
    @location(2) alpha: f32,
    @location(3) light_emission: f32,
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = camera.view_projection * vec4<f32>(in.position, 1.0);
    out.uv = in.uv;
    out.color = in.color;
    out.alpha = in.alpha;
    out.light_emission = in.light_emission;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let sampled = textureSample(image, image_sampler, in.uv);
    let alpha = sampled.a * in.alpha;
    let color = sampled.rgb * in.color;
    // Premultiplied output with the pipeline's (One, OneMinusSrcAlpha) blend
    // state; see `particles.wgsl`'s `fs_main` for the full reasoning.
    return vec4<f32>(color * alpha, alpha * (1.0 - in.light_emission));
}
