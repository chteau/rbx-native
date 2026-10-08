//! The R15 skeleton: 15 parts joined at the `RigAttachment`s Roblox's own
//! R15 meshes were authored with (see `bundle`). Every attachment is scaled
//! by the part's `Size / InitialSize`, exactly as the mesh is, so the parts
//! stay joined at every body scale, and a part at its `InitialSize` (Rthro
//! Normal) has the attachment CFrames of the real rig.

use super::bundle::{Piece, Pieces};
use super::cframe::{Cf, V3};
use super::layout::{JointSpec, Layout};
use super::proportions::{BodyShape, Sizes, FEMININE_HIP_SPREAD};

pub(super) const PARTS: [&str; 16] = [
    "HumanoidRootPart",
    "LowerTorso",
    "UpperTorso",
    "Head",
    "LeftUpperArm",
    "LeftLowerArm",
    "LeftHand",
    "RightUpperArm",
    "RightLowerArm",
    "RightHand",
    "LeftUpperLeg",
    "LeftLowerLeg",
    "LeftFoot",
    "RightUpperLeg",
    "RightLowerLeg",
    "RightFoot",
];

fn scale(piece: &Piece, size: V3, at: V3) -> V3 {
    [0, 1, 2].map(|i| at[i] * size[i] / piece.init[i])
}

pub(super) fn layout(sizes: &Sizes, shape: BodyShape, pieces: &Pieces) -> Layout {
    let spread = if shape == BodyShape::Feminine {
        FEMININE_HIP_SPREAD
    } else {
        1.
    };
    // The joint's attachment on `part`, scaled to that part's size.
    let rig_attachment = |part: &str, joint: &str| -> V3 {
        let Some(piece) = pieces.get(part) else {
            return [0.; 3];
        };
        let name = format!("{joint}RigAttachment");
        let at = piece
            .rig
            .iter()
            .find(|(n, _)| *n == name)
            .map_or([0.; 3], |(_, at)| *at);
        scale(piece, sizes[part], at)
    };
    let joint = |name: &str, part0: &str, part1: &str| {
        let mut c0 = rig_attachment(part0, name);
        if name.ends_with("Hip") {
            c0[0] *= spread;
        }
        JointSpec {
            name: name.into(),
            part0: part0.into(),
            part1: part1.into(),
            c0: Cf::at(c0),
            c1: Cf::at(rig_attachment(part1, name)),
            host: part1.into(),
        }
    };

    let mut joints = vec![
        joint("Root", "HumanoidRootPart", "LowerTorso"),
        joint("Waist", "LowerTorso", "UpperTorso"),
        joint("Neck", "UpperTorso", "Head"),
    ];
    for side in ["Left", "Right"] {
        let part = |kind: &str| format!("{side}{kind}");
        for (name, from, to) in [
            ("Shoulder", "UpperTorso", "UpperArm"),
            ("Elbow", "UpperArm", "LowerArm"),
            ("Wrist", "LowerArm", "Hand"),
            ("Hip", "LowerTorso", "UpperLeg"),
            ("Knee", "UpperLeg", "LowerLeg"),
            ("Ankle", "LowerLeg", "Foot"),
        ] {
            let sided = |kind: &str| {
                if kind.ends_with("Torso") {
                    kind.to_string()
                } else {
                    part(kind)
                }
            };
            joints.push(joint(&part(name), &sided(from), &sided(to)));
        }
    }

    let order: Vec<String> = PARTS.iter().map(|name| name.to_string()).collect();
    let mut attachments = |name: &str| -> Vec<(String, Cf)> {
        let Some(piece) = pieces.get(name) else {
            return vec![("RootAttachment".into(), Cf::at([0.; 3]))];
        };
        piece
            .extra
            .iter()
            .map(|(attachment, at)| {
                let at = scale(piece, sizes[name], *at);
                let cf = if attachment.contains("Grip") {
                    Cf::quarter_x(at)
                } else {
                    Cf::at(at)
                };
                (attachment.to_string(), cf)
            })
            .collect()
    };
    let mut rig = Layout::solve(sizes, &order, joints, &mut attachments, true);
    for part in &mut rig.parts {
        part.mesh = pieces.get(&part.name).cloned();
    }
    let root = rig.parts[0].size[1];
    rig.hip_height = -rig.ground() - root / 2.;
    rig
}
