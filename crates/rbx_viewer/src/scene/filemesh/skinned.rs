//! Skinned `MeshPart`s deformed live by the parts their bones are named
//! after.
//!
//! Roblox binds a mesh bone to the part (or `Bone`) of the same name in the
//! character, so a skinned R15 body part follows its own part and bends at the
//! joints into its neighbours. Here a bone binds to a part **among the
//! `MeshPart`'s siblings in its `Model`**, matched by `Name`; a bone that
//! names none stays at its bind pose.
//!
//! # Convention
//!
//! A bone's stored CFrame `bind_b` is its bind pose in the mesh's native
//! space: relative to the part, unrotated, before the `Size`/`InitialSize`
//! fit scale `S` (see [`super::Fit::transform`]). The bone's joint frame is
//! `J_b = part_b.CFrame · K_b`, where `K_b` is that part's fixed offset to its
//! joint, taken once from the rest state the place was opened in so that
//! `J_b` at rest equals `meshPart.CFrame · S·bind_b`:
//!
//! ```text
//! K_b = part_b_rest.CFrame⁻¹ · meshPart_rest.CFrame · S·bind_b
//! ```
//!
//! — the translation `o_b` alone when the bind rotation agrees with the two
//! parts' rest orientation (what the auto-rigger writes), the full transform
//! otherwise so a part already turned at rest does not twist its skin. Each
//! bone's skinning matrix, in the part's (scaled) space, is then
//!
//! ```text
//! M_b = meshPart.CFrame⁻¹ · J_b · (S·bind_b)⁻¹
//! ```
//!
//! — identity at rest — and in native space `S⁻¹ · M_b · S`, which is what
//! each vertex is blended through: `v' = Σ wᵢ · M_bᵢ · v` (weights
//! renormalized to sum to 1). Normals go through each matrix's
//! inverse-transpose and keep their stored length.
//!
//! The deformed copy is a mesh of its own under [`key`], so the renderer,
//! the hit test and the export all read it like any other derived mesh (see
//! `wrap`); it keeps the source's `bounds`, so its fit never changes with
//! the pose.

use std::collections::HashMap;
use std::sync::Arc;

use glam::{Mat3, Mat4, Vec3};
use rbx_assets::AssetRef;
use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::{Entry, Fit};
use crate::scene::cframe_matrix;

/// The derived mesh a skinned instance draws through.
pub(crate) fn key(referent: Ref) -> AssetRef {
    AssetRef::Thumb(format!("skinned-mesh/{referent:?}"))
}

/// Matrices closer than this to the last ones applied leave the mesh alone.
const EPSILON: f32 = 1e-6;

/// The `Model` a `MeshPart`'s bones look for their parts in: its parent,
/// when that is a `Model` but not the world itself.
pub(super) fn model_of(dom: &WeakDom, database: &ReflectionDatabase, referent: Ref) -> Option<Ref> {
    let parent = dom.parent(referent)?;
    let class = dom.get(parent)?.class();
    (database.is_subclass_of(class, "Model")
        && !database.is_subclass_of(class, "WorldRoot")
        && class != "Workspace")
        .then_some(parent)
}

fn cframe_of(dom: &WeakDom, referent: Ref) -> Option<Mat4> {
    match dom.get(referent)?.properties().get("CFrame") {
        Some(Variant::CFrame(cframe)) => Some(cframe_matrix(cframe)),
        _ => None,
    }
}

/// The parts a skinned mesh can bind to, read once when the place is planned,
/// with their rest and current frames.
///
/// Only `Model`s that hold a `MeshPart` are walked, each once. A part added
/// to such a model, or renamed, after the plan is not bound until a reload.
#[derive(Default)]
pub(crate) struct Poses {
    /// Per model: its child parts by `Name`, the first of a name winning.
    models: HashMap<Ref, HashMap<String, Ref>>,
    /// `CFrame` at plan time: the rest pose.
    rest: HashMap<Ref, Mat4>,
    /// `CFrame` as of the last edit `track` saw.
    current: HashMap<Ref, Mat4>,
    /// Some tracked part moved since [`super::super::Scene::pose_skins`] ran.
    dirty: bool,
}

