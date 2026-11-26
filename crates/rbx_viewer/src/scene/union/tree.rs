//! Walks the operation tree inside a legacy union's downloaded asset.
//!
//! `PartOperationAsset.ChildData` is itself a nested `.rbxm`; every
//! `UnionOperation`/`NegateOperation` found while walking it stores ITS OWN
//! children the same way, one freshly-deserialized document per level, down
//! to the original `BasePart`s a builder combined before Studio baked the CSG
//! result. `MeshData` (the baked triangle mesh) is not read here: the
//! geometry `super::csg` rebuilds from the tree supersedes it, and
//! `super::baked` reads it only for a union whose tree is gone or will not
//! carve.
//!
//! Empirically (see the `#[ignore]`d download test), a node's own `CFrame`
//! property is not useful on its own when its parent is a `NegateOperation`:
//! it always carries a zero position with the *operation's own* rotation, a
//! redundant echo of the parent rather than a further local offset, `walk`'s
//! `echo` parameter skips composing it in that case. Everywhere else, placing
//! every part is standard CFrame-parenting composition of each operation
//! node's `CFrame` down from the union's own frame, every [`Leaf::cframe`]
//! is that composed result, in the union's local space so one tree serves
//! every instance.

use std::collections::{BTreeMap, HashMap};

use glam::{Mat4, Vec3};
use rbx_assets::AssetRef;
use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::textures::asset_uri;

use super::super::material::Catalog;
use super::super::shape::{self, Geometry};
use super::super::{assemble_part, cframe_matrix, Part, PartId};

const NEGATE_OPERATION: &str = "NegateOperation";
const ASSET_CLASS: &str = "PartOperationAsset";
const CHILD_DATA_PROPERTIES: [&str; 2] = ["ChildData", "ChildData2"];
// `ChildData` is a nested .rbxm, so it is never valid UTF-8 and always decodes
// to `Variant::Unknown`: but its wire type is still String (0x01): Roblox's
// BinaryString shares String's wire encoding. See
// `rbx_binary::chunks::prop::scalar::string_value`. `rbx_xml` reports a
// non-UTF-8 `SharedString` under the same id.
const STRING_TYPE_ID: u8 = 0x01;
/// `ChildData2` read from a binary file: a resolved `SharedString`.
const SHARED_STRING_TYPE_ID: u8 = 0x1c;

/// One original `BasePart` of a union, in the union's own frame.
pub(super) struct Leaf {
    /// Whether the leaf itself is a `NegateOperation` with no children of its
    /// own, a shape to carve rather than to add.
    pub(super) negate: bool,
    pub(super) geometry: Geometry,
    /// Composed placement in the union's frame, see the module doc.
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
    /// under any `NegateOperation` ancestor, a compound shape subtracted from
    /// another subtracts every one of its own pieces, not just the top one.
    ///
    /// Every piece stands in for the same union instance, that `Ref`, rather
    /// than the leaf's own, which is only unique within its transient nested
    /// DOM, and is told apart from its siblings by its position in this
    /// walk. That position depends on the tree and nothing else, and the tree
    /// is a function of the union's asset bytes alone, so reading the same
    /// asset again hands the same leaf the same [`PartId`] however the union
    /// has been moved, resized or recoloured since: the record an edit
    /// rewrites is always the record that leaf already had.
    pub(super) fn pieces(
        &self,
        placement: Mat4,
        stand_in_for: Ref,
        database: &ReflectionDatabase,
        materials: &mut Catalog,
    ) -> Vec<Part> {
        let mut out = Vec::new();
        self.append_pieces(placement, stand_in_for, database, materials, &mut out);
        out
    }

    fn append_pieces(
        &self,
        placement: Mat4,
        stand_in_for: Ref,
        database: &ReflectionDatabase,
        materials: &mut Catalog,
        out: &mut Vec<Part>,
    ) {
        match self {
            Node::Leaf(leaf) if !leaf.negate => {
                let id = PartId::piece(stand_in_for, out.len() as u32);
                out.push(assemble_part(
                    &leaf.properties,
                    database,
                    materials,
                    leaf.geometry,
                    placement * leaf.cframe,
                    id,
                ));
            }
            Node::Leaf(_) => {}
            Node::Operation { negate, children } => {
                if *negate {
                    return;
                }
                for child in children {
                    child.append_pieces(placement, stand_in_for, database, materials, out);
                }
            }
        }
    }
}

