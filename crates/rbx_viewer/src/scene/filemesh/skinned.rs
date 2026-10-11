//! Skinned `MeshPart`s deformed live by the parts their bones are named
//! after.
//!
//! Roblox binds a mesh bone to the part (or `Bone`) of the same name in the
//! character, so a skinned R15 body part follows its own part and bends at the
//! joints into its neighbours. Here a bone binds to a part **among the
//! `MeshPart`'s siblings in its `Model`**, matched by `Name`; a bone that
//! names none stays at its bind pose.
//!
//! A rig binds wherever its mesh part comes from — the place as opened, a
//! patch inserting it later (the auto-rigger swaps its whole `Model` on every
//! edit), or a mesh streaming in after either — and binds again whenever its
//! `MeshId`, `Size` or model changes, or a part in that model arrives,
//! leaves or is renamed (see [`bind`]).
//!
//! # Convention
//!
//! A bone's stored CFrame `bind_b` is its bind pose in the mesh's native
//! space: relative to the part, unrotated, before the `Size`/`InitialSize`
//! fit scale `S` (see [`super::Fit::transform`]). The bone's joint frame is
//! `J_b = part_b.CFrame · K_b`, where `K_b` is that part's fixed offset to its
//! joint, taken from the parts' rest frames (see [`Poses`]) so that
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

/// The derived mesh `referent` draws through while skinned with `asset`.
///
/// Per mesh as well as per part: a rig rebuilt for another `MeshId` gets a
/// key (and so a GPU batch) of its own, so a re-upload in place only ever
/// meets the vertex count it was made for.
pub(crate) fn key(referent: Ref, asset: &AssetRef) -> AssetRef {
    AssetRef::Thumb(format!("skinned-mesh/{referent:?}/{asset:?}"))
}

/// Matrices closer than this to the last ones applied leave the mesh alone.
const EPSILON: f32 = 1e-6;

/// Whether `model` is somewhere a skinned mesh's bones look for parts: a
/// `Model`, but not the world itself.
fn is_rig_model(dom: &WeakDom, database: &ReflectionDatabase, model: Ref) -> bool {
    dom.get(model).is_some_and(|instance| {
        let class = instance.class();
        database.is_subclass_of(class, "Model")
            && !database.is_subclass_of(class, "WorldRoot")
            && class != "Workspace"
    })
}

/// The `Model` a `MeshPart`'s bones look for their parts in: its parent,
/// when that is a `Model` but not the world itself.
pub(super) fn model_of(dom: &WeakDom, database: &ReflectionDatabase, referent: Ref) -> Option<Ref> {
    let parent = dom.parent(referent)?;
    is_rig_model(dom, database, parent).then_some(parent)
}

fn cframe_of(dom: &WeakDom, referent: Ref) -> Option<Mat4> {
    match dom.get(referent)?.properties().get("CFrame") {
        Some(Variant::CFrame(cframe)) => Some(cframe_matrix(cframe)),
        _ => None,
    }
}

fn ref_of(dom: &WeakDom, referent: Ref, property: &str) -> Option<Ref> {
    match dom.get(referent)?.properties().get(property) {
        Some(Variant::Ref(target)) if dom.get(*target).is_some() => Some(*target),
        _ => None,
    }
}

