
// Textured MeshPart/SpecialMesh (FileMesh) geometry: one real triangle mesh per
// downloaded asset, drawn instanced, textured by `material.wgsl` and shaded by
// `lighting.wgsl` (both concatenated in front of this file) exactly like the
// boxes, with its own image sampled through its own UVs on top.

@group(2) @binding(0) var image: texture_2d<f32>;
@group(2) @binding(1) var image_sampler: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
}

struct InstanceInput {
    @location(3) model_0: vec4<f32>,
    @location(4) model_1: vec4<f32>,
    @location(5) model_2: vec4<f32>,
    @location(6) model_3: vec4<f32>,
    // Linear colour in xyz, 1 - Transparency in w.
    @location(7) color: vec4<f32>,
    @location(8) reflectance: f32,
    @location(9) material_layer: u32,
    @location(10) studs_per_tile: f32,
    @location(11) material_kind: u32,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) world_position: vec3<f32>,
    @location(4) reflectance: f32,
    @location(5) object_studs: vec3<f32>,
    @location(6) object_normal: vec3<f32>,
    @location(7) axis_x: vec3<f32>,
    @location(8) axis_y: vec3<f32>,
    @location(9) axis_z: vec3<f32>,
    @location(10) @interpolate(flat) material: vec2<u32>,
    @location(11) studs_per_tile: f32,
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
    // Same simplification as the box pass: rotating by the model matrix and
    // renormalizing skips a proper inverse-transpose. Works well when scale is
    // uniform per axis; a reasonable approximation for non-uniform scales.
    out.normal = (model * vec4<f32>(vertex.normal, 0.0)).xyz;
    out.uv = vertex.uv;
    out.color = instance.color;
    out.world_position = world.xyz;
    out.reflectance = instance.reflectance;
    // Mesh vertices are already in studs as authored, so the instance scale is
    // all that stands between them and the world scale the material tiles at.
    out.object_studs = vertex.position * model_extent(model);
    out.object_normal = vertex.normal;
    out.axis_x = normalize(model[0].xyz);
    out.axis_y = normalize(model[1].xyz);
    out.axis_z = normalize(model[2].xyz);
    out.material = vec2<u32>(instance.material_layer, instance.material_kind);
    out.studs_per_tile = instance.studs_per_tile;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let sample = textureSample(image, image_sampler, in.uv);

    var input: MaterialInput;
    input.object_studs = in.object_studs;
    input.object_normal = in.object_normal;
    input.rotation = mat3x3<f32>(in.axis_x, in.axis_y, in.axis_z);
    input.world_normal = in.normal;
    input.world_position = in.world_position;
    // The mesh's own image comes first: a material can only tint what it paints.
    input.albedo = sample.rgb * in.color.rgb;
    input.reflectance = in.reflectance;
    input.layer = in.material.x;
    input.studs_per_tile = in.studs_per_tile;
    input.kind = in.material.y;

    // A `ForceField` does not paint its image on: its red channel picks which
    // texels show at this moment and its alpha how strongly (see
    // `force_field_window`), in the part's own colour — the image's own
    // colour and alpha never reach the frame.
    var alpha = sample.a * in.color.a;
    if input.kind == KIND_FORCE_FIELD {
        input.albedo = in.color.rgb;
        input.pattern = force_field_window(sample.r, uniforms.viewport.z) * sample.a;
        alpha = in.color.a;
    }

    return material_output(input, alpha, in.clip_position.xy / uniforms.viewport.xy);
}