impl Poses {
    pub(super) fn plan(dom: &WeakDom, database: &ReflectionDatabase, entries: &[Entry]) -> Self {
        let mut poses = Poses::default();
        for model in entries.iter().filter_map(|entry| entry.model) {
            if poses.models.contains_key(&model) {
                continue;
            }
            let mut names = HashMap::new();
            for &child in dom.get(model).map(|m| m.children()).unwrap_or_default() {
                let Some(instance) = dom.get(child) else {
                    continue;
                };
                if !database.is_subclass_of(instance.class(), "BasePart") {
                    continue;
                }
                let Some(cframe) = cframe_of(dom, child) else {
                    continue;
                };
                names.entry(instance.name().to_owned()).or_insert(child);
                poses.rest.insert(child, cframe);
            }
            poses.models.insert(model, names);
        }
        poses.current = poses.rest.clone();
        poses
    }

    /// Notes where `referent` stands now if a skinned mesh could bind to it;
    /// a no-op for every other part.
    pub(crate) fn track(&mut self, dom: &WeakDom, referent: Ref) {
        let Some(held) = self.current.get_mut(&referent) else {
            return;
        };
        if let Some(cframe) = cframe_of(dom, referent) {
            if *held != cframe {
                *held = cframe;
                self.dirty = true;
            }
        }
    }

    /// Whether anything moved since the last call, clearing the flag.
    pub(crate) fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    fn current(&self, referent: Ref) -> Option<Mat4> {
        self.current.get(&referent).copied()
    }
}

/// One bone bound to a part.
#[derive(Clone)]
struct Bound {
    part: Ref,
    /// `K_b · (S·bind_b)⁻¹`: everything in `M_b` that does not move.
    tail: Mat4,
}

/// A skinned instance: where its rest geometry comes from and which part
/// drives each of its bones.
#[derive(Clone)]
pub(crate) struct Rig {
    /// The `MeshId` the rig was built for; an instance that now draws another
    /// mesh is not this rig's any more.
    pub(crate) asset: AssetRef,
    pub(crate) key: AssetRef,
    referent: Ref,
    source: Arc<rbx_mesh::SkinnedMesh>,
    /// Per global bone, `None` for one no sibling part is named after.
    bones: Vec<Option<Bound>>,
    /// The fit scale `S` between native and part space.
    scale: Vec3,
    /// The native-space matrices last applied; identity at rest.
    applied: Vec<Mat4>,
}

impl Rig {
    /// The rig for `entry` drawing `mesh`, or `None` when the mesh carries no
    /// skin, the part is not a `MeshPart` in a `Model`, or no bone names a
    /// part there.
    pub(super) fn of(
        entry: &Entry,
        mesh: &Arc<rbx_mesh::SkinnedMesh>,
        poses: &Poses,
    ) -> Option<Rig> {
        let skin = mesh.skin.as_ref()?;
        let Fit::Part { .. } = entry.fit else {
            return None;
        };
        let names = poses.models.get(&entry.model?)?;
        let mesh_rest = *poses.rest.get(&entry.referent)?;
        let scale = fit_scale(&entry.fit, mesh);
        let to_part = Mat4::from_scale(scale);

        let bones: Vec<Option<Bound>> = skin
            .bones
            .iter()
            .map(|bone| {
                let part = *names.get(&bone.name)?;
                let part_rest = *poses.rest.get(&part)?;
                let bind = to_part * bone_matrix(bone) * to_part.inverse();
                // The bind position scaled into part space, its rotation kept.
                let offset = part_rest.inverse() * mesh_rest * bind;
                Some(Bound {
                    part,
                    tail: offset * bind.inverse(),
                })
            })
            .collect();
        if bones.iter().all(Option::is_none) {
            return None;
        }
        Some(Rig {
            asset: entry.mesh.clone(),
            key: key(entry.referent),
            referent: entry.referent,
            source: mesh.clone(),
            applied: vec![Mat4::IDENTITY; bones.len()],
            bones,
            scale,
        })
    }