/// Decodes one union's operation tree, rooted at an implicit additive node
/// holding its top-level children. `bytes` is either a downloaded
/// `PartOperationAsset` (whose `ChildData` holds the children) or a union's
/// own inline `ChildData`/`ChildData2`, which *is* that nested document.
///
/// `assets` holds the bytes of the union assets operations deeper in the tree
/// point at by `AssetId` (see [`Parsed::missing`]); an empty entry is one that
/// could not be fetched, drawn as the box it stands for.
///
/// `None` means the bytes did not parse at all, the caller keeps the box in
/// that case. A tree with no additive leaf (all negated) is still a tree: the
/// caller hides the box regardless, an empty union being a better guess than
/// a solid one.
pub(super) fn parse(
    bytes: &[u8],
    database: &ReflectionDatabase,
    assets: &Assets,
) -> Option<Parsed> {
    parse_at_least(bytes, database, assets, 0.0)
}

/// [`parse`], every part's stored `size` raised to at least `min_size` on
/// each axis first, the smallest part the engine built when the union was
/// baked (see `super::legacy`).
pub(super) fn parse_at_least(
    bytes: &[u8],
    database: &ReflectionDatabase,
    assets: &Assets,
    min_size: f32,
) -> Option<Parsed> {
    let mut context = Context {
        database,
        assets,
        missing: Vec::new(),
        depth: 0,
        min_size,
        meshes: false,
    };
    let children = context.document(bytes)?;
    Some(Parsed {
        root: Node::Operation {
            negate: false,
            children,
        },
        missing: context.missing,
        meshes: context.meshes,
    })
}

/// Raw bytes of other union assets, by reference.
pub(super) type Assets = HashMap<AssetRef, Vec<u8>>;

pub(super) struct Parsed {
    pub(super) root: Node,
    /// Assets a nested operation points at that `assets` did not hold: drawn
    /// as boxes in `root`, which is therefore not final until they arrive.
    pub(super) missing: Vec<AssetRef>,
    /// A `MeshPart` is among the leaves: drawn here as its box, since its
    /// mesh is not at hand, so the tree carves wrong and the union's own
    /// bake (Studio's "Solid Modeling On Meshes") is what to draw.
    pub(super) meshes: bool,
}

/// A guard against an asset that (directly or not) names itself.
const MAX_DEPTH: usize = 16;

struct Context<'a> {
    database: &'a ReflectionDatabase,
    assets: &'a Assets,
    missing: Vec<AssetRef>,
    depth: usize,
    min_size: f32,
    meshes: bool,
}

