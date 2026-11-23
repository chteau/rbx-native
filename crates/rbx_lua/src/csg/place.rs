//! Studio's Union, Intersect, Negate and Separate on a place: the parts
//! become an operation and leave, or come back out of one.

use rbx_csg::glam::{DMat3, DMat4, DVec3};
use rbx_csg::solidmesh;
use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::tree::negated;
use super::{
    build, cframe, cframe_of, combine, frame, is_negate, local_mesh, read, set, shared,
    shared_bytes, size_of, triangle_count, vec3, vector3, world_solid, Flavor, Operation,
};

/// The parts among `objects` an operation is made of, the first one that
/// adds first: what Studio's Union, Intersect and Negate act on.
fn parts_of(dom: &WeakDom, db: &ReflectionDatabase, objects: &[Ref]) -> Vec<Ref> {
    let mut parts: Vec<Ref> = objects
        .iter()
        .copied()
        .filter(|&r| {
            dom.get(r)
                .is_some_and(|i| db.is_subclass_of(i.class(), "BasePart") && i.class() != "Terrain")
        })
        .collect();
    if let Some(first) = parts.iter().position(|&p| !is_negate(dom, p)) {
        let base = parts.remove(first);
        parts.insert(0, base);
    }
    parts
}

/// Studio's Union and Intersect (`plugin:Union`, `plugin:Intersect`, the
/// Model tab): the parts among `objects` become one operation, parented
/// where the first part was, and leave the place. `NegateOperation`s in a
/// union are carved out.
pub fn operate_in_place(
    dom: &mut WeakDom,
    db: &ReflectionDatabase,
    operation: Operation,
    objects: &[Ref],
) -> Result<Ref, String> {
    let parts = parts_of(dom, db, objects);
    let (&base, others) = parts.split_first().ok_or("No parts were selected")?;
    if is_negate(dom, base) {
        return Err("Cannot union only negated parts".to_owned());
    }
    let solid = combine(dom, db, operation, base, others)?;
    let parent = dom.parent(base);
    let made = build(dom, db, operation, Flavor::Part, base, others, solid, false);
    let referent = made.first().ok_or("The operation made nothing")?.referent;
    dom.set_parent(referent, parent);
    for part in parts {
        dom.remove(part);
    }
    Ok(referent)
}

/// Studio's Negate: each part becomes a `NegateOperation` in its place, and
/// a `NegateOperation` becomes its part again.
pub fn negate_in_place(dom: &mut WeakDom, db: &ReflectionDatabase, objects: &[Ref]) -> Vec<Ref> {
    let mut out = Vec::new();
    for part in parts_of(dom, db, objects) {
        if is_negate(dom, part) {
            out.extend(separate_in_place(dom, db, &[part]));
            continue;
        }
        let Ok(solid) = world_solid(dom, db, part) else {
            continue;
        };
        let Some(instance) = dom.get(part).cloned() else {
            continue;
        };
        let parent = dom.parent(part);
        let mut staged = WeakDom::new();
        let Some(negate) = negated(&mut staged, db, &instance, cframe_of(db, &instance), &solid)
        else {
            continue;
        };
        let Some(made) = staged.get(negate) else {
            continue;
        };
        let referent = dom.new_instance(made.class(), made.name(), parent);
        for (key, value) in made.properties() {
            let _ = dom.set_property(referent, key, value.clone());
        }
        dom.remove(part);
        out.push(referent);
    }
    out
}

