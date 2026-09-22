//! One part's surface, for an editor that needs more than *whether* a ray
//! hits it: where, which way the surface faces there, and what kind of solid
//! Studio's draggers take it for — the question their target frames branch
//! on (a box face, a wedge's slope, a ball, a cylinder's cap or side, a
//! mesh).

use std::sync::Arc;

use glam::{Mat4, Vec3};
use rbx_dom::{Ref, Variant, WeakDom};
use rbx_mesh::Mesh;
use rbx_reflection::ReflectionDatabase;

use super::shape;
use super::{mesh, Meshes, Ray};
use crate::scene::{cframe_matrix, file_mesh_fit, resolve_shape, ShapeKind};

/// The kinds of solid Studio's draggers tell apart: by class and by
/// `Part.Shape`, never by a mesh child — a `SpecialMesh` changes what is
/// drawn, not what the dragger measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Solid {
    /// Blocks, and every other part the draggers take as its box: trusses,
    /// seats, spawn locations.
    Box,
    Wedge,
    CornerWedge,
    Ball,
    /// `Part.Shape = Cylinder`, lying along the part's own X.
    Cylinder,
    /// A `MeshPart` or a union, whose faces the draggers probe for.
    Mesh,
}

/// One part as the draggers see it: its solid, its box (the part's `CFrame`
/// scaled by its `Size`) and what a ray tests against.
#[derive(Clone)]
pub struct PartSurface {
    pub solid: Solid,
    pub model: Mat4,
    drawn: Drawn,
}

#[derive(Clone)]
enum Drawn {
    Shape(ShapeKind, Mat4),
    Mesh(Arc<Mesh>, Mat4),
}

impl PartSurface {
    /// `referent`'s surface, or `None` for anything without a `CFrame` and
    /// a `size`.
    pub fn read(
        dom: &WeakDom,
        database: &ReflectionDatabase,
        meshes: &Meshes,
        referent: Ref,
    ) -> Option<Self> {
        let instance = dom.get(referent)?;
        let properties = instance.properties();
        let (Variant::CFrame(cframe), Variant::Vector3(size)) =
            (properties.get("CFrame")?, properties.get("size")?)
        else {
            return None;
        };
        let size = Vec3::new(size.x, size.y, size.z);
        let placement = cframe_matrix(cframe);
        let model = placement * Mat4::from_scale(size);
        let is = |class: &str| database.is_subclass_of(instance.class(), class);
        let solid = if is("WedgePart") {
            Solid::Wedge
        } else if is("CornerWedgePart") {
            Solid::CornerWedge
        } else if is("TriangleMeshPart") {
            Solid::Mesh
        } else {
            match properties.get("shape") {
                Some(Variant::Enum(0)) => Solid::Ball,
                Some(Variant::Enum(2)) => Solid::Cylinder,
                Some(Variant::Enum(3)) => Solid::Wedge,
                Some(Variant::Enum(4)) => Solid::CornerWedge,
                _ => Solid::Box,
            }
        };
        let drawn = file_mesh_fit(dom, database, referent)
            .and_then(|(asset, fit)| {
                let mesh = meshes.get(&asset)?.clone();
                let transform = fit.transform(&mesh);
                Some(Drawn::Mesh(mesh, transform))
            })
            .unwrap_or_else(|| {
                let geometry = resolve_shape(dom, database, instance, size);
                Drawn::Shape(geometry.kind, geometry.model(placement))
            });
        Some(PartSurface {
            solid,
            model,
            drawn,
        })
    }

    /// Where `ray` first meets the surface drawn, and the surface's outward
    /// unit normal there — a Roblox raycast's `Distance` and `Normal`: a
    /// wedge's slope, a ball's curve, a downloaded mesh's triangle. `None`
    /// when it misses, when the part lies wholly behind the ray, or when the
    /// ray starts inside a solid and so enters through no face.
    pub fn raycast(&self, ray: Ray) -> Option<(f32, Vec3)> {
        let (point, normal) = match &self.drawn {
            Drawn::Shape(kind, model) => shape::surface(*kind, *model, ray)?,
            Drawn::Mesh(mesh, model) => mesh::surface(mesh, *model, ray)?,
        };
        Some((point.distance(ray.origin), normal))
    }
}

#[cfg(test)]
#[path = "surface/tests.rs"]
mod tests;
