// The star field: one camera-facing speck per star, on the far plane.
//
// `lighting.wgsl` is concatenated in front of this file, which is where the
// rotation-only camera and the star fade (`sky_tint.w`, 0 by day and 1 at
// night) come from.

struct VertexInput {
    @location(0) direction: vec3<f32>,
    // Corner of the unit quad, in [-1, 1].
    @location(1) corner: vec2<f32>,
    @location(2) magnitude: f32,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) corner: vec2<f32>,
    @location(1) magnitude: f32,
}

// Angular radius of one star, in radians: a shade under a tenth of a degree,
// which is a speck a couple of pixels across at any reasonable field of view.
// Fixed in angle rather than in pixels, so the field does not grow denser as the
// frame shrinks.
const STAR_ANGULAR_RADIUS: f32 = 0.0009;
// How bright the brightest star draws, in the linear radiance everything else
// in the frame is in. Low on purpose: stars are the dimmest thing in any sky,
// and nothing here may reach the bloom threshold.
const STAR_RADIANCE: f32 = 0.5;

@vertex
fn vs_main(vertex: VertexInput) -> VertexOutput {
    let direction = normalize(vertex.direction);
    // Any axis that is not the direction itself does, a speck having no up.
    let aside = select(vec3<f32>(0.0, 1.0, 0.0), vec3<f32>(1.0, 0.0, 0.0), abs(direction.y) > 0.99);
    let right = normalize(cross(aside, direction));
    let up = cross(direction, right);

    let offset = (right * vertex.corner.x + up * vertex.corner.y) * STAR_ANGULAR_RADIUS;
    let clip = uniforms.view_projection * vec4<f32>(direction + offset, 1.0);

    var out: VertexOutput;
    // z = 0 is the far plane under reversed-Z (see camera.rs): a star sits
    // behind every piece of geometry and behind the sky itself.
    out.clip_position = vec4<f32>(clip.xy, 0.0, clip.w);
    out.corner = vertex.corner;
    out.magnitude = vertex.magnitude;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // A round speck with a soft edge rather than a square one: at two pixels
    // across, the corners of a quad are the whole difference between a star and
    // a pixel of dust.
    let falloff = saturate(1.0 - dot(in.corner, in.corner));
    let brightness = in.magnitude * falloff * falloff * lighting.sky_tint.w * lighting.tuning.x;

    // Premultiplied for the additive blend, like the sun and moon discs.
    return vec4<f32>(vec3<f32>(STAR_RADIANCE * brightness), 1.0);
}
