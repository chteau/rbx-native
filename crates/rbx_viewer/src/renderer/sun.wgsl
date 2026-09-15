// The sun and the moon: one camera-facing quad each, built in the vertex shader
// from the light direction the lighting uniform already carries.
//
// `lighting.wgsl` is concatenated in front of this file, which is where the
// direction comes from — the bodies move with the time of day for free.

@group(1) @binding(0) var image: texture_2d<f32>;
@group(1) @binding(1) var image_sampler: sampler;

struct VertexInput {
    // Corner of the unit quad, in [-1, 1], with its image coordinate.
    @location(0) corner: vec2<f32>,
    @location(1) uv: vec2<f32>,
    // tan(angular radius) in x; +1 for the sun, -1 for the moon, in y.
    @location(2) extent: vec2<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(vertex: VertexInput) -> VertexOutput {
    let direction = normalize(lighting.sun_direction.xyz) * vertex.extent.y;
    // Any axis that is not the direction itself does: the quad is round-ish and
    // the image has no up. Y unless the body is overhead, where Y is useless.
    let aside = select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), abs(direction.y) > 0.99);
    let right = normalize(cross(aside, direction));
    let up = cross(direction, right);

    let offset = (right * vertex.corner.x + up * vertex.corner.y) * vertex.extent.x;
    let clip = uniforms.view_projection * vec4<f32>(direction + offset, 1.0);

    var out: VertexOutput;
    // z = 0 is the far plane under reversed-Z (see camera.rs), so the disc sits
    // behind every piece of geometry and the depth test hides it wherever
    // something has already been drawn.
    out.clip_position = vec4<f32>(clip.xy, 0.0, clip.w);
    out.uv = vertex.uv;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let sample = textureSample(image, image_sampler, in.uv);
    // Premultiplied for the additive blend, and exposed like every other surface
    // in the frame. Roblox draws its own sun the same way: the disc adds light
    // to the sky rather than replacing it.
    return vec4<f32>(sample.rgb * sample.a * lighting.tuning.x, 1.0);
}
