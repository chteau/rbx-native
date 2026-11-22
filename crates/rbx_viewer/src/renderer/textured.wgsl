
// Decal and Texture instances, drawn on the part's own surface: the image is a
// planar projection in the part's object space, clipped to the face that surface
// belongs to, and shaded by `lighting.wgsl` (concatenated in front of this file)
// exactly like the surface underneath, so a painted face and a bare one read as
// the same material.

@group(1) @binding(0) var image: texture_2d<f32>;
@group(1) @binding(1) var image_sampler: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
}

struct InstanceInput {
    @location(2) model_0: vec4<f32>,
    @location(3) model_1: vec4<f32>,
    @location(4) model_2: vec4<f32>,
    @location(5) model_3: vec4<f32>,
    @location(6) face_normal: vec3<f32>,
    @location(7) u_axis: vec3<f32>,
    @location(8) v_axis: vec3<f32>,
    // Scale in xy, offset in zw.
    @location(9) uv_transform: vec4<f32>,
    @location(10) tint: vec4<f32>,
    @location(11) wedge: u32,
    // From the drawn mesh into the part's unit box: the identity for a unit
    // shape, the mesh's own fit for a real mesh a decal lands on.
    @location(12) to_unit_0: vec4<f32>,
    @location(13) to_unit_1: vec4<f32>,
    @location(14) to_unit_2: vec4<f32>,
    @location(15) to_unit_3: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) object_position: vec3<f32>,
    @location(1) object_normal: vec3<f32>,
    @location(2) world_normal: vec3<f32>,
    @location(3) face_normal: vec3<f32>,
    @location(4) u_axis: vec3<f32>,
    @location(5) v_axis: vec3<f32>,
    @location(6) uv_transform: vec4<f32>,
    @location(7) tint: vec4<f32>,
    @location(8) world_position: vec3<f32>,
    @location(9) @interpolate(flat) wedge: u32,
}

// What carries a normal through `m`: its cofactor matrix, the inverse
// transpose up to a scale, which stays right under a non-uniform scale (a
// mesh fitted to its part) where `m` itself would bend the normal.
fn normal_matrix(m: mat3x3<f32>) -> mat3x3<f32> {
    let cofactor = mat3x3<f32>(
        cross(m[1], m[2]),
        cross(m[2], m[0]),
        cross(m[0], m[1]),
    );
    return cofactor * sign(determinant(m));
}

@vertex
fn vs_main(vertex: VertexInput, instance: InstanceInput) -> VertexOutput {
    let model = mat4x4<f32>(
        instance.model_0,
        instance.model_1,
        instance.model_2,
        instance.model_3,
    );

    let to_unit = mat4x4<f32>(
        instance.to_unit_0,
        instance.to_unit_1,
        instance.to_unit_2,
        instance.to_unit_3,
    );
    let unit = to_unit * vec4<f32>(vertex.position, 1.0);
    let unit_normal = normalize(
        normal_matrix(mat3x3<f32>(to_unit[0].xyz, to_unit[1].xyz, to_unit[2].xyz)) * vertex.normal,
    );
    let world = model * unit;

    var out: VertexOutput;
    out.clip_position = uniforms.view_projection * world;
    out.world_position = world.xyz;
    // The projection lives in object space, where the face axes are constant
    // whichever way the part is turned.
    out.object_position = unit.xyz;
    out.object_normal = unit_normal;
    // Parts only ever carry an axis-aligned scale, so rotating the normal by the
    // model matrix and renormalizing gives the same direction as the inverse
    // transpose would.
    out.world_normal = (model * vec4<f32>(unit_normal, 0.0)).xyz;
    out.face_normal = instance.face_normal;
    out.u_axis = instance.u_axis;
    out.v_axis = instance.v_axis;
    out.uv_transform = instance.uv_transform;
    out.tint = instance.tint;
    out.wedge = instance.wedge;
    return out;
}

// The face a wedge's surface belongs to, which is not quite the axis its normal
// leans on: the slope is the Front face, and a wedge has no Top surface at all
// for a `Face = Top` decal to land on. Its 45-degree normal otherwise ties
// between the two, so which one a fragment picked would come down to rounding.
//
// Only the shapes flagged here are remapped, leaving balls, cylinders and boxes
// on the plain `dominant_axis` classification.
fn face_axis(normal: vec3<f32>, wedge: u32) -> vec3<f32> {
    let axis = dominant_axis(normal);
    if wedge != 0u && axis.y > 0.0 {
        return vec3<f32>(0.0, 0.0, -1.0);
    }
    return axis;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Both are unit axes, so the dot product is 1 on the instance's own face and
    // 0 or -1 on any other one. `dominant_axis`, which `face_axis` builds on,
    // comes from `lighting.wgsl`, where the materials read it from too.
    if dot(face_axis(in.object_normal, in.wedge), in.face_normal) < 0.5 {
        discard;
    }

    let projected = vec2<f32>(
        dot(in.object_position, in.u_axis),
        dot(in.object_position, in.v_axis),
    );
    let uv = (projected + 0.5) * in.uv_transform.xy + in.uv_transform.zw;

    let sample = textureSample(image, image_sampler, uv);
    // Reflectance 0: a Decal is paint on top of whatever the part is made of,
    // and Roblox gives the image no reflection of its own either.
    let surface = plastic(
        sample.rgb * in.tint.rgb,
        in.world_normal,
        in.world_position,
        0.0,
    );

    // Straight (non-premultiplied) alpha: the blended pipeline pairs this with
    // src_alpha / one_minus_src_alpha.
    return vec4<f32>(shade(surface), sample.a * in.tint.a);
}
