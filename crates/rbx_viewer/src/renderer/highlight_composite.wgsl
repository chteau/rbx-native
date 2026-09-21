// Turning the `Highlight` mask into the effect itself: a solid interior over
// every pixel a highlight owns, and an outline straddling the boundary
// between one highlight and whatever is not it.
//
// The outline is found by comparing each pixel's mask value against its
// neighbours rather than by tracing the geometry, which is what lets a ball,
// a wedge and a downloaded mesh all outline as their own silhouette instead
// of as the box around them.

// One highlight's two colours, each with its own alpha (`1 - Transparency`).
// Indexed by the mask value minus one — see `highlight.wgsl`.
struct Paint {
    fill: vec4<f32>,
    outline: vec4<f32>,
};

// This declaration is rewritten for a multisampled mask — see
// `renderer::highlight::pipelines::MASK_BINDING`. WGSL types the two as
// different texture types entirely, but `textureLoad` takes the same three
// arguments for both (the third being the mip level in one case and the
// sample index in the other), so only this one line has to change.
@group(0) @binding(0)
var mask: texture_2d<u32>;
@group(0) @binding(1)
var<uniform> paints: array<Paint, 255>;

// Sample 0 only, where the mask is multisampled: the highlight's own edge is
// then aliased against the geometry it traces, which is a cheaper trade than
// a mask that cannot be a single texture read.
fn load_mask(coord: vec2<i32>) -> u32 {
    return textureLoad(mask, coord, 0).r;
}

// Half the outline's width in pixels, so the drawn line is `2 * RADIUS + 1`
// across. Roblox's own `Highlight.LineThickness` is tagged `Hidden` in the API
// dump and is documented nowhere, so this is chosen to sit beside this
// renderer's existing cues (a ~3px selection box — see `selection.wgsl`)
// rather than to match a published number.
//
// ponytail: an O(RADIUS²) neighbourhood scan, 9 taps at RADIUS 1. A separable
// pass or a jump-flood would be the upgrade if the radius ever grows.
const RADIUS: i32 = 1;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
};

// One oversized triangle rather than two — it covers the viewport with three
// vertices and no vertex buffer at all.
@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    let x = f32(i32(index) / 2) * 4.0 - 1.0;
    let y = f32(i32(index) & 1) * 4.0 - 1.0;

    var out: VertexOutput;
    out.clip_position = vec4<f32>(x, y, 0.0, 1.0);
    return out;
}

fn sampled(coord: vec2<i32>, bounds: vec2<i32>) -> u32 {
    // Clamped rather than wrapped: a highlight running off the side of the
    // frame should keep its interior there, not grow an outline along the
    // window's edge.
    let clamped = clamp(coord, vec2<i32>(0, 0), bounds - vec2<i32>(1, 1));
    return load_mask(clamped);
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let bounds = vec2<i32>(textureDimensions(mask));
    let coord = vec2<i32>(input.clip_position.xy);
    let here = load_mask(coord);

    // The boundary this pixel sits on, if any: the highest-numbered highlight
    // among it and its neighbours, wherever the two disagree. Highest rather
    // than nearest because two highlights meeting at a pixel is already a tie
    // Roblox does not define an answer for, and a stable choice at least
    // draws one unbroken line instead of a dotted argument between them.
    var edge: u32 = 0u;
    for (var dy = -RADIUS; dy <= RADIUS; dy++) {
        for (var dx = -RADIUS; dx <= RADIUS; dx++) {
            let there = sampled(coord + vec2<i32>(dx, dy), bounds);
            if (there != here) {
                edge = max(edge, max(there, here));
            }
        }
    }

    if (edge != 0u) {
        let paint = paints[edge - 1u].outline;
        return vec4<f32>(paint.rgb, paint.a);
    }
    if (here != 0u) {
        let paint = paints[here - 1u].fill;
        return vec4<f32>(paint.rgb, paint.a);
    }
    return vec4<f32>(0.0, 0.0, 0.0, 0.0);
}
