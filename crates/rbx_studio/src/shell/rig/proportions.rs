//! How big each R15 part is for a body scale and shape.
//!
//! Three base bodies, blended by the avatar's own two sliders:
//! `BodyTypeScale` 0 is Classic and 1 is Rthro, and `BodyProportionScale` 0
//! is Rthro Normal and 1 is Rthro Slender (the Importer's Default / Rthro /
//! Rthro Narrow). The three presets are the slider endpoints, so a signed-in
//! avatar's own in-between values blend smoothly. Height, width and depth
//! then scale each axis, and head scale the head alone.
//!
//! The Rthro Normal body is the real R15 "Man" sample's part sizes, equal to
//! the meshes' `InitialSize`. The Classic and Slender sizes are this editor's
//! own bodies, sized to sit inside the avatar-rules ranges; the meshes are
//! scaled to them.
//!
//! Feminine is a preset of this editor, not a Roblox table: Roblox has no
//! body-shape setting on a rig, only sliders. It is the same body at
//! height 0.95, width 0.85 and head 0.97 with the hips set 10% wider,
//! all inside the avatar-rules ranges (height 0.90-1.05, width 0.70-1.0,
//! head 0.95-1.0).

use std::collections::BTreeMap;

use super::bundle::{piece, Family};
use super::cframe::V3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RigType {
    R6,
    R15,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BodyShape {
    Masculine,
    Feminine,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BodyScale {
    Classic,
    RthroNormal,
    RthroSlender,
}

/// The six numbers a R15 `Humanoid` keeps as `NumberValue` children.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Scales {
    pub(crate) height: f32,
    pub(crate) width: f32,
    pub(crate) depth: f32,
    pub(crate) head: f32,
    pub(crate) body_type: f32,
    pub(crate) proportion: f32,
}

/// A feminine Rthro body's hips sit this much wider apart.
pub(crate) const FEMININE_HIP_SPREAD: f32 = 1.1;

impl Scales {
    pub(crate) fn preset(scale: BodyScale, shape: BodyShape) -> Scales {
        let (body_type, proportion) = match scale {
            BodyScale::Classic => (0., 0.),
            BodyScale::RthroNormal => (1., 0.),
            BodyScale::RthroSlender => (1., 1.),
        };
        // Classic has a female body of its own; the Rthro Mannequin has one
        // body for both, so its feminine shape is this editor's slider preset.
        let (height, width, head) = match (shape, scale) {
            (BodyShape::Feminine, BodyScale::RthroNormal | BodyScale::RthroSlender) => {
                (0.95, 0.85, 0.97)
            }
            _ => (1., 1., 1.),
        };
        Scales {
            height,
            width,
            depth: 1.,
            head,
            body_type,
            proportion,
        }
    }
}

/// Part name -> size, for the 15 body parts and `HumanoidRootPart`.
pub(crate) type Sizes = BTreeMap<String, V3>;

/// Rthro Slender's part sizes as a fraction of Rthro Normal's, per axis. The
/// reference avatars have no slender body; this is the editor's own, the
/// proportions of the Importer's "Rthro Narrow".
const SLENDER: [(&str, V3); 9] = [
    ("Head", [0.917, 0.917, 0.917]),
    ("UpperTorso", [0.843, 0.989, 0.913]),
    ("LowerTorso", [0.849, 0.981, 0.916]),
    ("UpperArm", [0.786, 1., 0.896]),
    ("LowerArm", [0.8, 1., 0.887]),
    ("Hand", [0.797, 1., 0.906]),
    ("UpperLeg", [0.832, 1., 0.913]),
    ("LowerLeg", [0.832, 1., 0.913]),
    ("Foot", [0.832, 1., 0.925]),
];

fn lerp(a: V3, b: V3, t: f32) -> V3 {
    [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t)
}

/// The kinds that exist once per side, named `Left<kind>` / `Right<kind>`.
const SIDED: [&str; 6] = [
    "UpperArm", "LowerArm", "Hand", "UpperLeg", "LowerLeg", "Foot",
];

/// The part sizes of a body of `scales`: the reference Classic body of
/// `shape` at `BodyTypeScale` 0, the Mannequin at 1 (slender at proportion 1),
/// blended between, then stretched by height, width and depth, and the head
/// by head scale.
pub(super) fn r15_sizes(scales: &Scales, shape: BodyShape) -> Sizes {
    let size_of =
        |family, kind: &str| piece(family, shape, kind).map_or([1.; 3], |piece| piece.init);
    let mut sizes = Sizes::new();
    for kind in ["HumanoidRootPart", "Head", "UpperTorso", "LowerTorso"]
        .into_iter()
        .chain(SIDED)
    {
        let sided = |side: &str| {
            if SIDED.contains(&kind) {
                format!("{side}{kind}")
            } else {
                kind.to_string()
            }
        };
        let classic = size_of(Family::Classic, &sided("Left"));
        let normal = size_of(Family::Mannequin, &sided("Left"));
        let ratio = SLENDER
            .iter()
            .find(|(name, _)| *name == kind)
            .map_or([1.; 3], |(_, ratio)| *ratio);
        let slender = [0, 1, 2].map(|i| normal[i] * ratio[i]);
        let rthro = lerp(normal, slender, scales.proportion);
        let base = lerp(classic, rthro, scales.body_type);
        let size = match kind {
            "HumanoidRootPart" => base,
            "Head" => base.map(|v| v * scales.head),
            _ => [
                base[0] * scales.width,
                base[1] * scales.height,
                base[2] * scales.depth,
            ],
        };
        for side in ["Left", "Right"] {
            sizes.insert(sided(side), size);
        }
    }
    sizes
}
