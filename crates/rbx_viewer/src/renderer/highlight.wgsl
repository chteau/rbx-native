// The `Highlight` mask: which highlight, if any, owns each pixel of the frame.
//
// Position only, instanced against the same unit shapes and file meshes the
// colour pass draws — the same trick `shadow.wgsl` uses, and for the same
// reason: a silhouette needs the geometry, never its material. The fragment
// writes the highlight's own 1-based index into an R8Uint target, which
// `highlight_composite.wgsl` then reads to find both the interior and the
// edge. Zero means "no highlight here", which is what the pass clears to,
// and is why the index is 1-based rather than 0-based.

struct VertexInput {
    @location(0) position: vec3<f32>,
};

// The four columns of the instance's model matrix, then which highlight it
// belongs to — see `renderer::highlight::MaskInstance`.
struct InstanceInput {
    @location(1) model_0: vec4<f32>,
    @location(2) model_1: vec4<f32>,
    @location(3) model_2: vec4<f32>,
    @location(4) model_3: vec4<f32>,
    @location(5) index: u32,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    // Flat: an index is a name, and interpolating one across a triangle would
    // blend two highlights into a third that does not exist.
    @location(0) @interpolate(flat) index: u32,
};

// Bind group 0 is the renderer's shared frame uniform (see
// `renderer::pipeline::frame_layout`), of which only the view-projection
// matrix is read here.
struct Frame {
    view_proj: mat4x4<f32>,
    viewport: vec4<f32>,
};
@group(0) @binding(0)
var<uniform> frame: Frame;

@vertex
fn vs_main(vertex: VertexInput, instance: InstanceInput) -> VertexOutput {
    let model = mat4x4<f32>(
        instance.model_0,
        instance.model_1,
        instance.model_2,
        instance.model_3,
    );

    var out: VertexOutput;
    out.clip_position = frame.view_proj * model * vec4<f32>(vertex.position, 1.0);
    out.index = instance.index;
    return out;
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) u32 {
    return input.index;
}
