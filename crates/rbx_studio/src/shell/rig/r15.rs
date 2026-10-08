//! The R15 skeleton: 15 parts joined at the joints and attachment points of
//! Studio's own R15 rig. Each joint position is a fraction of its part's
//! size, blended by `BodyTypeScale` between the blocky Classic fractions
//! (parts meet face to face) and the Rthro ones (taken from the real "Man"
//! rig, where the meshes overlap at the joints), so every body scale keeps
//! its parts joined as the parts grow and shrink.

use super::cframe::{Cf, V3};
use super::layout::{self, JointSpec, Layout};
use super::proportions::BodyShape;
use super::proportions::{Scales, Sizes, FEMININE_HIP_SPREAD};

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

fn joint(name: &str, part0: &str, part1: &str, a0: V3, a1: V3) -> JointSpec {
    JointSpec {
        name: name.into(),
        part0: part0.into(),
        part1: part1.into(),
        c0: Cf::at(a0),
        c1: Cf::at(a1),
        host: part1.into(),
    }
}

pub(super) fn layout(sizes: &Sizes, scales: &Scales, shape: BodyShape) -> Layout {
    let t = scales.body_type;
    let f = |classic: f32, rthro: f32| classic + (rthro - classic) * t;
    let spread = if shape == BodyShape::Feminine {
        FEMININE_HIP_SPREAD
    } else {
        1.
    };
    let size = |name: &str| sizes[name];
    let (lt, ut, head) = (size("LowerTorso"), size("UpperTorso"), size("Head"));

    let mut joints = vec![
        joint(
            "Root",
            "HumanoidRootPart",
            "LowerTorso",
            [0.; 3],
            [0., -lt[1] / 2., 0.],
        ),
        joint(
            "Waist",
            "LowerTorso",
            "UpperTorso",
            [0., lt[1] * f(0.5, 0.114), 0.],
            [0., -ut[1] * f(0.5, 0.342), 0.],
        ),
        joint(
            "Neck",
            "UpperTorso",
            "Head",
            [0., ut[1] / 2., 0.],
            [0., -head[1] * f(0.5, 0.488), 0.],
        ),
    ];
    for (side, s) in [("Left", -1.), ("Right", 1.)] {
        let part = |kind: &str| format!("{side}{kind}");
        let (ua, la, hand) = (
            size(&part("UpperArm")),
            size(&part("LowerArm")),
            size(&part("Hand")),
        );
        let (ul, ll, foot) = (
            size(&part("UpperLeg")),
            size(&part("LowerLeg")),
            size(&part("Foot")),
        );
        // The arm hangs `reach` from the centre line; Rthro arms overlap the
        // torso by a quarter of their width, so the joint sits a little
        // inside the arm's own centre (`inset`).
        let reach = ut[0] / 2. + ua[0] / 2. - 0.25 * t * ua[0];
        let inset = 0.157 * ua[0] * t;
        joints.extend([
            joint(
                &part("Shoulder"),
                "UpperTorso",
                &part("UpperArm"),
                [s * (reach - inset), ut[1] * f(0.49, 0.375), 0.],
                [-s * inset, ua[1] * f(0.5, 0.369), 0.],
            ),
            joint(
                &part("Elbow"),
                &part("UpperArm"),
                &part("LowerArm"),
                [0., -ua[1] * f(0.5, 0.231), 0.],
                [0., la[1] * f(0.5, 0.22), 0.],
            ),
            joint(
                &part("Wrist"),
                &part("LowerArm"),
                &part("Hand"),
                [0., -la[1] * f(0.5, 0.435), 0.],
                [0., hand[1] * f(0.5, 0.2), 0.],
            ),
        ]);
        joints.extend([
            joint(
                &part("Hip"),
                "LowerTorso",
                &part("UpperLeg"),
                [s * lt[0] * f(0.25, 0.299) * spread, -lt[1] / 2., 0.],
                [s * 0.141 * ul[0] * t, ul[1] * f(0.5, 0.25), 0.],
            ),
            joint(
                &part("Knee"),
                &part("UpperLeg"),
                &part("LowerLeg"),
                [0., -ul[1] * f(0.5, 0.221), 0.],
                [0., ll[1] * f(0.5, 0.332), 0.],
            ),
            joint(
                &part("Ankle"),
                &part("LowerLeg"),
                &part("Foot"),
                [0., -ll[1] * f(0.5, 0.4), 0.],
                [0., foot[1] * f(0.5, -0.195), 0.],
            ),
        ]);
    }

    let order: Vec<String> = PARTS.iter().map(|name| name.to_string()).collect();
    let mut attachments = |name: &str| -> Vec<(String, Cf)> {
        let side = ["Left", "Right"]
            .into_iter()
            .find(|side| name.starts_with(side));
        match (name, side) {
            ("Head", _) => layout::head_attachments(head),
            ("UpperTorso", _) => layout::chest_attachments(ut),
            ("LowerTorso", _) => layout::waist_attachments(lt),
            (_, Some(side)) if name.ends_with("UpperArm") => {
                vec![layout::shoulder_attachment(side, size(name))]
            }
            (_, Some(side)) if name.ends_with("Foot") => {
                vec![layout::foot_attachment(side, size(name))]
            }
            (_, Some(side)) if name.ends_with("Hand") => vec![(
                format!("{side}GripAttachment"),
                Cf::quarter_x([0., -size(name)[1] * 0.2, 0.]),
            )],
            _ => Vec::new(),
        }
    };
    let mut rig = Layout::solve(sizes, &order, joints, &mut attachments, true);
    let root = rig.parts[0].size[1];
    rig.hip_height = -rig.ground() - root / 2.;
    rig
}
