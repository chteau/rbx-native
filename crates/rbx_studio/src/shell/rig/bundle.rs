//! The current-generation R15 body, as the Roblox bundle packages ship it:
//! mesh ids, `InitialSize`, cage meshes and the attachments each mesh was
//! authored with (the "R15Fixed" rigs of the torso and limb bundles). Every
//! position is in the part's own space at `init` size; `layout` scales them by
//! `Size / InitialSize` so a joint stays exact at any body scale.
//!
//! Each mesh's bounding box is exactly `init`, centred on the part origin.

use std::collections::BTreeMap;

use super::cframe::V3;

/// One body part's mesh and the attachments it was authored with. The stock
/// parts come from [`piece`]; a player's own body-part packages replace them.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Piece {
    pub(super) name: String,
    pub(super) mesh: u64,
    pub(super) texture: Option<u64>,
    pub(super) init: V3,
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
    init: V3,
    cage: Option<(u64, V3)>,
    rig: &'static [(&'static str, V3)],
    extra: &'static [(&'static str, V3)],
}

pub(super) type Pieces = BTreeMap<String, Piece>;

/// Every stock part.
pub(super) fn stock() -> Pieces {
    PIECES
        .iter()
        .filter_map(|raw| Some((raw.name.to_string(), piece(raw.name)?)))
        .collect()
}

/// The stock Studio part called `name`.
pub(super) fn piece(name: &str) -> Option<Piece> {
    let raw = PIECES.iter().find(|piece| piece.name == name)?;
    let owned = |list: &[(&str, V3)]| list.iter().map(|(n, at)| (n.to_string(), *at)).collect();
    Some(Piece {
        name: raw.name.into(),
        mesh: raw.mesh,
        texture: (raw.name == "Head").then_some(HEAD_TEXTURE),
        init: raw.init,
        cage: raw.cage,
        rig: owned(raw.rig),
        extra: owned(raw.extra),
    })
}

/// The texture Studio's stock R15 head carries.
const HEAD_TEXTURE: u64 = 96037266319483;

