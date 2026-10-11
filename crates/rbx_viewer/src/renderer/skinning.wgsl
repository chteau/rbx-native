// Linear-blend skinning of one rest mesh into one vertex buffer that draws
// it (see `skinning.rs`). The same blend as `scene::filemesh::skinned::deform`:
// positions are blended and divided by the total weight, normals go through
// each bone's inverse-transpose and keep their stored length, and a vertex
// with no weight at all stays where it is. Tangents turn with the bone.

struct Influence {
    bones: vec4<u32>,
    weights: vec4<f32>,
}

struct Params {
    // Vertices to write.
    count: u32,
    // Floats per vertex in `drawn`.
    stride: u32,
    // Where the normal and the tangent start in a vertex, in floats;
    // `NONE` for a buffer without one.
    normal: u32,
    tangent: u32,
    // Invocations per row of the dispatch grid.
    row: u32,
}

const NONE: u32 = 0xffffffffu;

// Per vertex: position, normal, tangent (w: handedness).
@group(0) @binding(0) var<storage, read> rest: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read> influences: array<Influence>;
// Per bone: the matrix, then its inverse-transpose.
@group(0) @binding(2) var<storage, read> palette: array<mat4x4<f32>>;
@group(0) @binding(3) var<uniform> params: Params;
@group(0) @binding(4) var<storage, read_write> drawn: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.y * params.row + id.x;
    if (i >= params.count) {
        return;
    }
    let p0 = rest[3u * i].xyz;
    let n0 = rest[3u * i + 1u].xyz;
    let t0 = rest[3u * i + 2u];
    let influence = influences[i];
    let total = dot(influence.weights, vec4<f32>(1.0));

    var p = p0;
    var n = n0;
    var t = t0.xyz;
    if (total > 0.0) {
        p = vec3<f32>(0.0);
        n = vec3<f32>(0.0);
        t = vec3<f32>(0.0);
        for (var k = 0u; k < 4u; k = k + 1u) {
            let w = influence.weights[k];
            if (w <= 0.0) {
                continue;
            }
            let bone = influence.bones[k];
            let m = palette[2u * bone];
            let inverse_transpose = palette[2u * bone + 1u];
            p = p + (m * vec4<f32>(p0, 1.0)).xyz * w;
            n = n + (inverse_transpose * vec4<f32>(n0, 0.0)).xyz * w;
            t = t + (m * vec4<f32>(t0.xyz, 0.0)).xyz * w;
        }
        p = p / total;
        let blended = length(n);
        if (blended > 1e-20) {
            n = n / blended * length(n0);
        } else {
            n = n0;
        }
        let turned = length(t);
        if (turned > 1e-20) {
            t = t / turned;
        } else {
            t = t0.xyz;
        }
    }

    let base = i * params.stride;
    drawn[base] = p.x;
    drawn[base + 1u] = p.y;
    drawn[base + 2u] = p.z;
    if (params.normal != NONE) {
        drawn[base + params.normal] = n.x;
        drawn[base + params.normal + 1u] = n.y;
        drawn[base + params.normal + 2u] = n.z;
    }
    if (params.tangent != NONE) {
        drawn[base + params.tangent] = t.x;
        drawn[base + params.tangent + 1u] = t.y;
        drawn[base + params.tangent + 2u] = t.z;
        drawn[base + params.tangent + 3u] = t0.w;
    }
}
