//! The `ChildData2` tree an operation saves: the parts it was made from,
//! in its own space, so it can be separated again.

use rbx_csg::glam::DMat4;
use rbx_dom::{CFrameData, Instance, Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::{
    carves, cframe, cframe_of, frame, shared, size_of, vector3, Operation, NEGATED_TAG,
    STALE_GEOMETRY,
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
/// space, a subtracted or negated one inside a `NegateOperation` as Studio
/// nests it (a `MeshPart` one tagged instead, see [`negated_mesh`]).
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
        let carving = i > 0 && carves(dom, operation, part);
        if carving && db.is_subclass_of(instance.class(), "MeshPart") {
            negated_mesh(&mut nested, instance, local)?;
        } else if carving && instance.class() != "NegateOperation" {
            negated(&mut nested, db, instance, local)?;
        } else {
            copy_into(&mut nested, instance, instance.class(), None, local);
        }
    }
    rbx_binary::serialize(&nested).ok()
}

/// A carving `MeshPart` as Studio's Separate reads it back: the part itself,
/// tagged `rbxNegated` like Studio's Negate leaves it (at `Transparency` 0.5
/// when it was not already). Studio 0.748 refuses to separate a union whose
/// tree wraps a `MeshPart` in a `NegateOperation` ("Part type not supported
/// in CSG").
fn negated_mesh(into: &mut WeakDom, part: &Instance, at: CFrameData) -> Option<Ref> {
    let copy = copy_into(into, part, part.class(), None, at);
    let mut tags = rbx_dom::attributes::tags(part.properties().get("Tags"));
    let props = into.get_mut(copy)?.properties_mut();
    if !tags.contains(&NEGATED_TAG) {
        tags.push(NEGATED_TAG);
        props.insert("Transparency".to_owned(), Variant::Float32(0.5));
    }
    props.insert(
        "Tags".to_owned(),
        Variant::String(rbx_dom::attributes::encode_tags(&tags)),
    );
    Some(copy)
}

/// A `NegateOperation` of `part` made in `into` at `at`, the way Studio
/// wraps a subtracted or negated part in a union's tree: "NegativePart",
/// white plastic at `Transparency` 0.1, the part's size, no mesh of its
/// own, and the part itself (untagged, at the wrapper's frame) in its
/// `ChildData2`.
pub(super) fn negated(
    into: &mut WeakDom,
    db: &ReflectionDatabase,
    part: &Instance,
    at: CFrameData,
) -> Option<Ref> {
    const CLASS: &str = "NegateOperation";
    /// `Enum.Material.Plastic`.
    const PLASTIC: u32 = 256;
    let negate = copy_into(into, part, CLASS, None, at);
    let size = Variant::Vector3(vector3(size_of(db, part)));
    let mut inner = WeakDom::new();
    let copy = copy_into(
        &mut inner,
        part,
        part.class(),
        None,
        cframe(DMat4::IDENTITY),
    );
    let tags: Vec<&str> = rbx_dom::attributes::tags(part.properties().get("Tags"))
        .into_iter()
        .filter(|&t| t != NEGATED_TAG)
        .collect();
    inner.get_mut(copy)?.properties_mut().insert(
        "Tags".to_owned(),
        Variant::String(rbx_dom::attributes::encode_tags(&tags)),
    );
    let raw = rbx_binary::serialize(&inner).ok()?;
    let wrapper = into.get_mut(negate)?;
    wrapper.set_name("NegativePart");
    let props = wrapper.properties_mut();
    props.retain(|k, _| {
        !STALE_GEOMETRY.contains(&k.as_str())
            && k != "Tags"
            && db
                .resolve_property(CLASS, db.canonical_name(CLASS, k))
                .is_some()
    });
    props.insert("InitialSize".to_owned(), size);
    props.insert("ChildData2".to_owned(), shared(raw));
    props.insert(
        "Color3uint8".to_owned(),
        Variant::Color3uint8 {
            r: 255,
            g: 255,
            b: 255,
        },
    );
    props.insert("Material".to_owned(), Variant::Enum(PLASTIC));
    props.insert("Transparency".to_owned(), Variant::Float32(0.1));
    Some(negate)
}
