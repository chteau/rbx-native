
// A `MeshPart` wearing a `SurfaceAppearance`: the same instanced file-mesh
// geometry as `filemesh.wgsl`, but skinned by the appearance's own four PBR maps
// in the mesh's own UVs instead of by the part's projected `Material`. Shaded
// through `mapped_shade` in `material.wgsl` (concatenated in front of this file,
// behind `lighting.wgsl`) so both map sources obey the same rules.

@group(2) @binding(0) var appearance_color: texture_2d<f32>;
@group(2) @binding(1) var appearance_normal: texture_2d<f32>;
@group(2) @binding(2) var appearance_metalness: texture_2d<f32>;
@group(2) @binding(3) var appearance_roughness: texture_2d<f32>;
@group(2) @binding(4) var appearance_sampler: sampler;
@group(2) @binding(5) var<uniform> appearance: Appearance;

struct Appearance {
    // xyz: `SurfaceAppearance.Color`, linear. w: 1 for AlphaMode Transparency,
    // 0 for Overlay.
    tint: vec4<f32>,
}

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) tangent: vec4<f32>,
}

struct InstanceInput {
    @location(4) model_0: vec4<f32>,
    @location(5) model_1: vec4<f32>,
    @location(6) model_2: vec4<f32>,
    @location(7) model_3: vec4<f32>,
    // Linear colour in xyz, 1 - Transparency in w.
    @location(8) color: vec4<f32>,
    @location(9) reflectance: f32,
    @location(10) material_layer: u32,
    @location(11) studs_per_tile: f32,
    @location(12) material_kind: u32,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) world_position: vec3<f32>,
    @location(4) reflectance: f32,
    @location(5) tangent: vec4<f32>,
    @location(6) @interpolate(flat) kind: u32,
}

@vertex
fn vs_main(vertex: VertexInput, instance: InstanceInput) -> VertexOutput {
    let model = mat4x4<f32>(
        instance.model_0,
        instance.model_1,
        instance.model_2,
        instance.model_3,
    );
    let world = model * vec4<f32>(vertex.position, 1.0);

    var out: VertexOutput;
    out.clip_position = uniforms.view_projection * world;
    // Same simplification as everywhere else: rotating by the model matrix and
    // renormalizing skips a proper inverse-transpose, which is exact only where
    // the instance scale is near uniform.
    out.normal = (model * vec4<f32>(vertex.normal, 0.0)).xyz;
    // The handedness is a property of the UV layout, not of the transform, so
    // only the axis is carried into world space.
    out.tangent = vec4<f32>((model * vec4<f32>(vertex.tangent.xyz, 0.0)).xyz, vertex.tangent.w);
    out.uv = vertex.uv;
    out.color = instance.color;
    out.world_position = world.xyz;
    out.reflectance = instance.reflectance;
    // A SurfaceAppearance replaces the part's texture pack outright, so only the
    // kinds that read no texture anyway survive it; everything else shades as a
    // plain textured material would.
    out.kind = select(KIND_TEXTURED, instance.material_kind, is_procedural(instance.material_kind));
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let map = textureSample(appearance_color, appearance_sampler, in.uv);
    let normal = textureSample(appearance_normal, appearance_sampler, in.uv).rgb;
    let metalness = textureSample(appearance_metalness, appearance_sampler, in.uv).r;
    let roughness = textureSample(appearance_roughness, appearance_sampler, in.uv).r;

    // The two alpha modes differ only in what the colour map's alpha does:
    // Overlay lets the part's own colour show through where it dips, while
    // Transparency hands it to the blend instead.
    let blends = appearance.tint.w > 0.5;
    let painted = select(mix(in.color.rgb, map.rgb, map.a), map.rgb, blends);
    let alpha = select(in.color.a, map.a * in.color.a, blends);

    var mapped: Mapped;
    mapped.base_albedo = in.color.rgb;
    mapped.albedo = painted * appearance.tint.rgb;
    mapped.geometric_normal = in.normal;
    mapped.normal = vertex_tangent_normal(normal, in.normal, in.tangent);
    mapped.world_position = in.world_position;
    mapped.reflectance = in.reflectance;
    mapped.metalness = metalness;
    mapped.roughness = roughness;
    mapped.kind = in.kind;

    return vec4<f32>(mapped_shade(mapped), alpha);
}