    /// The native-space skinning matrix of every bone for the pose in
    /// `poses`.
    fn palette(&self, poses: &Poses) -> Vec<Mat4> {
        let Some(mesh) = poses.current(self.referent) else {
            return vec![Mat4::IDENTITY; self.bones.len()];
        };
        let mesh_inverse = mesh.inverse();
        let to_part = Mat4::from_scale(self.scale);
        let to_native = to_part.inverse();
        self.bones
            .iter()
            .map(|bone| {
                let Some(bone) = bone else {
                    return Mat4::IDENTITY;
                };
                match poses.current(bone.part) {
                    Some(part) => to_native * mesh_inverse * part * bone.tail * to_part,
                    None => Mat4::IDENTITY,
                }
            })
            .collect()
    }

    /// The deformed mesh for the pose in `poses`, or `None` when it is the
    /// one last applied.
    pub(crate) fn pose(&mut self, poses: &Poses) -> Option<Arc<rbx_mesh::SkinnedMesh>> {
        let palette = self.palette(poses);
        let same = palette
            .iter()
            .zip(&self.applied)
            .all(|(a, b)| a.abs_diff_eq(*b, EPSILON));
        if same {
            return None;
        }
        // Back at rest is the source itself, bit for bit.
        let at_rest = palette
            .iter()
            .all(|matrix| matrix.abs_diff_eq(Mat4::IDENTITY, EPSILON));
        let posed = match at_rest {
            true => self.source.clone(),
            false => Arc::new(deform(&self.source, &palette).into()),
        };
        self.applied = palette;
        Some(posed)
    }
}

/// `S`: how [`Fit::transform`] scales native geometry into part space.
fn fit_scale(fit: &Fit, mesh: &rbx_mesh::Mesh) -> Vec3 {
    // The scale column lengths of an otherwise rigid transform.
    let model = fit.transform(mesh);
    Vec3::new(
        model.x_axis.truncate().length(),
        model.y_axis.truncate().length(),
        model.z_axis.truncate().length(),
    )
    .max(Vec3::splat(f32::EPSILON))
}

/// A bone's stored CFrame as a matrix: rotation row-major, then position.
pub(crate) fn bone_matrix(bone: &rbx_mesh::Bone) -> Mat4 {
    let r = bone.rotation;
    let p = bone.position;
    Mat4::from_cols(
        glam::Vec4::new(r[0], r[3], r[6], 0.0),
        glam::Vec4::new(r[1], r[4], r[7], 0.0),
        glam::Vec4::new(r[2], r[5], r[8], 0.0),
        glam::Vec4::new(p[0], p[1], p[2], 1.0),
    )
}

/// Linear-blend skinning of `source` through `palette` (one matrix per global
/// bone). Positions are blended; normals are blended through each matrix's
/// inverse-transpose and keep their stored length. A vertex with no
/// influence, or no skin entry at all, is left where it is.
pub(crate) fn deform(source: &rbx_mesh::SkinnedMesh, palette: &[Mat4]) -> rbx_mesh::Mesh {
    let mut mesh = source.mesh.clone();
    let Some(skin) = &source.skin else {
        return mesh;
    };
    let normals: Vec<Mat3> = palette
        .iter()
        .map(|m| Mat3::from_mat4(*m).inverse().transpose())
        .collect();
    for (vertex, influences) in mesh.vertices.iter_mut().zip(&skin.influences) {
        let total: f32 = influences.iter().map(|(_, w)| w).sum();
        if total <= 0.0 {
            continue;
        }
        let position = Vec3::from(vertex.position);
        let normal = Vec3::from(vertex.normal);
        let (mut p, mut n) = (Vec3::ZERO, Vec3::ZERO);
        for (bone, weight) in influences.iter() {
            let bone = bone as usize;
            let (Some(matrix), Some(normal_matrix)) = (palette.get(bone), normals.get(bone)) else {
                // A bone past the palette is a bone at rest.
                p += position * weight;
                n += normal * weight;
                continue;
            };
            p += matrix.transform_point3(position) * weight;
            n += *normal_matrix * normal * weight;
        }
        vertex.position = (p / total).to_array();
        let length = normal.length();
        vertex.normal = match n.try_normalize() {
            Some(unit) => (unit * length).to_array(),
            None => vertex.normal,
        };
    }
    mesh
}

#[cfg(test)]
#[path = "skinned/tests.rs"]
mod tests;
