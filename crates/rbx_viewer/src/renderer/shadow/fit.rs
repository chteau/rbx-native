//! Where the sun's orthographic shadow map looks, this frame.
//!
//! Two properties matter more than tightness here: the map must not shimmer
//! when the camera moves (its texels are snapped to a world grid), and it must
//! not change size when the camera *turns* (its extent comes from a sphere
//! around the truncated frustum, which is rotation-invariant). Both are what
//! makes a 0.3-stud step produce an identical image rather than a crawling
//! shadow edge.

use glam::camera::rh::proj::directx::orthographic;
use glam::camera::rh::view::look_to_mat4;
use glam::{Mat4, Vec3};

use crate::scene::Bounds;

/// Slack on the light-space depth range, so a caster sitting exactly on the
/// scene's bounding box is not clipped by the near plane it defines.
const DEPTH_MARGIN_STUDS: f32 = 8.0;
/// A scene with no extent at all (one flat part) still needs an invertible
/// projection.
const MIN_HALF_EXTENT_STUDS: f32 = 1.0;
/// Below this the light direction is vertical and `Vec3::Y` is no longer a
/// usable up vector for the light's own view matrix.
const VERTICAL_LIGHT: f32 = 0.999;

/// One frame's light-space fit.
pub(in crate::renderer) struct Fit {
    pub(in crate::renderer) view_projection: Mat4,
    /// World size of one shadow-map texel, which is what both the receiver's
    /// normal offset and the PCF kernel are measured in.
    pub(in crate::renderer) texel_studs: f32,
    /// Span of the orthographic depth range, so a bias expressed in studs can
    /// be converted to the [0, 1] the depth buffer stores.
    pub(in crate::renderer) depth_studs: f32,
    /// One texel of the map in UV, i.e. the reciprocal of its side. Carried
    /// rather than read off a constant: the map's size is a quality knob, and
    /// the PCF kernel only ever needs the step.
    pub(in crate::renderer) texel_uv: f32,
    /// The light's own rotation (no translation — the light has no position,
    /// only a direction) and the fitted box's light-space footprint, i.e.
    /// exactly what [`Fit::visible`] needs and nothing `view_projection`
    /// alone would hand back without inverting it every caster.
    pub(in crate::renderer) light_view: Mat4,
    pub(in crate::renderer) x: f32,
    pub(in crate::renderer) y: f32,
    pub(in crate::renderer) half: f32,
}

impl Fit {
    /// What the uniform carries before anything is fitted — with
    /// `GlobalShadows` off the map is never drawn and never sampled.
    pub(in crate::renderer) fn unfitted() -> Self {
        Fit {
            view_projection: Mat4::IDENTITY,
            texel_studs: 0.0,
            depth_studs: 1.0,
            texel_uv: 0.0,
            light_view: Mat4::IDENTITY,
            x: 0.0,
            y: 0.0,
            // Never actually drawn against — `Renderer::sun_shadow` only
            // returns this when there is no shadow pass to cull casters for
            // this frame — but infinite keeps `visible` honestly total rather
            // than quietly culling everything if it somehow were.
            half: f32::INFINITY,
        }
    }

    /// Whether a shadow caster's bounding sphere could still land inside this
    /// frame's map: its light-space (x, y) footprint overlaps the fitted box,
    /// expanded by its own radius.
    ///
    /// Depth is never checked: `scene`'s own extent (see [`fit`]'s `near`/
    /// `far`) already covers every part in the scene by construction, so
    /// nothing real can ever fail on that axis alone. Comparing per-axis
    /// against an expanded box rather than the sphere's true distance to the
    /// box is a coarser test (a sphere near a corner can read as overlapping
    /// when it is just outside) — conservative in the cull's favor, never the
    /// other way round, which is what a shadow-pass test has to be.
    pub(in crate::renderer) fn visible(&self, center: Vec3, radius: f32) -> bool {
        // A touching sphere must never be culled by rounding alone (in the
        // fit itself, or in a caller's own round trip through `light_view`).
        const TOUCHING_EPSILON: f32 = 1e-3;
        let bound = self.half + radius + TOUCHING_EPSILON;
        let local = self.light_view.transform_point3(center);
        (local.x - self.x).abs() <= bound && (local.y - self.y).abs() <= bound
    }
}

