//! The dynamic head side of `collect`: the head part's `FaceControls`,
//! `WrapTarget` cage and part settings read off the tree, then its mesh
//! posed for every FACS check off the UI thread (see `rbx_ugc::head`).

use std::sync::Arc;

use rbx_reflection::ReflectionDatabase;
use rbx_ugc::head::{pose_list, Facs, Head};

use super::*;

/// What [`resolve_head`] needs from the tree.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::shell::ugc) struct HeadPlan {
    pub(in crate::shell::ugc) part: Ref,
    name: String,
    mesh: Option<Key>,
    size: [f32; 3],
    face_controls: bool,
    /// The outer cage's mesh and its `CageOrigin`.
    cage: Option<(Key, CFrame)>,
    material: String,
    transparency: f32,
    vertex_color: [f32; 3],
    extraneous: Vec<String>,
    collision_fidelity: String,
    scale_type: bool,
}

/// The head mesh as the face preview and the checks use it.
#[derive(Debug, Clone)]
pub(in crate::shell::ugc) struct HeadMesh {
    pub(in crate::shell::ugc) source: Arc<rbx_mesh::SkinnedMesh>,
    center: [f32; 3],
    factor: [f32; 3],
}

impl HeadMesh {
    /// Native mesh positions into the part's space, in studs, the way
    /// `resolve::geometry` fits a part's mesh to its `Size`.
    pub(in crate::shell::ugc) fn to_part(&self, native: &[[f32; 3]]) -> Vec<[f32; 3]> {
        native
            .iter()
            .map(|p| std::array::from_fn(|a| (p[a] - self.center[a]) * self.factor[a]))
            .collect()
    }

    /// The mesh posed with each control at `weight(property)`, in the
    /// part's space; the rest mesh when it has no FACS block.
    pub(in crate::shell::ugc) fn posed(&self, weight: &dyn Fn(&str) -> f32) -> Vec<[f32; 3]> {
        let rest: Vec<[f32; 3]> = self.source.vertices.iter().map(|v| v.position).collect();
        let Some(skin) = &self.source.skin else {
            return self.to_part(&rest);
        };
        match skin.facs_palette(weight) {
            Some(palette) => self.to_part(&rbx_mesh::deform_positions(skin, &rest, &palette)),
            None => self.to_part(&rest),
        }
    }
}

fn float(props: &BTreeMap<String, Variant>, key: &str, default: f32) -> f32 {
    match props.get(key) {
        Some(Variant::Float32(v)) => *v,
        _ => default,
    }
}

/// The head part of `root`: itself when it is a `MeshPart`, else its
/// `MeshPart` named `Head`.
pub(in crate::shell::ugc) fn head_part(dom: &WeakDom, root: Ref) -> Option<Ref> {
    let inst = dom.get(root)?;
    if inst.class() == "MeshPart" {
        return Some(root);
    }
    let mut all = Vec::new();
    descendants(dom, root, &mut all);
    all.into_iter().find(|r| {
        dom.get(*r)
            .is_some_and(|c| c.class() == "MeshPart" && c.name() == "Head")
    })
}

pub(in crate::shell::ugc) fn head_plan(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    root: Ref,
) -> Option<HeadPlan> {
    let part = head_part(dom, root)?;
    let inst = dom.get(part)?;
    let props = inst.properties();
    let child = |class: &str| {
        inst.children()
            .iter()
            .filter_map(|c| dom.get(*c))
            .find(|c| c.class() == class)
    };
    let cage = child("WrapTarget").and_then(|wrap| {
        let props = wrap.properties();
        let mesh = uri(props, "CageMeshId").and_then(mesh_key)?;
        Some((mesh, cframe_at(props, "CageOrigin")))
    });
    let material = match props.get("Material") {
        Some(Variant::Enum(v)) => database.enum_name("Material", *v).unwrap_or("Plastic"),
        _ => "Plastic",
    };
    // A place file Studio saved has no `CollisionFidelity`: a Box part
    // keeps a `BLOCK` marker in `PhysicalConfigData` instead, which is
    // how Roblox's own head ships, and the value read back is the
    // reflection default. Hull and Precise can't be told from Default
    // that way, so only Box is caught there.
    let named = match props.get("CollisionFidelity") {
        Some(Variant::Enum(v)) => database.enum_name("CollisionFidelity", *v),
        _ => None,
    };
    let boxed = match props.get("PhysicalConfigData") {
        Some(Variant::Unknown { raw, .. }) => raw.windows(5).any(|w| w == b"BLOCK"),
        Some(Variant::String(data)) => data.contains("BLOCK"),
        _ => false,
    };
    let collision_fidelity = match named {
        Some(name) if name != "Default" => name,
        _ if boxed => "Box",
        _ => "Default",
    };
    let scale_type = inst
        .children()
        .iter()
        .filter_map(|c| dom.get(*c))
        .any(|c| c.class() == "StringValue" && c.name() == "AvatarPartScaleType");
    let vertex_color = match props.get("VertexColor") {
        Some(Variant::Vector3(v)) => [v.x, v.y, v.z],
        _ => [1.0; 3],
    };
    let mut all = Vec::new();
    descendants(dom, root, &mut all);
    let extraneous = all
        .iter()
        .filter_map(|r| dom.get(*r))
        .filter(|c| {
            matches!(
                c.class(),
                "Script" | "LocalScript" | "ModuleScript" | "Part"
            )
        })
        .map(|c| format!("{} ({})", c.name(), c.class()))
        .collect();
    let plan = part_plan(dom, part)?;
    Some(HeadPlan {
        part,
        name: plan.name,
        mesh: plan.mesh,
        size: plan.size,
        face_controls: child("FaceControls").is_some(),
        cage,
        material: material.to_string(),
        transparency: float(props, "Transparency", 0.0),
        vertex_color,
        extraneous,
        collision_fidelity: collision_fidelity.to_string(),
        scale_type,
    })
}