/// Studio's Separate: each operation among `objects` gives back the parts
/// (and `NegateOperation`s) its `ChildData2` holds, placed in the world as
/// the operation stands now, and leaves the place.
pub fn separate_in_place(dom: &mut WeakDom, db: &ReflectionDatabase, objects: &[Ref]) -> Vec<Ref> {
    let mut out = Vec::new();
    for &op in objects {
        let Some(instance) = dom.get(op) else {
            continue;
        };
        if !db.is_subclass_of(instance.class(), "PartOperation") {
            continue;
        }
        let Some(nested) =
            shared_bytes(instance, "ChildData2").and_then(|raw| rbx_binary::deserialize(raw).ok())
        else {
            continue;
        };
        let size = size_of(db, instance);
        let initial = match read(db, instance, "InitialSize") {
            Some(Variant::Vector3(v)) => vec3(*v),
            _ => size,
        };
        let ratio = size / initial.max(DVec3::splat(1e-6));
        let at = frame(&cframe_of(db, instance));
        let echo = instance.class() == "NegateOperation";
        let parent = dom.parent(op);
        for &root in nested.root_refs() {
            let Some(child) = nested.get(root) else {
                continue;
            };
            let local = frame(&cframe_of(db, child));
            let placed = if echo {
                at
            } else {
                let mut local = local;
                local.w_axis = (local.w_axis.truncate() * ratio).extend(1.0);
                at * local
            };
            let referent = dom.new_instance(child.class(), child.name(), parent);
            for (key, value) in child.properties() {
                let _ = dom.set_property(referent, key, value.clone());
            }
            set(dom, db, referent, "CFrame", Variant::CFrame(cframe(placed)));
            if ratio != DVec3::ONE {
                let rotation = DMat3::from_mat4(local);
                let scale = (rotation.transpose() * ratio).abs();
                let size = size_of(db, child) * scale;
                set(dom, db, referent, "Size", Variant::Vector3(vector3(size)));
            }
            // Studio gives a subtracted part back as itself, not as the
            // `NegateOperation` the tree keeps it in.
            if child.class() == "NegateOperation" && shared_bytes(child, "ChildData2").is_some() {
                out.extend(separate_in_place(dom, db, &[referent]));
                continue;
            }
            rebake(dom, db, referent);
            out.push(referent);
        }
        dom.remove(op);
    }
    out
}

/// Carves `op` again from its `ChildData2` when it has no mesh of its own,
/// the way Studio leaves the operations nested in a union's tree: one
/// brought back by Separate would otherwise draw as nothing in Studio,
/// which never rebuilds a missing `SolidMeshHolder` on load.
fn rebake(dom: &mut WeakDom, db: &ReflectionDatabase, op: Ref) {
    let Some(instance) = dom.get(op) else {
        return;
    };
    if !db.is_subclass_of(instance.class(), "PartOperation")
        || local_mesh(instance).is_some()
        || shared_bytes(instance, "ChildData2").is_none()
    {
        return;
    }
    let operation = match instance.class() {
        "IntersectOperation" => Operation::Intersect,
        _ => Operation::Union,
    };
    let size = size_of(db, instance);
    let initial = match read(db, instance, "InitialSize") {
        Some(Variant::Vector3(v)) => vec3(*v),
        _ => size,
    };
    let placement = frame(&cframe_of(db, instance))
        * DMat4::from_scale(size / initial.max(DVec3::splat(1e-6)));
    let mut scratch = WeakDom::new();
    let copy = scratch.new_instance(instance.class(), instance.name(), None);
    if let Some(copied) = scratch.get_mut(copy) {
        *copied.properties_mut() = instance.properties().clone();
    }
    let pieces = separate_in_place(&mut scratch, db, &[copy]);
    let parts = parts_of(&scratch, db, &pieces);
    let Some((&base, others)) = parts.split_first() else {
        return;
    };
    let Ok(solid) = combine(&scratch, db, operation, base, others) else {
        return;
    };
    let local = solid.transformed(placement.inverse());
    let Some(instance) = dom.get_mut(op) else {
        return;
    };
    let props = instance.properties_mut();
    props.insert(
        "SolidMeshHolder".to_owned(),
        shared(solidmesh::encode(&local)),
    );
    props.insert(
        "TriangleCount".to_owned(),
        Variant::Int32(triangle_count(&local)),
    );
}