/// Fits the map to the camera frustum's own bounding sphere, recentred onto
/// whatever part of the scene that sphere actually overlaps.
///
/// `light` points *at* the lamp casting the shadows (the sun by day, the moon
/// at night); `frustum` is the camera's eight corners already truncated at the
/// quality level's own shadow distance.
pub(in crate::renderer) fn fit(
    light: Vec3,
    frustum: &[Vec3; 8],
    bounds: &Bounds,
    resolution: u32,
) -> Fit {
    let resolution = resolution.max(1) as f32;
    let light = light.normalize_or(Vec3::Y);
    let up = if light.y.abs() > VERTICAL_LIGHT {
        Vec3::Z
    } else {
        Vec3::Y
    };
    // The light has no position, only a direction, so its view matrix is a pure
    // rotation about the world origin — which is also what keeps the texel grid
    // snapping below anchored to the world rather than to the camera.
    let view = look_to_mat4(Vec3::ZERO, -light, up);
    let scene = light_space_extent(&view, bounds);

    // The frustum's own minimal enclosing sphere, not a light-space box: turning
    // or pitching the camera rotates the frustum rigidly about its own centre,
    // so a sphere built purely from its corners is the one measure of it that
    // does not change with the view direction.
    let (center, radius) = bounding_sphere(frustum);
    let half = radius
        .min(0.5 * (scene.max - scene.min).truncate().max_element())
        .max(MIN_HALF_EXTENT_STUDS);
    let texel = 2.0 * half / resolution;

    let center = view.transform_point3(center);
    // Snapping to whole texels *after* the clamp: the grid is what stops the
    // shadow edges crawling, and being a texel outside the clamp costs nothing.
    let x = snap(clamp_span(center.x, scene.min.x, scene.max.x, half), texel);
    let y = snap(clamp_span(center.y, scene.min.y, scene.max.y, half), texel);

    // Depth comes from the whole scene, never from the frustum: a caster
    // between the sun and the visible region is off-screen and still has to be
    // in the map. Taking it from the bounds also makes it constant frame to
    // frame, which is one less thing that can shimmer.
    let near = -scene.max.z - DEPTH_MARGIN_STUDS;
    let far = -scene.min.z + DEPTH_MARGIN_STUDS;

    Fit {
        view_projection: orthographic(x - half, x + half, y - half, y + half, near, far) * view,
        texel_studs: texel,
        depth_studs: far - near,
        texel_uv: 1.0 / resolution,
        light_view: view,
        x,
        y,
        half,
    }
}

/// The frustum corners' minimal enclosing sphere, by Ritter's algorithm: seed
/// it from the pair of corners farthest apart, then grow it to cover whatever
/// still sits outside.
///
/// A sphere built from the corners' own centroid instead (the obvious thing to
/// reach for) pays for the pyramid's full diagonal spread even though the far
/// plane's own four corners already enclose every near-plane one — on a wide
/// field of view that inflates the map's coverage well past the quality
/// level's own `shadow_distance`, spending texels on empty space beyond the
/// camera's frustum and coarsening every texel actually in view. This is
/// tighter while staying exactly as rotation-invariant, since it is still a
/// pure function of the corner set.
fn bounding_sphere(corners: &[Vec3; 8]) -> (Vec3, f32) {
    let farthest_from = |from: Vec3| {
        corners
            .iter()
            .copied()
            .max_by(|a, b| {
                a.distance_squared(from)
                    .total_cmp(&b.distance_squared(from))
            })
            .expect("corners is non-empty")
    };

    let a = farthest_from(corners[0]);
    let b = farthest_from(a);
    let mut center = a.midpoint(b);
    let mut radius = a.distance(b) / 2.0;

    for corner in corners {
        let distance = corner.distance(center);
        if distance > radius {
            let extra = (distance - radius) / 2.0;
            center += (*corner - center) * (extra / distance);
            radius += extra;
        }
    }

    (center, radius)
}

/// The scene's bounding box seen from the light, as a light-space min/max pair.
fn light_space_extent(view: &Mat4, bounds: &Bounds) -> Extent {
    bounds
        .corners()
        .into_iter()
        .map(|corner| view.transform_point3(corner))
        .fold(None, |extent: Option<Extent>, corner| {
            Some(match extent {
                None => Extent {
                    min: corner,
                    max: corner,
                },
                Some(extent) => Extent {
                    min: extent.min.min(corner),
                    max: extent.max.max(corner),
                },
            })
        })
        .unwrap_or(Extent {
            min: Vec3::ZERO,
            max: Vec3::ZERO,
        })
}

struct Extent {
    min: Vec3,
    max: Vec3,
}

/// Keeps a half-width `half` window inside `[min, max]`, or centres it there
/// when the window is wider than the span itself.
fn clamp_span(value: f32, min: f32, max: f32, half: f32) -> f32 {
    if max - min <= 2.0 * half {
        return 0.5 * (min + max);
    }
    value.clamp(min + half, max - half)
}

fn snap(value: f32, texel: f32) -> f32 {
    if texel <= 0.0 {
        return value;
    }
    (value / texel).round() * texel
}

#[cfg(test)]
#[path = "fit/tests.rs"]
mod tests;
