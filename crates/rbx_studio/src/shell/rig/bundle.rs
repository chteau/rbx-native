//! The R15 bodies Roblox ships, as the reference avatar models carry them:
//! mesh ids, `InitialSize`, cage meshes, surface maps and the attachments
//! each mesh was authored with. Every position is in the part's own space at
//! `init` size; `layout` scales them by `Size / InitialSize` so a joint stays
//! exact at any body scale.
//!
//! Two families: Classic (the 2016 mesh avatar, with a male and a female
//! body) and the Mannequin Rthro body, which has one body for both. The
//! Mannequin parts also carry `Bone`s for skinned layered clothing; those are
//! not reproduced.

use std::collections::BTreeMap;

use super::cframe::V3;
use super::proportions::BodyShape;

mod tables;

/// Which reference body a rig's stock parts come from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Family {
    Classic,
    Mannequin,
}

impl Family {
    /// `BodyTypeScale` 0 is Classic, 1 is Rthro; halfway is the switch.
    pub(super) fn of(body_type: f32) -> Family {
        if body_type >= 0.5 {
            Family::Mannequin
        } else {
            Family::Classic
        }
    }
}

/// `ColorMap`, `MetalnessMap`, `NormalMap`, `RoughnessMap`, `TexturePack`.
pub(super) type Surface = [u64; 5];

/// One body part's mesh and the attachments it was authored with. The stock
/// parts come from [`piece`]; a player's own body-part packages replace them.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Piece {
    pub(super) name: String,
    pub(super) family: Family,
    /// Zero for `HumanoidRootPart`, which is a block.
    pub(super) mesh: u64,
    pub(super) texture: Option<u64>,
    pub(super) surface: Option<Surface>,
    pub(super) init: V3,
    /// What the part's `OriginalSize` value says (the size it was authored at).
    pub(super) original: V3,
    /// `CageMeshId` and `CageOrigin` of the `WrapTarget` (layered clothing).
    pub(super) cage: Option<(u64, V3)>,
    /// `<Joint>RigAttachment`s.
    pub(super) rig: Vec<(String, V3)>,
    /// Accessory attachments.
    pub(super) extra: Vec<(String, V3)>,
}

struct Raw {
    name: &'static str,
    mesh: u64,
    texture: Option<u64>,
    init: V3,
    original: V3,
    cage: Option<(u64, V3)>,
    surface: Option<Surface>,
    rig: &'static [(&'static str, V3)],
    extra: &'static [(&'static str, V3)],
}

pub(super) type Pieces = BTreeMap<String, Piece>;

fn table(family: Family, shape: BodyShape) -> &'static [Raw; 16] {
    match (family, shape) {
        (Family::Mannequin, _) => &tables::MANNEQUIN,
        (Family::Classic, BodyShape::Masculine) => &tables::CLASSIC_MALE,
        (Family::Classic, BodyShape::Feminine) => &tables::CLASSIC_FEMALE,
    }
}

/// Every stock part of the body, `HumanoidRootPart` included.
pub(super) fn stock(family: Family, shape: BodyShape) -> Pieces {
    table(family, shape)
        .iter()
        .map(|raw| (raw.name.to_string(), owned(raw, family)))
        .collect()
}

/// The stock part called `name`.
pub(super) fn piece(family: Family, shape: BodyShape, name: &str) -> Option<Piece> {
    let raw = table(family, shape).iter().find(|raw| raw.name == name)?;
    Some(owned(raw, family))
}

fn owned(raw: &Raw, family: Family) -> Piece {
    let list = |list: &[(&str, V3)]| list.iter().map(|(n, at)| (n.to_string(), *at)).collect();
    Piece {
        name: raw.name.into(),
        family,
        mesh: raw.mesh,
        texture: raw.texture,
        surface: raw.surface,
        init: raw.init,
        original: raw.original,
        cage: raw.cage,
        rig: list(raw.rig),
        extra: list(raw.extra),
    }
}
