// An editor overlay's lines and dots, drawn onto the finished frame after the
// tone map rather than into the HDR scene (see `renderer::lines`): a line one
// pixel across keeps the colour it was given instead of being multisampled,
// bloomed and tone mapped into a grey smear.
//
// There is no depth attachment here — the scene's is multisampled wherever
// the frame is, and this target is not — so the depth-tested entry points
// read the scene's depth buffer themselves and drop what stands behind it.

struct Frame {
    view_proj: mat4x4<f32>,
    viewport: vec4<f32>,
};
@group(0) @binding(0)
var<uniform> frame: Frame;

// Retyped, with `scene_farthest` below, above one sample — see
// `renderer::lines`.
@group(1) @binding(0)
var scene_depth: texture_depth_2d;

// The farthest the scene reaches anywhere in this pixel: every sample of it
// once the frame is multisampled, since none of them sits at the pixel's
// centre, where this pass's own depth is.
fn scene_farthest(pixel: vec2<i32>) -> f32 {
    return textureLoad(scene_depth, pixel, 0);
}

// Reversed-Z, so nearer is larger. What rounding leaves between two depths
// interpolated across different triangles of the one plane.
const DEPTH_SLACK: f32 = 1e-5;

// How far past its own width a line's quad reaches, for the edge to fade
// across: one pixel, the width of the filter below.
const FRINGE: f32 = 1.0;

// How much the scene's depth changes from one pixel to the next here: the
// gentler of the two steps each way along each axis, so a silhouette on one
// side of the pixel does not count as a slope.
fn scene_slope(pixel: vec2<i32>, here: f32) -> f32 {
    let x = min(
        abs(scene_beside(pixel, vec2<i32>(1, 0)) - here),
        abs(here - scene_beside(pixel, vec2<i32>(-1, 0)))
    );
    let y = min(
        abs(scene_beside(pixel, vec2<i32>(0, 1)) - here),
        abs(here - scene_beside(pixel, vec2<i32>(0, -1)))
    );
    return max(x, y);
}

// `scene_farthest` one pixel over, held inside the frame.
fn scene_beside(pixel: vec2<i32>, offset: vec2<i32>) -> f32 {
    let last = vec2<i32>(textureDimensions(scene_depth)) - vec2<i32>(1);
    return scene_farthest(clamp(pixel + offset, vec2<i32>(0), last));
}

// Whether the scene stands in front of this fragment. A line or a dot has one
// depth all the way across it — its centre's — while the surface it lies on
// keeps sloping away under it; so the further a fragment is from that centre
// (`off_centre`, in pixels), the more of the surface's own slope it is
// allowed, plus half a pixel for where in the pixel the samples sit. Without
// it a guide drawn on a sloped face loses the pixel on its nearer side.
fn behind_scene(position: vec4<f32>, off_centre: f32) -> bool {
    let pixel = vec2<i32>(position.xy);
    let scene = scene_farthest(pixel);
    let slack = scene * DEPTH_SLACK + scene_slope(pixel, scene) * (off_centre + 0.5);
    return position.z < scene - slack;
}

