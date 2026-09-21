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
    /// normal there — a Roblox raycast's `Distance` and `Normal`. `None` when
    /// it misses, or when the part lies wholly behind the ray.
    pub fn raycast(&self, ray: Ray) -> Option<(f32, Vec3)> {
        match &self.drawn {
            Drawn::Shape(kind, model) => {
                let distance = shape::hit(*kind, *model, ray)?;
                let local = model.inverse().transform_point3(ray.at(distance));
                let normal = shape_normal(*kind, local);
                Some((distance, outward(*model, normal)))
            }
            Drawn::Mesh(mesh, model) => {
                let (distance, normal) = mesh::hit_with_normal(mesh, *model, ray)?;
                let normal = outward(*model, normal);
                // Winding says nothing reliable about which side is out; the
                // side the ray arrived from is.
                let normal = if normal.dot(ray.direction) > 0.0 {
                    -normal
                } else {
                    normal
                };
                Some((distance, normal))
            }
        }
    }
}

/// A normal of the unit solid of `kind` at `local`, a point on its surface,
/// in the solid's own space.
fn shape_normal(kind: ShapeKind, local: Vec3) -> Vec3 {
    // Which of a few candidate faces the point lies on: the one it is least
    // inside of.
    let nearest = |faces: &[(Vec3, f32)]| {
        faces
            .iter()
            .copied()
            .max_by(|(a, da), (b, db)| (a.dot(local) - da).total_cmp(&(b.dot(local) - db)))
            .map(|(normal, _)| normal)
            .unwrap_or(Vec3::Y)
    };
    let slope = |normal: Vec3| (normal.normalize(), 0.0);
    let box_faces = [
        (Vec3::X, 0.5),
        (Vec3::NEG_X, 0.5),
        (Vec3::Y, 0.5),
        (Vec3::NEG_Y, 0.5),
        (Vec3::Z, 0.5),
        (Vec3::NEG_Z, 0.5),
    ];
    match kind {
        ShapeKind::Ball => local.normalize_or(Vec3::Y),
        ShapeKind::CylinderX | ShapeKind::CylinderY => {
            let axis = if kind == ShapeKind::CylinderX { 0 } else { 1 };
            let mut radial = local;
            radial[axis] = 0.0;
            if local[axis].abs() >= 0.5 - 1e-4 {
                let mut cap = Vec3::ZERO;
                cap[axis] = local[axis].signum();
                cap
            } else {
                radial.normalize_or(Vec3::Y)
            }
        }
        // Matches `shape::span_of`: the half-spaces the slopes are cut by.
        ShapeKind::Wedge => {
            let mut faces = box_faces.to_vec();
            faces.push(slope(Vec3::new(0.0, 1.0, -1.0)));
            nearest(&faces)
        }
        ShapeKind::CornerWedge => {
            let mut faces = box_faces.to_vec();
            faces.push(slope(Vec3::new(0.0, 1.0, 1.0)));
            faces.push(slope(Vec3::new(-1.0, 1.0, 0.0)));
            nearest(&faces)
        }
        ShapeKind::Box | ShapeKind::Truss { .. } => nearest(&box_faces),
    }
}

/// A normal in a solid's own space carried into the world through `model`:
/// by the inverse transpose, since the solid is scaled unevenly.
fn outward(model: Mat4, normal: Vec3) -> Vec3 {
    let matrix = glam::Mat3::from_mat4(model).inverse().transpose();
    (matrix * normal).normalize_or(normal)
}

#[cfg(test)]
#[path = "surface/tests.rs"]
mod tests;
