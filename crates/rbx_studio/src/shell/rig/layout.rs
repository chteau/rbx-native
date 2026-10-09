//! A rig as plain data: where every part sits, what joins it to what, and
//! which accessory attachments it carries. `build` turns this into
//! instances; keeping it apart is what lets the tests walk the joints
//! without a `WeakDom`.

use std::collections::BTreeMap;

use super::bundle::Piece;
use super::cframe::{Cf, V3};

pub(super) struct PartSpec {
    pub(super) name: String,
    pub(super) size: V3,
    /// In rig space: `HumanoidRootPart` at the origin.
    pub(super) cf: Cf,
    pub(super) attachments: Vec<(String, Cf)>,
    /// The real mesh this part is drawn with (R15), as a `MeshPart`.
    pub(super) mesh: Option<Piece>,
}

pub(super) struct JointSpec {
    pub(super) name: String,
    pub(super) part0: String,
    pub(super) part1: String,
    pub(super) c0: Cf,
    pub(super) c1: Cf,
    /// The part the joint instance lives in: `Part1` on an R15 rig, `Part0`
    /// on R6, as in Studio's own rigs.
    pub(super) host: String,
}

pub(super) struct Layout {
    pub(super) parts: Vec<PartSpec>,
    pub(super) joints: Vec<JointSpec>,
    /// Whether each joint also gets `<Joint>RigAttachment`s (R15 only).
    pub(super) rig_attachments: bool,
    pub(super) hip_height: f32,
}

impl Layout {
    /// Lays `parts` out through `joints`, the root first and every joint
    /// after the one that placed its `Part0`.
    pub(super) fn solve(
        sizes: &BTreeMap<String, V3>,
        order: &[String],
        joints: Vec<JointSpec>,
        attachments: &mut dyn FnMut(&str) -> Vec<(String, Cf)>,
        rig_attachments: bool,
    ) -> Layout {
        let mut placed: BTreeMap<&str, Cf> = BTreeMap::new();
        placed.insert(&order[0], Cf::at([0.; 3]));
        for joint in &joints {
            let Some(parent) = placed.get(joint.part0.as_str()).copied() else {
                continue;
            };
            placed.insert(&joint.part1, parent.joined(&joint.c0, &joint.c1));
        }
        let parts = order
            .iter()
            .map(|name| PartSpec {
                name: name.clone(),
                size: sizes[name],
                cf: placed[name.as_str()],
                attachments: attachments(name),
                mesh: None,
            })
            .collect();
        Layout {
            parts,
            joints,
            rig_attachments,
            hip_height: 0.,
        }
    }

    /// The lowest point of any part but the root: where the feet meet the
    /// ground, in rig space.
    pub(super) fn ground(&self) -> f32 {
        self.parts
            .iter()
            .skip(1)
            .map(|part| part.cf.p[1] - part.size[1] / 2.)
            .fold(f32::INFINITY, f32::min)
    }
}

/// `HatAttachment` & co. on a head of `size`.
pub(super) fn head_attachments(size: V3) -> Vec<(String, Cf)> {
    // The R6 head seats a hat and a face a tenth of a stud off its box.
    let top = [0., size[1] / 2. + 0.1, 0.];
    vec![
        ("HatAttachment".into(), Cf::at(top)),
        ("HairAttachment".into(), Cf::at(top)),
        (
            "FaceFrontAttachment".into(),
            Cf::at([0., 0., -size[2] / 2. - 0.1]),
        ),
        ("FaceCenterAttachment".into(), Cf::at([0.; 3])),
    ]
}

/// The collar, front and back points of the upper body (the whole torso on
/// R6).
pub(super) fn chest_attachments(size: V3) -> Vec<(String, Cf)> {
    let collar = size[0] / 2.;
    let top = size[1] / 2.;
    vec![
        ("NeckAttachment".into(), Cf::at([0., top, 0.])),
        ("LeftCollarAttachment".into(), Cf::at([-collar, top, 0.])),
        ("RightCollarAttachment".into(), Cf::at([collar, top, 0.])),
        (
            "BodyFrontAttachment".into(),
            Cf::at([0., 0., -size[2] / 2.]),
        ),
        ("BodyBackAttachment".into(), Cf::at([0., 0., size[2] / 2.])),
    ]
}

pub(super) fn waist_attachments(size: V3) -> Vec<(String, Cf)> {
    let low = -size[1] / 2.;
    vec![
        (
            "WaistFrontAttachment".into(),
            Cf::at([0., low, -size[2] / 2.]),
        ),
        ("WaistCenterAttachment".into(), Cf::at([0., low, 0.])),
        (
            "WaistBackAttachment".into(),
            Cf::at([0., low, size[2] / 2.]),
        ),
    ]
}

pub(super) fn shoulder_attachment(side: &str, size: V3) -> (String, Cf) {
    (
        format!("{side}ShoulderAttachment"),
        Cf::at([0., size[1] / 2., 0.]),
    )
}

pub(super) fn grip_attachment(side: &str, size: V3) -> (String, Cf) {
    (
        format!("{side}GripAttachment"),
        Cf::at([0., -size[1] / 2., 0.]),
    )
}

pub(super) fn foot_attachment(side: &str, size: V3) -> (String, Cf) {
    (
        format!("{side}FootAttachment"),
        Cf::at([0., -size[1] / 2., 0.]),
    )
}