// The target is attached through its non-sRGB view, so blending happens on
// the encoded values — how Roblox composites its overlays — and the colour
// is encoded here instead of by the hardware.
fn encoded(color: vec4<f32>, coverage: f32) -> vec4<f32> {
    let c = color.rgb;
    let low = c * 12.92;
    let high = 1.055 * pow(max(c, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - 0.055;
    return vec4<f32>(select(high, low, c <= vec3<f32>(0.0031308)), color.a * coverage);
}

struct LineInput {
    @location(0) position: vec3<f32>,
    @location(1) other: vec3<f32>,
    @location(2) side: f32,
    @location(3) half_width: f32,
    @location(4) color: vec4<f32>,
};

struct LineOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    // Signed distance from the line's centre, in pixels, and its half width.
    // Linear on screen rather than perspective-correct: the quad's width is
    // pixels, the same at both ends however far apart their depths are.
    @location(1) @interpolate(linear) across: f32,
    @location(2) half_width: f32,
};

// Whether `a` comes before `b` in a fixed order of points, so the two ends
// of one segment each know which of them is first.
fn precedes(a: vec3<f32>, b: vec3<f32>) -> bool {
    if a.x != b.x {
        return a.x < b.x;
    }
    if a.y != b.y {
        return a.y < b.y;
    }
    return a.z < b.z;
}

@vertex
fn vs_line(input: LineInput) -> LineOutput {
    let clip_this = frame.view_proj * vec4<f32>(input.position, 1.0);
    let clip_other = frame.view_proj * vec4<f32>(input.other, 1.0);
    let half_vp = 0.5 * frame.viewport.xy;

    let px_this = clip_this.xy / clip_this.w * half_vp;
    let px_other = clip_other.xy / clip_other.w * half_vp;
    var dir = px_other - px_this;
    let len = length(dir);
    dir = select(vec2<f32>(1.0, 0.0), dir / len, len > 1e-5);
    let perp = vec2<f32>(-dir.y, dir.x);
    let reach = input.half_width + FRINGE;
    let px = px_this + perp * input.side * reach;

    // `side` is measured against this end's own direction to the other,
    // which is reversed at the far end; which side of the line a corner is
    // on, for the filter, has to be measured against one direction both
    // ends agree on.
    let forward = select(-1.0, 1.0, precedes(input.position, input.other));

    var out: LineOutput;
    // Back to clip space keeping this end's own depth and w, so the depth
    // compared against the scene's is where the line really is.
    out.clip_position = vec4<f32>(px / half_vp * clip_this.w, clip_this.z, clip_this.w);
    out.color = input.color;
    out.across = input.side * reach * forward;
    out.half_width = input.half_width;
    return out;
}

// A one-pixel box filter across the line: a pixel its centre runs through is
// fully covered, one it passes between is shared with its neighbour.
fn line_coverage(input: LineOutput) -> f32 {
    return clamp(input.half_width + 0.5 - abs(input.across), 0.0, 1.0);
}

@fragment
fn fs_line(input: LineOutput) -> @location(0) vec4<f32> {
    let coverage = line_coverage(input);
    if coverage <= 0.0 || behind_scene(input.clip_position, abs(input.across)) {
        discard;
    }
    return encoded(input.color, coverage);
}

@fragment
fn fs_line_on_top(input: LineOutput) -> @location(0) vec4<f32> {
    let coverage = line_coverage(input);
    if coverage <= 0.0 {
        discard;
    }
    return encoded(input.color, coverage);
}

// A dot: a disc a fixed number of pixels across round a world-space point,
// whatever its distance — the size Studio's draggers hold their
// `SphereHandleAdornment` markers at. It arrives as a square pushed out to
// size here and is trimmed to a circle with a pixel of soft edge.
struct DotInput {
    @location(0) centre: vec3<f32>,
    @location(1) corner: vec2<f32>,
    @location(2) radius: f32,
    @location(3) color: vec4<f32>,
};

struct DotOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
    // Where this fragment is from the centre, in pixels, and the radius.
    @location(1) offset: vec2<f32>,
    @location(2) radius: f32,
};

@vertex
fn vs_dot(input: DotInput) -> DotOutput {
    let clip = frame.view_proj * vec4<f32>(input.centre, 1.0);
    let half_vp = 0.5 * frame.viewport.xy;
    let offset = input.corner * (input.radius + FRINGE);
    let px = clip.xy / clip.w * half_vp + offset;

    var out: DotOutput;
    out.clip_position = vec4<f32>(px / half_vp * clip.w, clip.z, clip.w);
    out.color = input.color;
    out.offset = offset;
    out.radius = input.radius;
    return out;
}

fn dot_coverage(input: DotOutput) -> f32 {
    return clamp(input.radius + 0.5 - length(input.offset), 0.0, 1.0);
}

@fragment
fn fs_dot(input: DotOutput) -> @location(0) vec4<f32> {
    let coverage = dot_coverage(input);
    if coverage <= 0.0 || behind_scene(input.clip_position, length(input.offset)) {
        discard;
    }
    return encoded(input.color, coverage);
}

@fragment
fn fs_dot_on_top(input: DotOutput) -> @location(0) vec4<f32> {
    let coverage = dot_coverage(input);
    if coverage <= 0.0 {
        discard;
    }
    return encoded(input.color, coverage);
}
