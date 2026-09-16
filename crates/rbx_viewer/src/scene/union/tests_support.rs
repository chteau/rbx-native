//! Legacy union assets built in memory, for the tests of everything that
//! reads one. No asset is committed to this repository and none of these
//! touch the network (see `agents/AGENTS.md`), so a test that needs a
//! `PartOperationAsset` serializes one here — through `rbx_binary`, in the
//! very `ChildData`-of-a-nested-`.rbxm` shape `tree::parse` decodes.

use glam::{Mat4, Vec3};
use rbx_dom::{CFrameData, Instance, Ref, Variant, Vector3Data, WeakDom};

/// One box leaf of a synthetic asset's operation tree: an additive `Part` or
/// a `NegateOperation` carving into one.
pub(in crate::scene) struct Leaf {
    class: &'static str,
    cframe: Mat4,
    size: Vec3,
    color: Option<[u8; 3]>,
}

impl Leaf {
    pub(in crate::scene) fn additive(at: Vec3, size: f32) -> Self {
        Leaf {
            class: "Part",
            cframe: Mat4::from_translation(at),
            size: Vec3::splat(size),
            color: None,
        }
    }

    pub(in crate::scene) fn negation(at: Vec3, size: f32) -> Self {
        Leaf {
            class: "NegateOperation",
            cframe: Mat4::from_translation(at),
            size: Vec3::splat(size),
            color: None,
        }
    }

    /// A colour of its own, so a test can tell one recovered piece from
    /// another — a real builder's parts nearly always have one.
    pub(in crate::scene) fn painted(mut self, color: [u8; 3]) -> Self {
        self.color = Some(color);
        self
    }
}

/// `pieces` additive boxes in a row with one negation swallowing every one of
/// them: the boolean carves the result away to nothing (`csg::Failure::Empty`)
/// and `union::resolve` falls back to drawing the additive leaves — the case
/// a union is drawn as its recovered pieces at all.
pub(in crate::scene) fn fallback_leaves(pieces: usize) -> Vec<Leaf> {
    let mut leaves: Vec<Leaf> = (0..pieces)
        .map(|index| {
            Leaf::additive(Vec3::new(index as f32 * 3.0, 0.0, 0.0), 2.0).painted([
                20 * index as u8,
                200,
                30,
            ])
        })
        .collect();
    leaves.push(Leaf::negation(
        Vec3::new(pieces as f32 * 1.5, 0.0, 0.0),
        pieces as f32 * 8.0 + 16.0,
    ));
    leaves
}

/// The raw bytes of a `PartOperationAsset` holding `leaves`.
pub(in crate::scene) fn asset_bytes(leaves: &[Leaf]) -> Vec<u8> {
    let mut inner = WeakDom::new();
    for (index, leaf) in leaves.iter().enumerate() {
        let referent = Ref::new(index as u32 + 1);
        inner.insert(instance(referent, leaf));
        inner.set_parent(referent, None);
    }
    let inner_bytes = rbx_binary::serialize(&inner).expect("synthetic inner dom must serialize");

    let mut outer = WeakDom::new();
    let root = Ref::new(1);
    let mut asset = Instance::new(root, "PartOperationAsset", "Union");
    // `ChildData` is a `BinaryString`, which shares wire type 0x01 (String)
    // with `Variant::String` — see the module doc on `tree::parse`.
    asset.properties_mut().insert(
        "ChildData".to_string(),
        Variant::Unknown {
            type_id: 0x01,
            raw: inner_bytes,
        },
    );
    outer.insert(asset);
    outer.set_parent(root, None);
    rbx_binary::serialize(&outer).expect("synthetic outer dom must serialize")
}

fn instance(referent: Ref, leaf: &Leaf) -> Instance {
    let (_, rotation, translation) = leaf.cframe.to_scale_rotation_translation();
    let basis = glam::Mat3::from_quat(rotation);
    let mut instance = Instance::new(referent, leaf.class, "leaf");
    let properties = instance.properties_mut();
    properties.insert(
        "CFrame".to_string(),
        Variant::CFrame(CFrameData {
            position: Vector3Data {
                x: translation.x,
                y: translation.y,
                z: translation.z,
            },
            // Row-major, the order `cframe_matrix` reads back.
            rotation: [
                basis.x_axis.x,
                basis.y_axis.x,
                basis.z_axis.x,
                basis.x_axis.y,
                basis.y_axis.y,
                basis.z_axis.y,
                basis.x_axis.z,
                basis.y_axis.z,
                basis.z_axis.z,
            ],
        }),
    );
    properties.insert(
        "size".to_string(),
        Variant::Vector3(Vector3Data {
            x: leaf.size.x,
            y: leaf.size.y,
            z: leaf.size.z,
        }),
    );
    if let Some([r, g, b]) = leaf.color {
        properties.insert("Color3uint8".to_string(), Variant::Color3uint8 { r, g, b });
    }
    instance
}
