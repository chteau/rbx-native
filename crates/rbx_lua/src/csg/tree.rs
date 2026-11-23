//! The `ChildData2` tree an operation saves: the parts it was made from,
//! in its own space, so it can be separated again.

use rbx_csg::glam::DMat4;
use rbx_csg::{solidmesh, Solid};
use rbx_dom::{CFrameData, Instance, Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::{
    carves, cframe, cframe_of, frame, shared, size_of, triangle_count, vector3, world_solid,
    Operation, STALE_GEOMETRY,
};

/// `source`'s instance copied into `into` under `parent` as `class`,
/// placed at `cframe`, children left behind.
fn copy_into(
    into: &mut WeakDom,
    source: &Instance,
    class: &str,
    parent: Option<Ref>,
    cframe: CFrameData,
) -> Ref {
    let referent = into.new_instance(class, source.name(), parent);
    if let Some(copy) = into.get_mut(referent) {
        *copy.properties_mut() = source.properties().clone();
        copy.properties_mut()
            .insert("CFrame".to_owned(), Variant::CFrame(cframe));
    }
    referent
}

/// The `ChildData2` document: every source part in the operation's own
/// space, a subtracted one inside a `NegateOperation` as Studio nests it.
pub(super) fn child_data(
    dom: &WeakDom,
    db: &ReflectionDatabase,
    operation: Operation,
    parts: &[Ref],
    inverse: DMat4,
) -> Option<Vec<u8>> {
    let mut nested = WeakDom::new();
    for (i, &part) in parts.iter().enumerate() {
        let instance = dom.get(part)?;
        let local = cframe(inverse * frame(&cframe_of(db, instance)));
        if i > 0 && carves(dom, operation, part) && instance.class() != "NegateOperation" {
            let solid = world_solid(dom, db, part).ok()?;
            negated(&mut nested, db, instance, local, &solid)?;
        } else {
            copy_into(&mut nested, instance, instance.class(), None, local);
        }
    }
    rbx_binary::serialize(&nested).ok()
}

/// A `NegateOperation` of `part` (whose world solid is `solid`) made in
/// `into` at `at`, holding the part as Studio nests it, at the operation's
/// own frame, and the part's mesh as its `SolidMeshHolder`.
pub(super) fn negated(
    into: &mut WeakDom,
    db: &ReflectionDatabase,
    part: &Instance,
    at: CFrameData,
    solid: &Solid,
) -> Option<Ref> {
    const CLASS: &str = "NegateOperation";
    let negate = copy_into(into, part, CLASS, None, at);
    let size = Variant::Vector3(vector3(size_of(db, part)));
    let world = frame(&cframe_of(db, part));
    let mut inner = WeakDom::new();
    copy_into(&mut inner, part, part.class(), None, cframe(DMat4::IDENTITY));
    let raw = rbx_binary::serialize(&inner).ok()?;
    let props = into.get_mut(negate)?.properties_mut();
    props.retain(|k, _| {
        !STALE_GEOMETRY.contains(&k.as_str())
            && db
                .resolve_property(CLASS, db.canonical_name(CLASS, k))
                .is_some()
    });
    props.insert("InitialSize".to_owned(), size);
    props.insert("ChildData2".to_owned(), shared(raw));
    let local = solid.transformed(world.inverse());
    props.insert(
        "SolidMeshHolder".to_owned(),
        shared(solidmesh::encode(&local)),
    );
    props.insert(
        "TriangleCount".to_owned(),
        Variant::Int32(triangle_count(&local)),
    );
    Some(negate)
}