impl Context<'_> {
    /// The roots of the operation document in `bytes`, walked under `parent`.
    fn document(&mut self, bytes: &[u8]) -> Option<Vec<Node>> {
        self.document_under(bytes, Mat4::IDENTITY, false)
    }

    fn document_under(&mut self, bytes: &[u8], parent: Mat4, echo: bool) -> Option<Vec<Node>> {
        let dom = rbx_binary::deserialize(bytes).ok()?;
        let asset_root = dom
            .root_refs()
            .first()
            .filter(|&&root| dom.get(root).is_some_and(|i| i.class() == ASSET_CLASS))
            .copied();
        match asset_root {
            Some(root) => {
                let raw = child_data(dom.get(root)?.properties())?;
                self.children(raw, parent, echo)
            }
            None => Some(self.roots(&dom, parent, echo)),
        }
    }

    fn children(&mut self, raw: &[u8], parent: Mat4, echo: bool) -> Option<Vec<Node>> {
        let nested = rbx_binary::deserialize(raw).ok()?;
        Some(self.roots(&nested, parent, echo))
    }

    fn roots(&mut self, dom: &WeakDom, parent: Mat4, echo: bool) -> Vec<Node> {
        dom.root_refs()
            .iter()
            .filter_map(|&child| self.walk(dom, child, parent, echo))
            .collect()
    }

    /// `echo`: whether `referent`'s immediate parent is a `NegateOperation`. A
    /// `NegateOperation` is restricted by the engine to exactly one child, and
    /// that child's own `CFrame` is not a further local offset but a verbatim
    /// copy of the `NegateOperation`'s own rotation with position zeroed out
    /// (empirically, see the module doc); composing it in as if it were a
    /// real relative transform silently double-applies that rotation. When
    /// `echo` is set, `referent`'s own `CFrame` is ignored and `parent` (the
    /// `NegateOperation`'s already-composed frame) is used as-is.
    fn walk(&mut self, dom: &WeakDom, referent: Ref, parent: Mat4, echo: bool) -> Option<Node> {
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

        // A nested operation's own tree is authored against its
        // `InitialSize`, exactly like a top-level union's, so a resize since
        // scales its children the same way `Entry::placement` scales those.
        let frame = cframe * Mat4::from_scale(resize(properties));
        if let Some(raw) = child_data(properties) {
            let children = self.children(raw, frame, negate)?;
            return Some(Node::Operation { negate, children });
        }
        if let Some(children) = self.referenced(properties, frame, negate) {
            return Some(Node::Operation { negate, children });
        }

        let Some(&Variant::Vector3(size)) = properties.get("size") else {
            return None;
        };
        let size = Vec3::new(size.x, size.y, size.z).max(Vec3::splat(self.min_size));
        self.meshes |= self.database.is_subclass_of(instance.class(), "MeshPart");
        Some(Node::Leaf(Leaf {
            negate,
            geometry: shape::resolve(dom, self.database, instance, size),
            cframe,
            properties: properties.clone(),
        }))
    }

    /// The children of a nested operation baked into an asset of its own,
    /// which the operation names by `AssetId` instead of carrying them,
    /// placed in `frame`. `None` (drawn as its box) while the asset is
    /// missing or unusable.
    fn referenced(
        &mut self,
        properties: &BTreeMap<String, Variant>,
        frame: Mat4,
        negate: bool,
    ) -> Option<Vec<Node>> {
        let asset = AssetRef::parse(asset_uri(properties.get("AssetId")?)?).ok()?;
        if asset == AssetRef::Empty {
            return None;
        }
        let Some(bytes) = self.assets.get(&asset) else {
            if !self.missing.contains(&asset) {
                self.missing.push(asset);
            }
            return None;
        };
        if self.depth >= MAX_DEPTH {
            return None;
        }
        self.depth += 1;
        let children = self.document_under(bytes, frame, negate);
        self.depth -= 1;
        children
    }
}

/// `size / InitialSize`: how far an operation was resized after its tree was
/// baked. One where either is missing, as in a file that predates the
/// property.
fn resize(properties: &BTreeMap<String, Variant>) -> Vec3 {
    match (properties.get("size"), properties.get("InitialSize")) {
        (Some(&Variant::Vector3(size)), Some(&Variant::Vector3(initial))) => {
            Vec3::new(size.x, size.y, size.z)
                / Vec3::new(initial.x, initial.y, initial.z).max(Vec3::splat(f32::EPSILON))
        }
        _ => Vec3::ONE,
    }
}

/// A union's own nested operation document: `ChildData` (a `BinaryString`)
/// or `ChildData2` (the `SharedString` newer files keep it in), whichever is
/// filled. `None` when neither holds anything, or only valid UTF-8, which
/// can only mean no nested document at all.
pub(super) fn child_data(properties: &BTreeMap<String, Variant>) -> Option<&[u8]> {
    CHILD_DATA_PROPERTIES
        .iter()
        .find_map(|key| match properties.get(*key) {
            Some(Variant::Unknown { type_id, raw })
                if (*type_id == STRING_TYPE_ID || *type_id == SHARED_STRING_TYPE_ID)
                    && !raw.is_empty() =>
            {
                Some(raw.as_slice())
            }
            _ => None,
        })
}