const PIECES: [Raw; 15] = [
    Raw {
        name: "Head",
        mesh: 135773526460632,
        init: [1.2, 1.2, 1.2],
        cage: None,
        rig: &[("NeckRigAttachment", [0.0, -0.58581, 0.00107])],
        extra: &[
            ("FaceFrontAttachment", [0.0, -0.01431, -0.59359]),
            ("HatAttachment", [0.0, 0.60686, 0.00107]),
            ("HairAttachment", [0.0, 0.60686, 0.00107]),
            ("FaceCenterAttachment", [0.0, -0.09382, 0.00107]),
        ],
    },
    Raw {
        name: "LowerTorso",
        mesh: 12994951537,
        init: [1.67224, 0.63209, 1.03744],
        cage: Some((12994951451, [0.0, -0.19361, -0.00737])),
        rig: &[
            ("RootRigAttachment", [0.0, -0.32817, -0.02013]),
            ("WaistRigAttachment", [0.0, 0.07183, -0.02013]),
            ("LeftHipRigAttachment", [-0.5, -0.32817, -0.02013]),
            ("RightHipRigAttachment", [0.5, -0.32817, -0.02013]),
        ],
        extra: &[
            ("WaistCenterAttachment", [0.0, -0.32817, -0.02013]),
            ("WaistBackAttachment", [0.0, -0.32817, 0.47987]),
            ("WaistFrontAttachment", [0.0, -0.32817, -0.52013]),
        ],
    },
    Raw {
        name: "UpperTorso",
        mesh: 12994951566,
        init: [1.83908, 1.90124, 1.07332],
        cage: Some((12994951458, [0.0, 0.15459, 0.00577])),
        rig: &[
            ("WaistRigAttachment", [0.0, -0.65038, 0.01925]),
            ("NeckRigAttachment", [0.0, 0.94962, 0.01925]),
            ("RightShoulderRigAttachment", [1.0, 0.71262, 0.01925]),
            ("LeftShoulderRigAttachment", [-1.0, 0.71262, 0.01925]),
        ],
        extra: &[
            ("NeckAttachment", [0.0, 0.94962, 0.01925]),
            ("RightCollarAttachment", [1.0, 0.94962, 0.01925]),
            ("BodyFrontAttachment", [0.0, -0.05038, -0.48075]),
            ("LeftCollarAttachment", [-1.0, 0.94962, 0.01925]),
            ("BodyBackAttachment", [0.0, -0.05038, 0.51925]),
        ],
    },
    Raw {
        name: "LeftUpperArm",
        mesh: 12994951526,
        init: [0.94244, 1.21264, 0.75894],
        cage: Some((12994951449, [-0.13133, 0.08027, -0.05669])),
        rig: &[
            ("LeftShoulderRigAttachment", [0.14767, 0.44817, -0.07464]),
            ("LeftElbowRigAttachment", [-0.35233, -0.27983, -0.07464]),
        ],
        extra: &[("LeftShoulderAttachment", [-0.35233, 0.63817, -0.07464])],
    },
    Raw {
        name: "LeftLowerArm",
        mesh: 12994951524,
        init: [0.81186, 1.16055, 0.90227],
        cage: Some((12994951450, [-0.01559, -0.15361, -0.02887])),
        rig: &[
            ("LeftElbowRigAttachment", [-0.1063, 0.25468, 0.00086]),
            ("LeftWristRigAttachment", [-0.1063, -0.50532, 0.00086]),
        ],
        extra: &[],
    },
    Raw {
        name: "LeftHand",
        mesh: 12994951523,
        init: [0.75315, 0.89053, 0.77295],
        cage: Some((12994951448, [-0.01474, -0.0704, 0.03613])),
        rig: &[("LeftWristRigAttachment", [0.00632, 0.17876, 0.32913])],
        extra: &[("LeftGripAttachment", [0.00632, -0.09624, 0.32913])],
    },
    Raw {
        name: "LeftUpperLeg",
        mesh: 12994951532,
        init: [0.78077, 1.74234, 0.8536],
        cage: Some((12994951453, [-0.04959, 0.00744, 0.00771])),
        rig: &[
            ("LeftHipRigAttachment", [-0.10962, 0.43565, -0.01028]),
            ("LeftKneeRigAttachment", [-0.10962, -0.38635, -0.01028]),
        ],
        extra: &[],
    },
    Raw {
        name: "LeftLowerLeg",
        mesh: 12994951525,
        init: [0.72139, 1.26342, 0.8536],
        cage: Some((12994951447, [-0.00012, 0.00912, -0.00315])),
        rig: &[
            ("LeftKneeRigAttachment", [-0.07993, 0.41989, -0.01028]),
            ("LeftAnkleRigAttachment", [-0.07993, -0.50611, -0.01028]),
        ],
        extra: &[],
    },
    Raw {
        name: "LeftFoot",
        mesh: 12994951501,
        init: [0.72139, 0.82006, 1.243],
        cage: Some((12994951452, [-0.00012, -0.17242, -0.02005])),
        rig: &[("LeftAnkleRigAttachment", [-0.07993, -0.16, 0.18442])],
        extra: &[("LeftFootAttachment", [-0.07993, -0.412, 0.18442])],
    },
    Raw {
        name: "RightUpperArm",
        mesh: 12994951553,
        init: [0.94263, 1.21264, 0.75894],
        cage: Some((12994951485, [0.13123, 0.08027, -0.05669])),
        rig: &[
            ("RightShoulderRigAttachment", [-0.14777, 0.44817, -0.07464]),
            ("RightElbowRigAttachment", [0.35223, -0.27983, -0.07464]),
        ],
        extra: &[("RightShoulderAttachment", [0.35223, 0.63817, -0.07464])],
    },
    Raw {
        name: "RightLowerArm",
        mesh: 12994951560,
        init: [0.81186, 1.16055, 0.90227],
        cage: Some((12994951487, [0.01559, -0.15361, -0.02887])),
        rig: &[
            ("RightElbowRigAttachment", [0.1063, 0.25468, 0.00086]),
            ("RightWristRigAttachment", [0.1063, -0.50532, 0.00086]),
        ],
        extra: &[],
    },
    Raw {
        name: "RightHand",
        mesh: 12994951542,
        init: [0.75315, 0.89053, 0.77295],
        cage: Some((12994951484, [0.01474, -0.0704, 0.03613])),
        rig: &[("RightWristRigAttachment", [-0.00631, 0.17876, 0.32913])],
        extra: &[("RightGripAttachment", [-0.00631, -0.09624, 0.32913])],
    },
    Raw {
        name: "RightUpperLeg",
        mesh: 12994951556,
        init: [0.78077, 1.74232, 0.85365],
        cage: Some((12994951488, [0.04959, 0.00746, 0.00773])),
        rig: &[
            ("RightHipRigAttachment", [0.10962, 0.43564, -0.01025]),
            ("RightKneeRigAttachment", [0.10962, -0.38636, -0.01025]),
        ],
        extra: &[],
    },
    Raw {
        name: "RightLowerLeg",
        mesh: 12994951550,
        init: [0.72139, 1.26341, 0.85363],
        cage: Some((12994951490, [0.00012, 0.0091, -0.00308])),
        rig: &[
            ("RightKneeRigAttachment", [0.07993, 0.41988, -0.01021]),
            ("RightAnkleRigAttachment", [0.07993, -0.50612, -0.01021]),
        ],
        extra: &[],
    },
    Raw {
        name: "RightFoot",
        mesh: 12994951541,
        init: [0.72139, 0.82007, 1.24303],
        cage: Some((12994951494, [0.00012, -0.17242, -0.01996])),
        rig: &[("RightAnkleRigAttachment", [0.07993, -0.16001, 0.18452])],
        extra: &[("RightFootAttachment", [0.07993, -0.41201, 0.18452])],
    },
];
