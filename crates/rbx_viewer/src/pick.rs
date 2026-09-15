//! Turning a point on screen into a world-space ray, and testing that ray
//! against the boxes parts are drawn as.
//!
//! Public because the picking happens in the *embedder*: `rbxstudio`'s
//! viewport owns the cursor and the DOM, while the renderer runs on a thread
//! of its own behind a one-way command channel. Keeping the unprojection here
//! rather than mirroring it there is what stops a click from resolving against
//! a slightly different camera than the frame under it was drawn with — see
//! [`Pose::view_projection`], which is the matrix both sides share.

use glam::{Mat4, Vec2, Vec3};
use rbx_dom::{CFrameData, Ref, Variant, Vector3Data, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::scene::{cframe_matrix, is_drawable, workspace_descendants};

// Reversed-Z (see `camera::Camera::projection`) puts the near plane at depth 1
// and the far end at 0, in both the perspective and the orthographic
// projection. Unprojecting those two depths gives two points on the same
// eye ray; the second is deliberately not 0, which perspective's infinite far
// plane maps to a point at infinity (`w` of zero).
const NEAR_DEPTH: f32 = 1.0;
const AHEAD_DEPTH: f32 = 0.02;

/// A half-line through the world: where it starts and, as a unit vector, which
/// way it goes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ray {
    pub origin: Vec3,
    pub direction: Vec3,
}

impl Ray {
    pub fn new(origin: Vec3, direction: Vec3) -> Self {
        Ray {
            origin,
            direction: direction.normalize_or(-Vec3::Z),
        }
    }

    /// The point `distance` studs along the ray.
    pub fn at(&self, distance: f32) -> Vec3 {
        self.origin + self.direction * distance
    }

    /// How far along the ray the point nearest `point` sits, and how far off
    /// the ray that point is. Behind the origin counts: the caller decides
    /// whether a negative distance disqualifies a hit, since a gizmo handle
    /// straddling the eye plane is still worth reporting.
    pub fn nearest(&self, point: Vec3) -> (f32, f32) {
        let along = (point - self.origin).dot(self.direction);
        (along, (point - self.at(along)).length())
    }
}

/// Normalized device coordinates for a pixel inside a viewport of `size`
/// pixels: x grows right, y grows *up*, both spanning -1 to 1 — the y flip
/// every windowing system's top-left origin needs before it meets a
/// projection matrix.
pub fn ndc_of(pixel: Vec2, size: Vec2) -> Vec2 {
    let clamped = size.max(Vec2::ONE);
    Vec2::new(
        2.0 * pixel.x / clamped.x - 1.0,
        1.0 - 2.0 * pixel.y / clamped.y,
    )
}

/// The world-space ray under `ndc`, given the matrix that frame was drawn
/// with. Works for a parallel projection as well as a perspective one: both
/// unproject two depths and join them, which is the only formulation that
/// doesn't assume the ray fans out from a single eye point.
pub fn ray_through(view_projection: Mat4, ndc: Vec2) -> Ray {
    let inverse = view_projection.inverse();
    let near = unproject(inverse, ndc, NEAR_DEPTH);
    let ahead = unproject(inverse, ndc, AHEAD_DEPTH);
    Ray::new(near, ahead - near)
}

fn unproject(inverse_view_projection: Mat4, ndc: Vec2, depth: f32) -> Vec3 {
    let point = inverse_view_projection * glam::Vec4::new(ndc.x, ndc.y, depth, 1.0);
    point.truncate() / point.w
}

/// The matrix a `BasePart` with this `CFrame` and `Size` is drawn with — the
/// same one `scene` builds for the GPU, so a hit test against it agrees with
/// what's actually on screen rather than approximating it.
pub fn part_model(cframe: &CFrameData, size: Vector3Data) -> Mat4 {
    cframe_matrix(cframe) * Mat4::from_scale(Vec3::new(size.x, size.y, size.z))
}

// A part scaled to nothing on some axis has a singular model matrix, which
// cannot be inverted into box space at all. Rather than let the slab test
// below produce NaNs and non-deterministic hits, such a part simply isn't
// pickable — it has no visible surface to click on either.
const SINGULAR: f32 = 1e-12;

