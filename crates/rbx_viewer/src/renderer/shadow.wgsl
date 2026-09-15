// The sun's shadow map: every caster in the scene, drawn depth-only from the
// lamp's own orthographic view (see `renderer::shadow::fit`).
//
// No fragment stage at all — the pass writes nothing but depth — and no normals,
// UVs or materials: a caster only has to say where it is. Both pipelines share
// this shader and differ solely in the stride of the vertex buffer they read a
// position out of (24 bytes for the shared unit shapes, 12 for a file mesh's
// position-only copy).

struct Light {
    view_projection: mat4x4<f32>,
}

@group(0) @binding(0) var<uniform> light: Light;

struct VertexInput {
    @location(0) position: vec3<f32>,
}

struct InstanceInput {
    @location(1) model_0: vec4<f32>,
    @location(2) model_1: vec4<f32>,
    @location(3) model_2: vec4<f32>,
    @location(4) model_3: vec4<f32>,
}

@vertex
fn vs_main(vertex: VertexInput, instance: InstanceInput) -> @builtin(position) vec4<f32> {
    let model = mat4x4<f32>(
        instance.model_0,
        instance.model_1,
        instance.model_2,
        instance.model_3,
    );
    return light.view_projection * (model * vec4<f32>(vertex.position, 1.0));
}
