// A `BillboardGui`/`SurfaceGui` canvas placed in the world (see
// `crate::scene::gui::space`).
//
// The canvas itself was painted by `gui.wgsl` into an offscreen sRGB texture,
// so all that is left here is a textured quad: every vertex already sits at its
// final world position (a billboard's camera-facing corners are resolved on the
// CPU each frame, exactly like a beam's ribbon), and the sampler's own sRGB
// decode is what puts the canvas back into the linear space the HDR target
// works in.

struct Camera {
    view_projection: mat4x4<f32>,
}

@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var canvas: texture_2d<f32>;
@group(1) @binding(1) var canvas_sampler: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) brightness: f32,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) brightness: f32,
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = camera.view_projection * vec4<f32>(in.position, 1.0);
    out.uv = in.uv;
    out.brightness = in.brightness;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let sampled = textureSample(canvas, canvas_sampler, in.uv);
    // Straight alpha, matching the pipeline's (SrcAlpha, OneMinusSrcAlpha)
    // state: the canvas was composited with that same blend, so carrying it
    // into the scene un-premultiplied keeps the two passes consistent. Only
    // the colour is scaled by `Brightness`; the container's own transparency
    // is the tree's, not the light's.
    return vec4<f32>(sampled.rgb * in.brightness, sampled.a);
}
