//! Turning a point on screen into a world-space ray, and testing that ray
//! against the surfaces parts are drawn with.
//!
//! Public because the picking happens in the *embedder*: `rbxstudio`'s
//! viewport owns the cursor and the DOM, while the renderer runs on a thread
//! of its own behind a one-way command channel. Keeping the unprojection here
//! rather than mirroring it there is what stops a click from resolving against
//! a slightly different camera than the frame under it was drawn with — see
//! [`Pose::view_projection`], which is the matrix both sides share.
//!
//! The same reasoning puts the hit tests here: [`parts_along`] resolves each
//! part's shape through the very `scene` code that decides what the GPU draws
//! (`scene::shape::resolve` for the procedural solids, `scene::filemesh` for
//! a downloaded mesh), so what a click reaches is exactly the silhouette on
//! screen — not a box around it.

mod mesh;
mod shape;

use glam::{Mat4, Vec2, Vec3};
use rbx_dom::{CFrameData, Ref, Variant, Vector3Data, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::scene::{
    cframe_matrix, descendants_of, file_mesh_fit, is_drawable, resolve_shape,
    workspace_descendants, ShapeKind,
};

pub use mesh::Meshes;

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

/// How far along `ray` it first meets the unit cube carried through `model`
/// — the oriented box every `BasePart` occupies, whatever shape fills it.
/// `None` when the ray misses, or when the box is entirely behind the ray's
/// origin.
///
/// A ray starting *inside* the box hits at distance 0 rather than missing, so
/// clicking while the camera sits inside a part still selects it.
pub fn ray_hits_box(ray: Ray, model: Mat4) -> Option<f32> {
    shape::hit(ShapeKind::Box, model, ray)
}

/// Every drawn `BasePart` `ray` passes through, nearest first.
///
/// Scoped to `Workspace`'s own descendants and filtered by the same
/// `is_drawable` test the scene builder uses, so what a click can reach is
/// exactly what is on screen — a `Part` staged in `ServerStorage` is neither
/// drawn nor clickable.
///
/// Each part is tested against the surface it is drawn with: a `Ball` as a
/// sphere, a `Cylinder` along its own axis, a wedge under its slope, and a
/// `MeshPart` (or a `SpecialMesh` FileMesh) against the triangles of its
/// downloaded mesh where `meshes` holds them — so clicking between a ball's
/// silhouette and the corner of its bounding box hits whatever stands behind
/// it, as it does in Studio. Everything else, a `MeshPart` whose download
/// failed included, picks as the box it is drawn as.
pub fn parts_along(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    meshes: &Meshes,
    ray: Ray,
) -> Vec<Ref> {
    let mut hits: Vec<(f32, Ref)> = drawable_parts(dom, database)
        .filter_map(|referent| Some((distance_to(dom, database, meshes, referent, ray)?, referent)))
        .collect();
    hits.sort_by(|(a, _), (b, _)| a.total_cmp(b));
    hits.into_iter().map(|(_, referent)| referent).collect()
}

/// How `parts_along` orders one part along `ray`: how far its drawn surface
/// lies, except that a *box* the ray starts inside is ordered by where the ray
/// leaves it rather than by 0 (see `shape::hit_key`), so a part the camera
/// sits inside sorts behind whatever it encloses. `None` when the ray misses
/// it (or the part has nothing to draw). Only ever used to sort, never as a
/// hit point.
fn distance_to(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    meshes: &Meshes,
    referent: Ref,
    ray: Ray,
) -> Option<f32> {
    if let Some((asset, fit)) = file_mesh_fit(dom, database, referent) {
        if let Some(mesh) = meshes.get(&asset) {
            return mesh::hit(mesh, fit.transform(mesh), ray);
        }
        // The mesh never downloaded: the part is drawn as its fallback box,
        // which is exactly what the shape resolution below answers for it.
    }

    let instance = dom.get(referent)?;
    let properties = instance.properties();
    // Roblox's binary format spells `BasePart.Size` lowercase, which is the
    // name the DOM keeps — see `scene::build_part`, which reads the same pair.
    let (Variant::CFrame(cframe), Variant::Vector3(size)) =
        (properties.get("CFrame")?, properties.get("size")?)
    else {
        return None;
    };
    let geometry = resolve_shape(dom, database, instance, Vec3::new(size.x, size.y, size.z));
    shape::hit_key(geometry.kind, geometry.model(cframe_matrix(cframe)), ray)
}

/// Everything in the scene a click or a drag can resolve against: `Workspace`'s
/// own descendants, filtered by the same `is_drawable` test the scene builder
/// uses, so what the editor can reach is exactly what is on screen.
pub fn drawable_parts<'a>(
    dom: &'a WeakDom,
    database: &'a ReflectionDatabase,
) -> impl Iterator<Item = Ref> + 'a {
    workspace_descendants(dom, database)
        .filter(move |&referent| is_drawable(dom, database, referent))
}

