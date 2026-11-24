// The `Highlight` mask: which highlight, if any, owns each pixel of the frame.
//
// Position only, instanced against the same unit shapes and file meshes the
// colour pass draws, the same trick `shadow.wgsl` uses, and for the same
// reason: a silhouette needs the geometry, never its material. The fragment
// writes the highlight's own 1-based index into an R8Uint target, which
// `highlight_composite.wgsl` then reads to find both the interior and the
// edge. Zero means "no highlight here", which is what the pass clears to,
// and is why the index is 1-based rather than 0-based.

struct VertexInput {
    @location(0) position: vec3<f32>,
};

// The four columns of the instance's model matrix, then which highlight it
// belongs to, see `renderer::highlight::MaskInstance`.
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

// `DepthMode::Faded`'s second draw: the same index with `VISIBLE` set, over
// the pixels where the geometry is the nearest thing there. The pipeline's
// depth test has already dropped the ones a part hides; this drops the ones
// an opaque depth-tested adornment hides, whose depth is a buffer of its own
// (see `renderer::adornment`). Only this entry point reads group 1.
const VISIBLE: u32 = 0x80u;

@group(1) @binding(0)
var adornment_depth: texture_depth_2d;

@fragment
fn fs_visible(input: VertexOutput) -> @location(0) u32 {
    // Clamped: with no adornment drawn the binding is a 1x1 stand-in at the
    // far plane (see `post::Targets::adornment_depth_binding`).
    let last = vec2<i32>(textureDimensions(adornment_depth)) - vec2<i32>(1, 1);
    let texel = min(vec2<i32>(input.clip_position.xy), last);
    // Reversed-Z: the larger depth is the nearer surface.
    if textureLoad(adornment_depth, texel, 0) > input.clip_position.z {
        discard;
    }
    return input.index | VISIBLE;
}
