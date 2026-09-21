// An editor overlay's dots: a disc a fixed number of pixels across round a
// world-space point, whatever its distance — the size Studio's draggers hold
// their `SphereHandleAdornment` markers at. Each arrives as a square pushed
// out to size in the vertex shader, the way `adornment_line.wgsl` pushes a
// line out to its width, and is trimmed to a circle here with a pixel of
// soft edge.

struct VertexInput {
    @location(0) centre: vec3<f32>,
    @location(1) corner: vec2<f32>,
    @location(2) radius: f32,
    @location(3) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    // Where this fragment is from the centre, in pixels, and the radius.
    @location(1) offset: vec2<f32>,
    @location(2) radius: f32,
};

struct Frame {
    view_proj: mat4x4<f32>,
    viewport: vec4<f32>,
};
@group(0) @binding(0)
var<uniform> frame: Frame;

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    let clip = frame.view_proj * vec4<f32>(input.centre, 1.0);
    let half_vp = 0.5 * frame.viewport.xy;
    // A pixel of margin past the radius, for the soft edge to fade across.
    let offset = input.corner * (input.radius + 1.0);
    let px = clip.xy / clip.w * half_vp + offset;

    var out: VertexOutput;
    out.clip_position = vec4<f32>(px / half_vp * clip.w, clip.z, clip.w);
    out.color = input.color;
    out.offset = offset;
    out.radius = input.radius;
    return out;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let coverage = clamp(input.radius + 0.5 - length(input.offset), 0.0, 1.0);
    if coverage <= 0.0 {
        discard;
    }
    return vec4<f32>(input.color.rgb, input.color.a * coverage);
}