/// Every drawable `BasePart` `referent` stands for: itself, when it is one,
/// and otherwise everything beneath it.
///
/// A `Model`, a `Folder` or a service carries no `CFrame` of its own, so the
/// only geometry the viewport can outline or transform for one is what it
/// contains. That is not an edge case: a click in the 3D view resolves to the
/// outermost `Model` around whatever it hit (`rbxstudio`'s
/// `shell::selection::outermost_model`, matching Studio), so most selections a
/// user makes by clicking arrive here as a container rather than as a part.
///
/// A part stands for itself *alone*, even where parts are parented under it —
/// a welded assembly, or a `Tool`'s `Handle` with something screwed onto it.
/// Nothing in Roblox moves a child part because its parent part moved (only a
/// weld or a `Model`'s pivot does), so pulling those in would drag geometry
/// the user did not select, and would swell the box drawn around one part
/// into a loose box around several.
///
/// Both sides of the editor resolve a selection through this one function —
/// the renderer that draws the handles and the viewport that hit-tests the
/// cursor against them — so the order matters as much as the membership: the
/// first part yielded is the one Scale and Rotate anchor on, and two
/// derivations disagreeing about which that is would draw the handles
/// somewhere the cursor cannot reach.
pub fn parts_of<'a>(
    dom: &'a WeakDom,
    database: &'a ReflectionDatabase,
    referent: Ref,
) -> impl Iterator<Item = Ref> + 'a {
    let itself = is_drawable(dom, database, referent).then_some(referent);
    let beneath = itself
        .is_none()
        .then(|| {
            descendants_of(dom, referent).filter(move |&found| is_drawable(dom, database, found))
        })
        .into_iter()
        .flatten();
    itself.into_iter().chain(beneath)
}

/// One entry in the viewport's selection: the instance the Explorer names,
/// and every drawable part it stands for (see [`parts_of`]).
///
/// Resolved by the editor, which owns the DOM, and handed to the renderer
/// whole rather than worked out again there. The render thread holds no DOM —
/// every command that needs one ships a clone of the entire place — and a
/// copy of the place per selection change, for what is usually one click,
/// would cost far more than the handful of referents this carries instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selected {
    referent: Ref,
    /// Whether `referent` is a drawable `BasePart` itself, answered from its
    /// own class while the DOM is still in hand — the render thread has none
    /// to ask again, and the shape of [`Selected::parts`] cannot stand in for
    /// the question (see [`Selected::is_part`]).
    drawable: bool,
    parts: Vec<Ref>,
}

impl Selected {
    pub fn read(dom: &WeakDom, database: &ReflectionDatabase, referent: Ref) -> Self {
        Selected {
            referent,
            drawable: is_drawable(dom, database, referent),
            parts: parts_of(dom, database, referent).collect(),
        }
    }

    /// One `BasePart` standing for itself, for a caller that already knows it
    /// is one and has no DOM in hand to say so again — what every entry looked
    /// like before a container could be selected at all.
    pub fn part(referent: Ref) -> Self {
        Selected {
            referent,
            drawable: true,
            parts: vec![referent],
        }
    }

    pub fn referent(&self) -> Ref {
        self.referent
    }

    /// The parts this entry covers, in the order the gizmo's anchor is picked
    /// from — empty for a container holding no drawable geometry at all,
    /// which is the one case that still outlines and transforms nothing.
    pub fn parts(&self) -> &[Ref] {
        &self.parts
    }

    /// Whether the selected instance is a drawable part in its own right.
    ///
    /// Only then does it have an orientation of its own to draw an oriented
    /// bounding box along; a container is outlined by one world-axis-aligned
    /// box around everything beneath it instead, the same extent
    /// `creator-docs` means by a model's bounding box (`studio/pivot-tools.md`).
    ///
    /// Read from the instance's own class rather than inferred from what
    /// [`parts_of`] answered for it: the two are not the same question, and
    /// only the class says which box the part deserves.
    pub fn is_part(&self) -> bool {
        self.drawable
    }
}

/// What a selection covers, one entry per referent, with anything already
/// covered by another entry left out.
///
/// Selecting a `Model` *and* something inside it names the same geometry
/// twice. A group drag would then move that part twice as far as the gizmo
/// travelled, and the outline would draw a second box inside the first,
/// visibly doubled along every shared edge. The entry covering the other wins,
/// because its box is the one that spans everything the user picked; between
/// two entries covering exactly the same parts the earlier one wins, since the
/// anchor Scale and Rotate act on is taken from the front.
///
/// Both halves of the editor resolve a selection through here — the boxes
/// `rbxstudio`'s `shell::selection::outlined` sends the renderer and the
/// parts its `transform::Targets::read` hit-tests — so neither can disagree
/// with the other about what a selection covers.
pub fn selection(dom: &WeakDom, database: &ReflectionDatabase, referents: &[Ref]) -> Vec<Selected> {
    let entries: Vec<Selected> = referents
        .iter()
        .map(|&referent| Selected::read(dom, database, referent))
        .collect();

    entries
        .iter()
        .enumerate()
        .filter(|&(index, entry)| {
            !entries.iter().enumerate().any(|(other, candidate)| {
                other != index
                    && covers(candidate, entry)
                    // Equal coverage is a tie only position can break.
                    && (candidate.parts.len() > entry.parts.len() || other < index)
            })
        })
        .map(|(_, entry)| entry.clone())
        .collect()
}

/// Whether every part `inner` covers is one `outer` covers too.
///
/// Both sets are a node's drawable descendants (or a lone part), and two such
/// sets in a tree are nested or disjoint — never partly overlapping. So one
/// shared part already settles which way round they nest, and the sizes settle
/// which of the two is the container.
fn covers(outer: &Selected, inner: &Selected) -> bool {
    inner.parts.len() <= outer.parts.len()
        && inner
            .parts
            .first()
            .is_some_and(|part| outer.parts.contains(part))
}

/// The matrix one `BasePart` in `dom` is drawn with, or `None` for anything
/// without both a `CFrame` and a `size` to build one from.
pub fn model_of(dom: &WeakDom, referent: Ref) -> Option<Mat4> {
    let properties = dom.get(referent)?.properties();
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
