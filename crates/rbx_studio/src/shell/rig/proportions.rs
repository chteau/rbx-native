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

/// Feminine's hips sit this much wider apart.
pub(crate) const FEMININE_HIP_SPREAD: f32 = 1.1;

impl Scales {
    pub(crate) fn preset(scale: BodyScale, shape: BodyShape) -> Scales {
        let (body_type, proportion) = match scale {
            BodyScale::Classic => (0., 0.),
            BodyScale::RthroNormal => (1., 0.),
            BodyScale::RthroSlender => (1., 1.),
        };
        let (height, width, head) = match shape {
            BodyShape::Masculine => (1., 1., 1.),
            BodyShape::Feminine => (0.95, 0.85, 0.97),
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

/// `[Classic, Rthro Normal, Rthro Slender]` per part kind.
const BASE: [(&str, [V3; 3]); 10] = [
    (
        "HumanoidRootPart",
        [[2., 2., 1.], [2., 2., 1.], [2., 2., 1.]],
    ),
    ("Head", [[1.2, 1.2, 1.2], [1.2, 1.2, 1.2], [1.1, 1.1, 1.1]]),
    (
        "UpperTorso",
        [[2., 1.6, 1.], [1.839, 1.901, 1.073], [1.55, 1.88, 0.98]],
    ),
    (
        "LowerTorso",
        [[2., 0.4, 1.], [1.672, 0.632, 1.037], [1.42, 0.62, 0.95]],
    ),
    (
        "UpperArm",
        [[1., 1.17, 1.], [0.942, 1.213, 0.759], [0.74, 1.213, 0.68]],
    ),
    (
        "LowerArm",
        [[1., 1.05, 1.], [0.812, 1.161, 0.902], [0.65, 1.161, 0.8]],
    ),
    (
        "Hand",
        [[1., 0.3, 1.], [0.753, 0.891, 0.773], [0.6, 0.891, 0.7]],
    ),
    (
        "UpperLeg",
        [[1., 1.22, 1.], [0.781, 1.742, 0.854], [0.65, 1.742, 0.78]],
    ),
    (
        "LowerLeg",
        [[1., 1.19, 1.], [0.721, 1.263, 0.854], [0.6, 1.263, 0.78]],
    ),
    (
        "Foot",
        [[1., 0.3, 1.], [0.721, 0.82, 1.243], [0.6, 0.82, 1.15]],
    ),
];

fn lerp(a: V3, b: V3, t: f32) -> V3 {
    [0, 1, 2].map(|i| a[i] + (b[i] - a[i]) * t)
}

/// The kinds that exist once per side, named `Left<kind>` / `Right<kind>`.
const SIDED: [&str; 6] = [
    "UpperArm", "LowerArm", "Hand", "UpperLeg", "LowerLeg", "Foot",
];

pub(crate) fn r15_sizes(scales: &Scales) -> Sizes {
    let mut sizes = Sizes::new();
    for (kind, [classic, normal, slender]) in BASE {
        let rthro = lerp(normal, slender, scales.proportion);
        let base = lerp(classic, rthro, scales.body_type);
        let size = if kind == "Head" {
            base.map(|v| v * scales.head)
        } else {
            [
                base[0] * scales.width,
                base[1] * scales.height,
                base[2] * scales.depth,
            ]
        };
        if SIDED.contains(&kind) {
            sizes.insert(format!("Left{kind}"), size);
            sizes.insert(format!("Right{kind}"), size);
        } else {
            sizes.insert(kind.to_string(), size);
        }
    }
    sizes
}