/// How far along `ray` it first meets the unit cube carried through `model`
/// — the oriented box every `BasePart` is drawn inside, whatever shape fills
/// it. `None` when the ray misses, or when the box is entirely behind the
/// ray's origin.
///
/// A ray starting *inside* the box hits at distance 0 rather than missing, so
/// clicking while the camera sits inside a part still selects it.
pub fn ray_hits_box(ray: Ray, model: Mat4) -> Option<f32> {
    if model.determinant().abs() < SINGULAR {
        return None;
    }

    // The slab test wants the box axis-aligned, so the ray moves into the
    // box's own space instead. An affine map carries the ray parameter
    // through unchanged, so the distance that comes out is already the
    // world-space one even though the direction there isn't unit length.
    let inverse = model.inverse();
    let origin = inverse.transform_point3(ray.origin);
    let direction = inverse.transform_vector3(ray.direction);

    let mut entry = f32::NEG_INFINITY;
    let mut exit = f32::INFINITY;
    for axis in 0..3 {
        let (origin, direction) = (origin[axis], direction[axis]);
        if direction.abs() < f32::EPSILON {
            if !(-0.5..=0.5).contains(&origin) {
                return None;
            }
            continue;
        }
        let first = (-0.5 - origin) / direction;
        let second = (0.5 - origin) / direction;
        entry = entry.max(first.min(second));
        exit = exit.min(first.max(second));
    }

    if exit < entry.max(0.0) {
        return None;
    }
    Some(entry.max(0.0))
}

/// Every drawn `BasePart` `ray` passes through, nearest first.
///
/// Scoped to `Workspace`'s own descendants and filtered by the same
/// `is_drawable` test the scene builder uses, so what a click can reach is
/// exactly what is on screen — a `Part` staged in `ServerStorage` is neither
/// drawn nor clickable.
///
/// Every part is tested as the oriented box it occupies rather than against
/// its real surface: a `Ball` or a `MeshPart` is therefore clickable slightly
/// beyond its own silhouette, out to the corners of its bounding box. Studio
/// picks against the actual geometry; matching that means the renderer's mesh
/// data, which lives on the render thread and is not what this reads.
pub fn parts_along(dom: &WeakDom, database: &ReflectionDatabase, ray: Ray) -> Vec<Ref> {
    let mut hits: Vec<(f32, Ref)> = workspace_descendants(dom, database)
        .filter(|&referent| is_drawable(dom, database, referent))
        .filter_map(|referent| {
            let model = model_of(dom, referent)?;
            Some((ray_hits_box(ray, model)?, referent))
        })
        .collect();
    hits.sort_by(|(a, _), (b, _)| a.total_cmp(b));
    hits.into_iter().map(|(_, referent)| referent).collect()
}

/// The matrix one `BasePart` in `dom` is drawn with, or `None` for anything
/// without both a `CFrame` and a `size` to build one from.
pub fn model_of(dom: &WeakDom, referent: Ref) -> Option<Mat4> {
    let properties = dom.get(referent)?.properties();
    // Roblox's binary format spells `BasePart.Size` lowercase, which is the
    // name the DOM keeps — see `scene::build_part`, which reads the same pair.
    let (Variant::CFrame(cframe), Variant::Vector3(size)) =
        (properties.get("CFrame")?, properties.get("size")?)
    else {
        return None;
    };
    Some(part_model(cframe, *size))
}

/// Where `ray` crosses the plane through `point` with this `normal`, or
/// `None` when the two are parallel (or the crossing is behind the ray).
pub fn ray_hits_plane(ray: Ray, point: Vec3, normal: Vec3) -> Option<Vec3> {
    let slope = ray.direction.dot(normal);
    if slope.abs() < 1e-6 {
        return None;
    }
    let distance = (point - ray.origin).dot(normal) / slope;
    (distance > 0.0).then(|| ray.at(distance))
}

#[cfg(test)]
#[path = "pick/tests.rs"]
mod tests;
