// Camera-facing billboards for `ParticleEmitter` (see `crate::scene::particles`).
//
// Unlit: particles read no lighting uniform at all, matching how Roblox itself
// shades them (`LightInfluence` is a documented TODO). `LightEmission` instead
// lerps the blend equation itself between straight alpha and additive — see
// `fs_main` — by scaling the premultiplied output alpha down to zero as it
// approaches 1.

struct Camera {
    view_projection: mat4x4<f32>,
    eye: vec3<f32>,
}

@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var image: texture_2d<f32>;
@group(1) @binding(1) var image_sampler: sampler;

struct VertexInput {
    // Unit quad corner in [-1, 1], shared by every instance (see `renderer::particles::QUAD_CORNERS`).
    @location(0) corner: vec2<f32>,
    @location(1) position: vec3<f32>,
    @location(2) size: f32,
    @location(3) color: vec3<f32>,
    @location(4) alpha: f32,
    @location(5) rotation: f32,
    @location(6) light_emission: f32,
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
    let to_camera = camera.eye - in.position;
    let distance = length(to_camera);
    // Degenerate only when the eye sits exactly on the particle, which a real
    // camera never does; the fallback just avoids a NaN from dividing by zero.
    let forward = select(vec3<f32>(0.0, 0.0, 1.0), to_camera / max(distance, 1e-4), distance > 1e-4);
    var right = cross(vec3<f32>(0.0, 1.0, 0.0), forward);
    if (dot(right, right) < 1e-6) {
        // Looking straight down/up at the particle: world-up cannot build a
        // basis with `forward`, so fall back to world-right instead.
        right = cross(vec3<f32>(1.0, 0.0, 0.0), forward);
    }
    right = normalize(right);
    let up = cross(forward, right);

    let c = cos(in.rotation);
    let s = sin(in.rotation);
    let spun = vec2<f32>(in.corner.x * c - in.corner.y * s, in.corner.x * s + in.corner.y * c);
    let half_size = in.size * 0.5;
    let world_position = in.position + (right * spun.x + up * spun.y) * half_size;

    var out: VertexOutput;
    out.clip_position = camera.view_projection * vec4<f32>(world_position, 1.0);
    out.uv = in.corner * 0.5 + vec2<f32>(0.5, 0.5);
    out.color = in.color;
    out.alpha = in.alpha;
    out.light_emission = in.light_emission;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let sampled = textureSample(image, image_sampler, in.uv);
    let alpha = sampled.a * in.alpha;

    // Premultiplied output with the pipeline's (One, OneMinusSrcAlpha) blend
    // state: at `light_emission` 0 this is exactly straight alpha blending
    // (`dst *= 1 - alpha`); at 1 the output alpha is zero, so `dst` passes
    // through untouched and the colour term alone adds on top of it — additive.
    // Anything between lerps continuously from one equation to the other.
    let color = sampled.rgb * in.color;
    return vec4<f32>(color * alpha, alpha * (1.0 - in.light_emission));
}
