//! Walks the operation tree inside a legacy union's downloaded asset.
//!
//! `PartOperationAsset.ChildData` is itself a nested `.rbxm`; every
//! `UnionOperation`/`NegateOperation` found while walking it stores ITS OWN
//! children the same way, one freshly-deserialized document per level, down
//! to the original `BasePart`s a builder combined before Studio baked the CSG
//! result. `MeshData` (the baked triangle mesh) is never read: it is opaque
//! and superseded by the geometry `super::csg` rebuilds from the tree here.
//!
//! Empirically (see the `#[ignore]`d download test), a node's own `CFrame`
//! property is not useful on its own when its parent is a `NegateOperation`:
//! it always carries a zero position with the *operation's own* rotation, a
//! redundant echo of the parent rather than a further local offset — `walk`'s
//! `echo` parameter skips composing it in that case. Everywhere else, placing
//! every part is standard CFrame-parenting composition of each operation
//! node's `CFrame` down from the union's own frame — every [`Leaf::cframe`]
//! is that composed result, in the union's local space so one tree serves
//! every instance.

use std::collections::BTreeMap;

use glam::{Mat4, Vec3};
use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::super::material::Catalog;
use super::super::shape::{self, Geometry};
use super::super::{assemble_part, cframe_matrix, Part};

const NEGATE_OPERATION: &str = "NegateOperation";
const CHILD_DATA_PROPERTY: &str = "ChildData";
// `ChildData` is a nested .rbxm, so it is never valid UTF-8 and always decodes
// to `Variant::Unknown` — but its wire type is still String (0x01): Roblox's
// BinaryString shares String's wire encoding. See
// `rbx_binary::chunks::prop::scalar::string_value`.
const STRING_TYPE_ID: u8 = 0x01;

/// One original `BasePart` of a union, in the union's own frame.
pub(super) struct Leaf {
    /// Whether the leaf itself is a `NegateOperation` with no children of its
    /// own — a shape to carve rather than to add.
    pub(super) negate: bool,
    pub(super) geometry: Geometry,
    /// Composed placement in the union's frame — see the module doc.
    pub(super) cframe: Mat4,
    /// The leaf's own properties (colour, material, transparency…), kept so a
    /// fallback box can be built through the normal part pipeline later.
    pub(super) properties: BTreeMap<String, Variant>,
}

impl Leaf {
    /// Unit shape to union frame: the same matrix `Part::transform` would be.
    pub(super) fn model(&self) -> Mat4 {
        self.cframe
            * Mat4::from_translation(self.geometry.offset)
            * Mat4::from_scale(self.geometry.size)
    }
}

/// One node of the operation tree, with the leaves already resolved to shapes.
pub(super) enum Node {
    Leaf(Leaf),
    /// A `UnionOperation`/`NegateOperation` combinator, pure tree structure:
    /// its own size/colour duplicate whichever leaf sits directly beneath it.
    Operation {
        negate: bool,
        children: Vec<Node>,
    },
}

impl Node {
    pub(super) fn is_negate(&self) -> bool {
        match self {
            Node::Leaf(leaf) => leaf.negate,
            Node::Operation { negate, .. } => *negate,
        }
    }

    pub(super) fn leaf_count(&self) -> usize {
        match self {
            Node::Leaf(_) => 1,
            Node::Operation { children, .. } => children.iter().map(Node::leaf_count).sum(),
        }
    }

