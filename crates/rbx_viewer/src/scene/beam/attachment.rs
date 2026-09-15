//! Resolves a `Beam`'s `Attachment0`/`Attachment1` — Refs to `Attachment`
//! instances anywhere in the DOM, not necessarily nearby — into a world
//! CFrame: the parent `BasePart`'s own `CFrame` composed with the
//! attachment's own, which is relative to it. Same composition
//! `lighting::local` uses for a light hung off an `Attachment`, just walked
//! from the attachment side since a `Beam` only ever holds the Ref.

use std::collections::HashMap;

use glam::Mat4;
use rbx_dom::{Instance, Ref, Variant, WeakDom};

use crate::scene::{cframe_matrix, descendants};

/// Referent → parent referent, built once per [`super::plan`] call since
/// `WeakDom` keeps no back-pointer of its own (the identical need is met the
/// same way in `scene::particles::emitter`).
///
/// `pub(crate)`, not `pub(super)`: `scene::trail` resolves its own
/// `Attachment0`/`Attachment1` the identical way and reuses this rather than
/// rebuilding the same parent walk (see `scene::beam`'s re-export).
pub(crate) struct ParentMap(HashMap<Ref, Ref>);

impl ParentMap {
    pub(crate) fn build(dom: &WeakDom) -> Self {
        let mut parents = HashMap::new();
        for referent in descendants(dom) {
            if let Some(instance) = dom.get(referent) {
                for &child in instance.children() {
                    parents.insert(child, referent);
                }
            }
        }
        ParentMap(parents)
    }

    fn get(&self, referent: Ref) -> Option<Ref> {
        self.0.get(&referent).copied()
    }
}

/// World CFrame of the `Attachment` instance `referent` points at, or `None`
/// if the reference is dangling, either instance carries no `CFrame`, or the
/// attachment's parent was never recorded (no parent, i.e. a root instance) —
/// a beam that cannot be placed is simply not drawn (see [`super::plan`]).
pub(crate) fn world_cframe(dom: &WeakDom, parents: &ParentMap, referent: Ref) -> Option<Mat4> {
    let attachment = dom.get(referent)?;
    let local = frame_of(attachment)?;
    let parent = dom.get(parents.get(referent)?)?;
    let world = frame_of(parent)?;
    Some(world * local)
}

fn frame_of(instance: &Instance) -> Option<Mat4> {
    match instance.properties().get("CFrame")? {
        Variant::CFrame(cframe) => Some(cframe_matrix(cframe)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use glam::Vec3;
    use rbx_dom::{CFrameData, Vector3Data};

    use super::*;

    const IDENTITY_ROTATION: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    /// A quarter turn about +Y: local +X (column 0 of the row-major matrix,
    /// i.e. `[r[0], r[3], r[6]]`) becomes world -Z here.
    const YAW_90_ROTATION: [f32; 9] = [0.0, 0.0, 1.0, 0.0, 1.0, 0.0, -1.0, 0.0, 0.0];

    fn cframe(position: Vec3, rotation: [f32; 9]) -> Variant {
        Variant::CFrame(CFrameData {
            position: Vector3Data {
                x: position.x,
                y: position.y,
                z: position.z,
            },
            rotation,
        })
    }

    /// Builds `Part -> Attachment` with the given CFrames and returns
    /// (dom, part, attachment).
    fn fixture(part_cframe: Variant, attachment_cframe: Variant) -> (WeakDom, Ref) {
        let mut dom = WeakDom::new();
        let part_ref = Ref::new(1);
        let mut part = Instance::new(part_ref, "Part", "Part");
        part.properties_mut()
            .insert("CFrame".to_string(), part_cframe);
        dom.insert(part);
        dom.set_parent(part_ref, None);

        let attachment_ref = Ref::new(2);
        let mut attachment = Instance::new(attachment_ref, "Attachment", "Attachment");
        attachment
            .properties_mut()
            .insert("CFrame".to_string(), attachment_cframe);
        dom.insert(attachment);
        dom.set_parent(attachment_ref, Some(part_ref));

        (dom, attachment_ref)
    }

    #[test]
    fn composes_the_parents_cframe_with_the_attachments_own() {
        let (dom, attachment) = fixture(
            cframe(Vec3::new(10.0, 0.0, 0.0), IDENTITY_ROTATION),
            cframe(Vec3::new(0.0, 2.0, 0.0), IDENTITY_ROTATION),
        );
        let parents = ParentMap::build(&dom);
        let world = world_cframe(&dom, &parents, attachment).unwrap();
        assert!((world.w_axis.truncate() - Vec3::new(10.0, 2.0, 0.0)).length() < 1e-5);
    }

    #[test]
    fn the_parents_rotation_carries_the_attachments_local_offset() {
        // Part yawed 90°: its local +X (where the attachment sits) is world -Z.
        let (dom, attachment) = fixture(
            cframe(Vec3::ZERO, YAW_90_ROTATION),
            cframe(Vec3::new(5.0, 0.0, 0.0), IDENTITY_ROTATION),
        );
        let parents = ParentMap::build(&dom);
        let world = world_cframe(&dom, &parents, attachment).unwrap();
        assert!((world.w_axis.truncate() - Vec3::new(0.0, 0.0, -5.0)).length() < 1e-4);
    }

    #[test]
    fn a_dangling_reference_resolves_to_none() {
        let dom = WeakDom::new();
        let parents = ParentMap::build(&dom);
        assert!(world_cframe(&dom, &parents, Ref::new(99)).is_none());
    }

    #[test]
    fn an_attachment_with_no_recorded_parent_resolves_to_none() {
        let mut dom = WeakDom::new();
        let attachment_ref = Ref::new(1);
        let mut attachment = Instance::new(attachment_ref, "Attachment", "Attachment");
        attachment
            .properties_mut()
            .insert("CFrame".to_string(), cframe(Vec3::ZERO, IDENTITY_ROTATION));
        dom.insert(attachment);
        dom.set_parent(attachment_ref, None);

        let parents = ParentMap::build(&dom);
        assert!(world_cframe(&dom, &parents, attachment_ref).is_none());
    }
}
