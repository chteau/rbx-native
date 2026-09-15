//! Where a file mesh's native geometry goes: which asset a `MeshPart` or a
//! `SpecialMesh` FileMesh child draws, and the transform that fits it to the
//! part. Read off the DOM alone, before the mesh itself is known, so the same
//! answer serves both the scene builder and a hit test that has nothing but
//! the DOM in hand.

use glam::{Mat4, Vec3};
use rbx_assets::AssetRef;
use rbx_dom::{Instance, Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::{parsed_asset_ref, vector3, FILE_MESH, MESH_PART};
use crate::scene::cframe_matrix;

/// How to place a mesh asset's native geometry in the world, before the mesh
/// itself is known.
pub(crate) enum Fit {
    /// `MeshPart`: native geometry is scaled componentwise so its own extent
    /// matches `size` — `InitialSize` if the file carries it (undocumented but
    /// still round-tripped by Studio), otherwise the mesh's own bounds.
    Part {
        cframe: Mat4,
        size: Vec3,
        initial_size: Option<Vec3>,
    },
    /// `SpecialMesh`: native geometry (studs as authored) is scaled by `Scale`
    /// then translated by `Offset`, both in the parent part's own frame. The
    /// part's `size` plays no part in this: unlike `MeshPart`, nothing here
    /// asks the mesh to fit any particular extent.
    Special {
        cframe: Mat4,
        scale: Vec3,
        offset: Vec3,
    },
}

impl Fit {
    /// The model matrix `mesh`'s native vertices are drawn through.
    pub(crate) fn transform(&self, mesh: &rbx_mesh::Mesh) -> Mat4 {
        match self {
            Fit::Part {
                cframe,
                size,
                initial_size,
            } => {
                let native = initial_size
                    .filter(|v| v.min_element() > f32::EPSILON)
                    .unwrap_or_else(|| Vec3::from(mesh.bounds.size()))
                    .max(Vec3::splat(f32::EPSILON));
                *cframe * Mat4::from_scale(*size / native)
            }
            Fit::Special {
                cframe,
                scale,
                offset,
            } => *cframe * Mat4::from_translation(*offset) * Mat4::from_scale(*scale),
        }
    }
}

/// Which mesh asset `referent` draws and how it is fitted, or `None` for a
/// part that is not file-mesh-backed. Same precedence as [`super::plan`]: a
/// `MeshPart`'s own mesh first, then a plain part wearing a `SpecialMesh`
/// FileMesh child.
pub(crate) fn of(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    referent: Ref,
) -> Option<(AssetRef, Fit)> {
    let instance = dom.get(referent)?;
    mesh_part(database, instance)
        .or_else(|| special_mesh(dom, instance).map(|(_, asset, fit)| (asset, fit)))
}

/// A `MeshPart`'s own `MeshId`, fitted to its `size`.
pub(super) fn mesh_part(
    database: &ReflectionDatabase,
    instance: &Instance,
) -> Option<(AssetRef, Fit)> {
    if !database.is_subclass_of(instance.class(), MESH_PART) {
        return None;
    }
    let properties = instance.properties();
    let mesh = parsed_asset_ref(properties.get("MeshId")?)?;
    let Some(&Variant::Vector3(size)) = properties.get("size") else {
        return None;
    };
    let Some(Variant::CFrame(cframe)) = properties.get("CFrame") else {
        return None;
    };
    let initial_size = match properties.get("InitialSize") {
        Some(&Variant::Vector3(v)) => Some(Vec3::new(v.x, v.y, v.z)),
        _ => None,
    };
    let fit = Fit::Part {
        cframe: cframe_matrix(cframe),
        size: Vec3::new(size.x, size.y, size.z),
        initial_size,
    };
    Some((mesh, fit))
}

/// The `SpecialMesh` FileMesh child that replaces `part`'s own geometry, with
/// the child itself so the scene builder can read the texture and tint only
/// it cares about. Only the FileMesh case answers: every other `MeshType`
/// already has a procedural stand-in built in `scene::shape::resolve`.
pub(super) fn special_mesh<'a>(
    dom: &'a WeakDom,
    part: &Instance,
) -> Option<(&'a Instance, AssetRef, Fit)> {
    let Some(Variant::CFrame(cframe)) = part.properties().get("CFrame") else {
        return None;
    };
    let child = part.children().iter().find_map(|&child_ref| {
        let child = dom.get(child_ref)?;
        (child.class() == "SpecialMesh").then_some(child)
    })?;
    let properties = child.properties();
    if !matches!(properties.get("MeshType"), Some(&Variant::Enum(FILE_MESH))) {
        return None;
    }
    let mesh = parsed_asset_ref(properties.get("MeshId")?)?;
    let fit = Fit::Special {
        cframe: cframe_matrix(cframe),
        scale: vector3(properties.get("Scale"), Vec3::ONE),
        offset: vector3(properties.get("Offset"), Vec3::ZERO),
    };
    Some((child, mesh, fit))
}