    /// The additive-only reading of the tree, one `Part` per surviving leaf:
    /// what draws when the boolean itself cannot be computed. `negate` latches
    /// under any `NegateOperation` ancestor — a compound shape subtracted from
    /// another subtracts every one of its own pieces, not just the top one.
    pub(super) fn fallback_parts(
        &self,
        placement: Mat4,
        stand_in_for: Ref,
        database: &ReflectionDatabase,
        materials: &mut Catalog,
        out: &mut Vec<Part>,
    ) {
        match self {
            // Every recovered part stands in for the same union instance: that
            // Ref (rather than the leaf's own, which is only unique within its
            // transient nested DOM) is what lets `Scene::resolve_unions` find
            // and suppress the union's fallback box.
            Node::Leaf(leaf) if !leaf.negate => out.push(assemble_part(
                &leaf.properties,
                database,
                materials,
                leaf.geometry,
                placement * leaf.cframe,
                stand_in_for,
            )),
            Node::Leaf(_) => {}
            Node::Operation { negate, children } => {
                if *negate {
                    return;
                }
                for child in children {
                    child.fallback_parts(placement, stand_in_for, database, materials, out);
                }
            }
        }
    }
}

/// Decodes one union's downloaded asset bytes into its operation tree, rooted
/// at an implicit additive node holding the asset's top-level children.
///
/// `None` means the bytes did not parse as the expected
/// `PartOperationAsset`/`ChildData` chain at all — the caller keeps the box in
/// that case. A tree with no additive leaf (all negated) is still a tree: the
/// caller hides the box regardless, an empty union being a better guess than
/// a solid one.
pub(super) fn parse(bytes: &[u8], database: &ReflectionDatabase) -> Option<Node> {
    let asset = rbx_binary::deserialize(bytes).ok()?;
    let root = *asset.root_refs().first()?;
    let raw = child_data(&asset, root)?;
    let operations = rbx_binary::deserialize(raw).ok()?;
    let children = operations
        .root_refs()
        .iter()
        .filter_map(|&child| walk(&operations, child, Mat4::IDENTITY, false, database))
        .collect();
    Some(Node::Operation {
        negate: false,
        children,
    })
}

/// `ChildData`'s raw bytes, or `None` when the property is absent, empty, or
/// (surprisingly) valid UTF-8 — the last of which can only mean it holds no
/// nested document at all.
fn child_data(dom: &WeakDom, referent: Ref) -> Option<&[u8]> {
    match dom.get(referent)?.properties().get(CHILD_DATA_PROPERTY) {
        Some(Variant::Unknown { type_id, raw })
            if *type_id == STRING_TYPE_ID && !raw.is_empty() =>
        {
            Some(raw)
        }
        _ => None,
    }
}

/// `echo`: whether `referent`'s immediate parent is a `NegateOperation`. A
/// `NegateOperation` is restricted by the engine to exactly one child, and
/// that child's own `CFrame` is not a further local offset but a verbatim
/// copy of the `NegateOperation`'s own rotation with position zeroed out
/// (empirically — see the module doc); composing it in as if it were a real
/// relative transform silently double-applies that rotation. When `echo` is
/// set, `referent`'s own `CFrame` is ignored and `parent` (the
/// `NegateOperation`'s already-composed frame) is used as-is.
fn walk(
    dom: &WeakDom,
    referent: Ref,
    parent: Mat4,
    echo: bool,
    database: &ReflectionDatabase,
) -> Option<Node> {
    let instance = dom.get(referent)?;
    let properties = instance.properties();
    let Some(Variant::CFrame(own_cframe)) = properties.get("CFrame") else {
        return None;
    };
    let cframe = if echo {
        parent
    } else {
        parent * cframe_matrix(own_cframe)
    };
    let negate = instance.class() == NEGATE_OPERATION;

    if let Some(raw) = child_data(dom, referent) {
        let nested = rbx_binary::deserialize(raw).ok()?;
        let children = nested
            .root_refs()
            .iter()
            .filter_map(|&child| walk(&nested, child, cframe, negate, database))
            .collect();
        return Some(Node::Operation { negate, children });
    }

    let Some(&Variant::Vector3(size)) = properties.get("size") else {
        return None;
    };
    let size = Vec3::new(size.x, size.y, size.z);
    Some(Node::Leaf(Leaf {
        negate,
        geometry: shape::resolve(dom, database, instance, size),
        cframe,
        properties: properties.clone(),
    }))
}
