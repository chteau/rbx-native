//! `MeshPart` operands, Studio's "Solid Modeling On Meshes": a mesh carves
//! as its own triangles, fitted to the part the way it draws, once the host
//! hands over a way to read mesh assets ([`set_mesh_source`]). Without one a
//! `MeshPart` counts as its box, as it always did here.

use std::collections::HashMap;
use std::sync::RwLock;

use rbx_csg::glam::{DMat4, DVec3};
use rbx_csg::Solid;
use rbx_dom::{Content, Instance, Variant};
use rbx_reflection::ReflectionDatabase;

use super::{read, vec3};

/// Studio's words when a mesh operand is not a closed solid, from its
/// "Union failed" dialog (Studio 0.748).
pub const NON_MANIFOLD: &str =
    "The mesh has non-manifold geometry and cannot be used in solid modeling.";

/// Reads a mesh asset's bytes by URI (`rbxassetid://…`, `rbxasset://…`).
pub type MeshSource = Box<dyn Fn(&str) -> Result<Vec<u8>, String> + Send + Sync>;

static SOURCE: RwLock<Option<MeshSource>> = RwLock::new(None);

/// Lets `MeshPart`s carve as their meshes, read through `source`, in every
/// boolean of the process (the editor's Model tab, `plugin:Union`,
/// `BasePart:UnionAsync`), or, with `None`, as their boxes.
pub fn set_mesh_source(source: Option<MeshSource>) {
    *SOURCE.write().unwrap_or_else(|e| e.into_inner()) = source;
}

/// The URI of the mesh a `MeshPart` draws, an empty one counting as none.
fn mesh_uri(instance: &Instance) -> Option<&str> {
    ["MeshId", "MeshContent"].iter().find_map(|key| {
        let uri = match instance.properties().get(*key)? {
            Variant::String(text) => text.as_str(),
            Variant::Content(Content::Uri(uri)) => uri.as_str(),
            _ => return None,
        };
        (!uri.is_empty()).then_some(uri)
    })
}

/// The `EditableMesh` a `MeshPart` draws from memory (one made by
/// `AssetService:CreateMeshPartAsync(Content.fromObject(mesh))`, with no
/// `MeshId`): its triangles, three corners each.
fn editable_triangles(instance: &Instance) -> Option<(Vec<[f32; 3]>, Vec<u32>)> {
    let Some(Variant::Content(Content::Object(referent))) = instance.properties().get("MeshContent")
    else {
        return None;
    };
    let rbx_dom::editable::Editable::Mesh(mesh) = rbx_dom::editable::get(*referent)? else {
        return None;
    };
    let positions = mesh.triangles().positions;
    let indices = (0..positions.len() as u32).collect();
    Some((positions, indices))
}

/// `instance`'s mesh in world space, placed at `at` and fitted to `size`
/// like the renderer fits it (its `InitialSize`, else its own bounds, scaled
/// to `size`): the asset it names, or the `EditableMesh` it draws. `None`
/// when it is not a `MeshPart`, has no mesh, or no source is set (the beta
/// is off): the caller falls back to the box.
pub(super) fn world_solid(
    db: &ReflectionDatabase,
    instance: &Instance,
    size: DVec3,
    at: DMat4,
) -> Option<Result<Solid, String>> {
    if !db.is_subclass_of(instance.class(), "MeshPart") {
        return None;
    }
    let guard = SOURCE.read().unwrap_or_else(|e| e.into_inner());
    let source = guard.as_ref()?;
    let (positions, indices) = match mesh_uri(instance) {
        Some(uri) => {
            match source(uri).and_then(|bytes| rbx_mesh::parse(&bytes).map_err(|e| e.to_string()))
            {
                Ok(mesh) => (
                    mesh.vertices.iter().map(|v| v.position).collect(),
                    mesh.indices,
                ),
                Err(err) => return Some(Err(format!("{}: {err}", instance.name()))),
            }
        }
        None => editable_triangles(instance)?,
    };
    if !closed(&positions, &indices) {
        return Some(Err(NON_MANIFOLD.to_owned()));
    }
    let native = match read(db, instance, "InitialSize") {
        Some(Variant::Vector3(v)) if v.x.min(v.y).min(v.z) > f32::EPSILON => vec3(*v),
        _ => used_extent(&positions, &indices),
    };
    let model = at * DMat4::from_scale(size / native.max(DVec3::splat(1e-6)));
    Some(Ok(Solid::from_triangles(&positions, &indices, model)))
}

/// The extent of the vertices `indices` uses: a file's coarser LODs keep
/// vertices of their own, which would only widen the bounds.
fn used_extent(positions: &[[f32; 3]], indices: &[u32]) -> DVec3 {
    let mut lo = DVec3::splat(f64::INFINITY);
    let mut hi = DVec3::splat(f64::NEG_INFINITY);
    for &i in indices {
        if let Some(p) = positions.get(i as usize) {
            let p = DVec3::from(p.map(f64::from));
            lo = lo.min(p);
            hi = hi.max(p);
        }
    }
    (hi - lo).max(DVec3::ZERO)
}

/// Whether the triangles close up into a solid: with corners at the same
/// spot welded (a mesh splits its vertices along UV and normal seams), every
/// edge borders exactly two triangles. Measured against Studio 0.748: an
/// open mesh and one with edges shared three ways or more both fail there.
fn closed(positions: &[[f32; 3]], indices: &[u32]) -> bool {
    /// A ten-thousandth of a stud in the mesh's own units: far finer than any
    /// modelled feature, far coarser than the float noise of an exported
    /// seam.
    const WELD: f32 = 1e-4;
    let mut ids: HashMap<[i64; 3], u32> = HashMap::new();
    let mut weld = |i: u32| -> Option<u32> {
        let key = positions
            .get(i as usize)?
            .map(|c| (c / WELD).round() as i64);
        let next = ids.len() as u32;
        Some(*ids.entry(key).or_insert(next))
    };
    let mut edges: HashMap<(u32, u32), u32> = HashMap::new();
    for triangle in indices.as_chunks::<3>().0 {
        let (Some(a), Some(b), Some(c)) = (weld(triangle[0]), weld(triangle[1]), weld(triangle[2]))
        else {
            return false;
        };
        // A collapsed triangle still counts its two real sides, both along
        // one edge: Studio refuses such a mesh too (its `headA.mesh`).
        for (x, y) in [(a, b), (b, c), (c, a)] {
            if x != y {
                *edges.entry((x.min(y), x.max(y))).or_default() += 1;
            }
        }
    }
    !edges.is_empty() && edges.values().all(|&n| n == 2)
}

#[cfg(test)]
mod tests;
