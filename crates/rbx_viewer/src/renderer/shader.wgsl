
// Instanced parts: boxes and the procedural shapes, textured by `material.wgsl`
// and shaded by `lighting.wgsl`, both concatenated in front of this file (so
// bind groups 0 and 1, `Surface` and `shade` all come from there).

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    // White but on a union painted in its pieces' colours.
    @location(15) color: vec4<f32>,
}

struct InstanceInput {
    @location(2) model_0: vec4<f32>,
    @location(3) model_1: vec4<f32>,
    @location(4) model_2: vec4<f32>,
    @location(5) model_3: vec4<f32>,
    // Linear colour in xyz, 1 - Transparency in w.
    @location(6) color: vec4<f32>,
    @location(7) reflectance: f32,
    @location(8) material_layer: u32,
    @location(9) studs_per_tile: f32,
    @location(10) material_kind: u32,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) color: vec4<f32>,
    @location(2) world_position: vec3<f32>,
    @location(3) reflectance: f32,
    @location(4) object_studs: vec3<f32>,
    @location(5) object_normal: vec3<f32>,
    @location(6) axis_x: vec3<f32>,
    @location(7) axis_y: vec3<f32>,
    @location(8) axis_z: vec3<f32>,
    @location(9) @interpolate(flat) material: vec3<u32>,
    @location(10) studs_per_tile: f32,
    @location(11) @interpolate(flat) extent: vec3<f32>,
}

// A vertex colour is stored sRGB, like a part's; the instance colour
// arrives already linear.
fn vertex_tint(srgb: vec3<f32>) -> vec3<f32> {
    let low = srgb / 12.92;
    let high = pow((srgb + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, srgb <= vec3<f32>(0.04045));
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
    // Parts only ever carry an axis-aligned scale, so rotating the normal by the model
    // matrix and renormalizing gives the same direction as the inverse transpose would.
    out.normal = (model * vec4<f32>(vertex.normal, 0.0)).xyz;
    out.color = vec4<f32>(instance.color.rgb * vertex_tint(vertex.color.rgb), instance.color.a);
    out.world_position = world.xyz;
    out.reflectance = instance.reflectance;
    // The material is projected in object space, in studs: the column lengths
    // are the part's own size, since the only scale a part carries is its own.
    out.extent = model_extent(model);
    out.object_studs = vertex.position * out.extent;
    out.object_normal = vertex.normal;
    out.axis_x = normalize(model[0].xyz);
    out.axis_y = normalize(model[1].xyz);
    out.axis_z = normalize(model[2].xyz);
    out.material = vec3<u32>(
        instance.material_layer,
        instance.material_kind & ((1u << SURFACES_SHIFT) - 1u),
        instance.material_kind >> SURFACES_SHIFT,
    );
    out.studs_per_tile = instance.studs_per_tile;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    var input: MaterialInput;
    input.object_studs = in.object_studs;
    input.object_normal = in.object_normal;
    input.rotation = mat3x3<f32>(in.axis_x, in.axis_y, in.axis_z);
    input.world_normal = in.normal;
    input.world_position = in.world_position;
    input.albedo = in.color.rgb;
    input.reflectance = in.reflectance;
    input.layer = in.material.x;
    input.studs_per_tile = in.studs_per_tile;
    input.kind = in.material.y & KIND_MASK;
    input.variant_alpha = (in.material.y >> VARIANT_ALPHA_SHIFT) & VARIANT_ALPHA_MASK;
    input.extent = in.extent;
    input.anchored = (in.material.y & FACE_ANCHORED) != 0u;
    input.curved = (in.material.y & FACE_CURVED) != 0u;
    surface_relief(&input, in.material.z, in.extent);

    // Straight (non-premultiplied) alpha: the translucent pipeline pairs this
    // with src_alpha / one_minus_src_alpha, and the opaque one always gets 1.
    return material_output(input, in.color.a, in.clip_position);
}
