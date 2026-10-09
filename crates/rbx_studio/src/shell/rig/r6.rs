//! The R6 skeleton: six blocky parts and the six `Motor6D`s of Studio's own
//! R6 rig, with their exact rotated `C0`/`C1`s.

use std::collections::BTreeMap;

use super::cframe::{Cf, V3};
use super::layout::{self, JointSpec, Layout};

const HRP: V3 = [2., 2., 1.];
const TORSO: V3 = [2., 2., 1.];
const HEAD: V3 = [2., 1., 1.];
const LIMB: V3 = [1., 2., 1.];

const ROOT_ROT: [f32; 9] = [-1., 0., 0., 0., 0., 1., 0., 1., 0.];
const RIGHT_ROT: [f32; 9] = [0., 0., 1., 0., 1., 0., -1., 0., 0.];
const LEFT_ROT: [f32; 9] = [0., 0., -1., 0., 1., 0., 1., 0., 0.];

fn motor(name: &str, part0: &str, part1: &str, c0: Cf, c1: Cf) -> JointSpec {
    JointSpec {
        name: name.into(),
        part0: part0.into(),
        part1: part1.into(),
        c0,
        c1,
        host: part0.into(),
    }
}

pub(super) fn layout() -> Layout {
    let order: Vec<String> = [
        "HumanoidRootPart",
        "Torso",
        "Head",
        "Left Arm",
        "Right Arm",
        "Left Leg",
        "Right Leg",
    ]
    .map(String::from)
    .to_vec();
    let sizes: BTreeMap<String, V3> = order
        .iter()
        .map(|name| {
            let size = match name.as_str() {
                "HumanoidRootPart" => HRP,
                "Torso" => TORSO,
                "Head" => HEAD,
                _ => LIMB,
            };
            (name.clone(), size)
        })
        .collect();
    let joints = vec![
        motor(
            "RootJoint",
            "HumanoidRootPart",
            "Torso",
            Cf::with([0.; 3], ROOT_ROT),
            Cf::with([0.; 3], ROOT_ROT),
        ),
        motor(
            "Right Shoulder",
            "Torso",
            "Right Arm",
            Cf::with([1., 0.5, 0.], RIGHT_ROT),
            Cf::with([-0.5, 0.5, 0.], RIGHT_ROT),
        ),
        motor(
            "Left Shoulder",
            "Torso",
            "Left Arm",
            Cf::with([-1., 0.5, 0.], LEFT_ROT),
            Cf::with([0.5, 0.5, 0.], LEFT_ROT),
        ),
        motor(
            "Right Hip",
            "Torso",
            "Right Leg",
            Cf::with([1., -1., 0.], RIGHT_ROT),
            Cf::with([0.5, 1., 0.], RIGHT_ROT),
        ),
        motor(
            "Left Hip",
            "Torso",
            "Left Leg",
            Cf::with([-1., -1., 0.], LEFT_ROT),
            Cf::with([-0.5, 1., 0.], LEFT_ROT),
        ),
        motor(
            "Neck",
            "Torso",
            "Head",
            Cf::with([0., 1., 0.], ROOT_ROT),
            Cf::with([0., -0.5, 0.], ROOT_ROT),
        ),
    ];
    let mut attachments = |name: &str| -> Vec<(String, Cf)> {
        match name {
            "Head" => layout::head_attachments(HEAD),
            "Torso" => {
                let mut all = layout::chest_attachments(TORSO);
                all.extend(layout::waist_attachments(TORSO));
                all
            }
            "Left Arm" => vec![
                layout::shoulder_attachment("Left", LIMB),
                layout::grip_attachment("Left", LIMB),
            ],
            "Right Arm" => vec![
                layout::shoulder_attachment("Right", LIMB),
                layout::grip_attachment("Right", LIMB),
            ],
            "HumanoidRootPart" => vec![("RootAttachment".into(), Cf::at([0.; 3]))],
            "Left Leg" => vec![layout::foot_attachment("Left", LIMB)],
            "Right Leg" => vec![layout::foot_attachment("Right", LIMB)],
            _ => Vec::new(),
        }
    };
    Layout::solve(&sizes, &order, joints, &mut attachments, false)
}
