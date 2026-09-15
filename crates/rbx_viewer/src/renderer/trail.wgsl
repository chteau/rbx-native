// A `Trail`'s ribbon (see `crate::scene::trail` and `renderer::trail::ribbon`).
//
// Every vertex here already sits at its final world position: `FaceCamera`
// (implicit for every trail — see `scene::trail::Trail`'s doc), width scaling
// and colour/transparency sampling are all resolved on the CPU each frame, so
// this shader only projects and shades. Identical to `beam.wgsl` — see that
// shader's doc for why one blend state covers both straight alpha and
// additive `LightEmission`.

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
    // state; see `beam.wgsl`'s `fs_main` for the full reasoning.
    return vec4<f32>(color * alpha, alpha * (1.0 - in.light_emission));
}