/// The parts skinned meshes can bind to, with their rest and current frames.
///
/// A `Model` is watched from the first time a `MeshPart` in it is planned or
/// patched in, and its child parts are read again whenever one of them
/// arrives, leaves or is renamed (see [`Poses::note`],
/// [`Poses::forget_part`]); a part that only moves costs a lookup.
///
/// A part's rest frame is fixed when it is first read: from the model's
/// animated joints where it hangs off one (see [`joint_rests`]), so a place
/// saved mid-pose still rests where its joints say, otherwise its `CFrame`
/// at that moment — which is rest for a rig inserted the way the
/// auto-rigger inserts one.
#[derive(Default)]
pub(crate) struct Poses {
    /// Per watched model: its child parts by `Name`, the first of a name (in
    /// child order) winning.
    models: HashMap<Ref, HashMap<String, Ref>>,
    /// Every child part of a watched model: the model and its name.
    members: HashMap<Ref, (Ref, String)>,
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
            if !poses.watches(model) {
                poses.watch(dom, database, model);
            }
        }
        poses.dirty = false;
        poses
    }

    pub(crate) fn watches(&self, model: Ref) -> bool {
        self.models.contains_key(&model)
    }

    /// (Re-)reads `model`'s child parts, starting to watch it if need be or
    /// letting it go if it is no longer a model with parts. Answers whether
    /// a bone name could now find another part there than before — what
    /// makes the skinned meshes in it bind again.
    pub(crate) fn watch(
        &mut self,
        dom: &WeakDom,
        database: &ReflectionDatabase,
        model: Ref,
    ) -> bool {
        let before = self.models.remove(&model);
        let parts: Vec<(Ref, String, Mat4)> = match is_rig_model(dom, database, model) {
            false => Vec::new(),
            true => dom
                .get(model)
                .map(|instance| instance.children())
                .unwrap_or_default()
                .iter()
                .filter_map(|&child| {
                    let instance = dom.get(child)?;
                    database
                        .is_subclass_of(instance.class(), "BasePart")
                        .then_some(())?;
                    Some((child, instance.name().to_owned(), cframe_of(dom, child)?))
                })
                .collect(),
        };
        let leaving: Vec<Ref> = self
            .members
            .iter()
            .filter(|(part, (owner, _))| {
                *owner == model && !parts.iter().any(|(kept, ..)| kept == *part)
            })
            .map(|(part, _)| *part)
            .collect();
        for part in leaving {
            self.drop_member(part);
        }
        if parts.is_empty() {
            return before.is_some_and(|names| !names.is_empty());
        }

        let members: Vec<Ref> = parts.iter().map(|(part, ..)| *part).collect();
        let rests = joint_rests(dom, database, model, &members, &self.rest);
        let mut names = HashMap::new();
        for (part, name, cframe) in parts {
            names.entry(name.clone()).or_insert(part);
            self.members.insert(part, (model, name));
            if self
                .current
                .insert(part, cframe)
                .is_some_and(|held| held != cframe)
            {
                self.dirty = true;
            }
            let rest = rests.get(&part).copied().unwrap_or(cframe);
            self.rest.entry(part).or_insert(rest);
        }
        let changed = before.as_ref() != Some(&names);
        self.models.insert(model, names);
        changed
    }

    /// Keeps `referent`'s membership of the watched models in step with the
    /// DOM — it may have just arrived in one, left one or been renamed —
    /// answering with every model whose bindings that changed. A part whose
    /// model and name stand as they were costs two lookups.
    pub(crate) fn note(
        &mut self,
        dom: &WeakDom,
        database: &ReflectionDatabase,
        referent: Ref,
    ) -> Vec<Ref> {
        let before = self
            .members
            .get(&referent)
            .map(|(model, name)| (*model, name.clone()));
        let now = dom.get(referent).and_then(|instance| {
            let parent = dom.parent(referent)?;
            (self.watches(parent) && database.is_subclass_of(instance.class(), "BasePart"))
                .then(|| (parent, instance.name().to_owned()))
        });
        if before == now {
            return Vec::new();
        }
        let mut models: Vec<Ref> = before
            .into_iter()
            .chain(now)
            .map(|(model, _)| model)
            .collect();
        models.dedup();
        models.retain(|&model| self.watch(dom, database, model));
        models
    }

    /// `referent` is gone from the DOM: it binds nothing any more. Answers
    /// with its model when a bone there was bound to it.
    pub(crate) fn forget_part(&mut self, referent: Ref) -> Option<Ref> {
        let (model, name) = self.drop_member(referent)?;
        let names = self.models.get_mut(&model)?;
        if names.get(&name) != Some(&referent) {
            return None;
        }
        names.remove(&name);
        // Another part of the same name takes the bone over; without the DOM
        // to say which comes first among the children, the oldest referent.
        let next = self
            .members
            .iter()
            .filter(|(_, (owner, other))| *owner == model && *other == name)
            .map(|(part, _)| *part)
            .min();
        if let Some(next) = next {
            names.insert(name, next);
        }
        if names.is_empty() {
            self.models.remove(&model);
        }
        Some(model)
    }

    fn drop_member(&mut self, part: Ref) -> Option<(Ref, String)> {
        self.rest.remove(&part);
        self.current.remove(&part);
        self.members.remove(&part)
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

/// Rest frames for `members` of `model` read off its animated joints: a
/// `Motor6D` (or `Motor`) rests at `Part1 = Part0 · C0 · C1⁻¹`, an
/// `AnimationConstraint` at `Part1 = Part0 · A0 · A1⁻¹` (its attachments'
/// frames), whatever pose the place was saved in. Walked from every member
/// no joint drives, which rests where it stands (or where `known` already
/// has it); a member the walk never reaches is left out, to rest where it
/// stands. A part keeps a rest it already has.
fn joint_rests(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    model: Ref,
    members: &[Ref],
    known: &HashMap<Ref, Mat4>,
) -> HashMap<Ref, Mat4> {
    let member = |part: Ref| members.contains(&part);
    let mut joints: Vec<(Ref, Ref, Mat4)> = Vec::new();
    for joint in crate::scene::descendants_of(dom, model) {
        let Some(instance) = dom.get(joint) else {
            continue;
        };
        let class = instance.class();
        let edge = if database.is_subclass_of(class, "Motor") {
            (|| {
                let part0 = ref_of(dom, joint, "Part0")?;
                let part1 = ref_of(dom, joint, "Part1")?;
                let c0 = match instance.properties().get("C0") {
                    Some(Variant::CFrame(c)) => cframe_matrix(c),
                    _ => Mat4::IDENTITY,
                };
                let c1 = match instance.properties().get("C1") {
                    Some(Variant::CFrame(c)) => cframe_matrix(c),
                    _ => Mat4::IDENTITY,
                };
                Some((part0, part1, c0 * c1.inverse()))
            })()
        } else if class == "AnimationConstraint" {
            (|| {
                let a0 = ref_of(dom, joint, "Attachment0")?;
                let a1 = ref_of(dom, joint, "Attachment1")?;
                let part0 = dom.parent(a0)?;
                let part1 = dom.parent(a1)?;
                let frame0 = cframe_of(dom, a0).unwrap_or(Mat4::IDENTITY);
                let frame1 = cframe_of(dom, a1).unwrap_or(Mat4::IDENTITY);
                Some((part0, part1, frame0 * frame1.inverse()))
            })()
        } else {
            None
        };
        if let Some((part0, part1, offset)) = edge {
            if member(part0) && member(part1) && part0 != part1 {
                joints.push((part0, part1, offset));
            }
        }
    }

    let mut rests = HashMap::new();
    for &part in members {
        let driven = joints.iter().any(|&(_, part1, _)| part1 == part);
        let rest = known.get(&part).copied().or_else(|| match driven {
            true => None,
            false => cframe_of(dom, part),
        });
        if let Some(rest) = rest {
            rests.insert(part, rest);
        }
    }
    // One joint further from the roots each round.
    for _ in 0..joints.len() {
        let mut grew = false;
        for &(part0, part1, offset) in &joints {
            if rests.contains_key(&part1) {
                continue;
            }
            if let Some(&rest0) = rests.get(&part0) {
                rests.insert(part1, rest0 * offset);
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    rests
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
    /// The model its bones were bound in.
    model: Ref,
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
        let model = entry.model?;
        let names = poses.models.get(&model)?;
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
            key: key(entry.referent, &entry.mesh),
            referent: entry.referent,
            model,
            source: mesh.clone(),
            applied: vec![Mat4::IDENTITY; bones.len()],
            bones,
            scale,
        })
    }

    /// Whether this rig is still the one `entry` drawing `mesh` would get:
    /// the same mesh, in the same model, at the same fit scale.
    fn fits(&self, entry: &Entry, mesh: &Arc<rbx_mesh::SkinnedMesh>) -> bool {
        Arc::ptr_eq(&self.source, mesh)
            && self.asset == entry.mesh
            && Some(self.model) == entry.model
            && self.scale.abs_diff_eq(fit_scale(&entry.fit, mesh), 1e-6)
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

/// What [`bind`] did.
#[derive(Default)]
pub(crate) struct Binding {
    /// Models first watched for this entry, whose other mesh parts may now
    /// bind too.
    pub(crate) models: Vec<Ref>,
    /// The derived mesh (re)built, whose GPU copy is now stale.
    pub(crate) key: Option<AssetRef>,
}

/// Brings the rig of `entry`'s part in step with the entry as just
/// re-planned off the DOM: built where its skinned mesh has landed and its
/// bones find parts, rebuilt when anything it was built from changed (or
/// `stale` says the parts of its model did), dropped otherwise. The new rig
/// starts out in the current pose.
pub(in crate::scene) fn bind(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    poses: &mut Poses,
    resolved: &mut super::Resolved,
    entry: &Entry,
    stale: bool,
) -> Binding {
    let mut bound = Binding::default();
    if let Some(model) = entry.model {
        if !poses.watches(model) && poses.watch(dom, database, model) {
            bound.models.push(model);
        }
    }
    let referent = entry.referent;
    let Some(mesh) = resolved.meshes.get(&entry.mesh).cloned() else {
        resolved.drop_rig(referent);
        return bound;
    };
    let current = resolved.rigs.get(&referent);
    if !stale && current.is_some_and(|rig| rig.fits(entry, &mesh)) {
        return bound;
    }
    resolved.drop_rig(referent);
    let Some(mut rig) = Rig::of(entry, &mesh, poses) else {
        return bound;
    };
    let posed = rig.pose(poses).unwrap_or(mesh);
    resolved.meshes.insert(rig.key.clone(), posed);
    bound.key = Some(rig.key.clone());
    resolved.rigs.insert(referent, rig);
    bound
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