fn apply(cframe: &CFrame, p: [f32; 3]) -> [f32; 3] {
    let r = &cframe[3..];
    std::array::from_fn(|i| r[i * 3] * p[0] + r[i * 3 + 1] * p[1] + r[i * 3 + 2] * p[2] + cframe[i])
}

/// Blocking: the head mesh and cage fetched, the mesh posed for every
/// check. `None` mesh when it would not load (the item's notes say why).
pub(in crate::shell::ugc) fn resolve_head(
    plan: &HeadPlan,
    fetch: &dyn Fn(&Key) -> Result<Vec<u8>, String>,
) -> (Head, Option<HeadMesh>) {
    let source = plan
        .mesh
        .as_ref()
        .and_then(|key| fetch(key).ok())
        .and_then(|bytes| rbx_mesh::parse_skinned(&bytes).ok());
    let mesh = source.map(|source| {
        let (center, size) = (source.bounds.center(), source.bounds.size());
        let factor = std::array::from_fn(|a| match size[a] {
            0.0 => 1.0,
            s => plan.size[a] / s,
        });
        HeadMesh {
            source: Arc::new(source),
            center,
            factor,
        }
    });
    let facs = mesh.as_ref().and_then(|m| {
        let facs = m.source.skin.as_ref()?.facs.as_ref()?;
        let controls: Vec<String> = facs
            .controls
            .iter()
            .map(|c| rbx_mesh::control_property(c).to_string())
            .collect();
        let active = controls
            .iter()
            .filter(|c| facs.is_active(c))
            .cloned()
            .collect();
        let poses = pose_list(&controls)
            .into_iter()
            .map(|(name, values)| {
                let weight = |p: &str| values.iter().find(|(c, _)| c == p).map_or(0.0, |v| v.1);
                (name, m.posed(&weight))
            })
            .collect();
        Some(Facs {
            controls,
            active,
            poses,
        })
    });
    let geometry = mesh.as_ref().map(|m| Geometry {
        positions: m.posed(&|_| 0.0),
        triangles: m.source.lod0().as_chunks::<3>().0.to_vec(),
        uvs: m.source.vertices.iter().map(|v| v.uv).collect(),
        influences: Vec::new(),
        bones: Vec::new(),
        in_studs: true,
    });
    let cage = plan.cage.as_ref().and_then(|(key, origin)| {
        let cage = rbx_mesh::parse(&fetch(key).ok()?).ok()?;
        let factor = mesh.as_ref().map_or([1.0; 3], |m| m.factor);
        Some(Geometry {
            positions: cage
                .vertices
                .iter()
                .map(|v| {
                    let p = apply(origin, v.position);
                    std::array::from_fn(|a| p[a] * factor[a])
                })
                .collect(),
            triangles: cage.lod0().as_chunks::<3>().0.to_vec(),
            uvs: cage.vertices.iter().map(|v| v.uv).collect(),
            influences: Vec::new(),
            bones: Vec::new(),
            in_studs: true,
        })
    });
    let head = Head {
        name: plan.name.clone(),
        face_controls: plan.face_controls,
        mesh: geometry,
        facs,
        cage,
        material: plan.material.clone(),
        transparency: plan.transparency,
        vertex_color: plan.vertex_color,
        extraneous: plan.extraneous.clone(),
        collision_fidelity: plan.collision_fidelity.clone(),
        scale_type: plan.scale_type,
    };
    (head, mesh)
}

#[cfg(test)]
mod tests;
