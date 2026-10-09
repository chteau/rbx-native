//! The 37 hidden `Bone`s each reference Mannequin R15 body carries (a skeleton
//! for the skinned parts), as the reference rig lists them: the part, the
//! parent (the part or an earlier bone) and the offset from that parent.

use rbx_dom::{Ref, Variant, WeakDom};

use super::cframe::Cf;

type Bone = (&'static str, &'static str, &'static str, [f32; 3]);

const BONES: [Bone; 37] = [
    (
        "UpperTorso",
        "UpperTorso",
        "Spine",
        [1.98781e-05, -0.168967, -0.0443532],
    ),
    ("UpperTorso", "Spine", "Chest", [0., 0.566852, 0.0153639]),
    (
        "UpperTorso",
        "Chest",
        "RightClavicle",
        [0.227808, 0.157456, 0.140732],
    ),
    (
        "UpperTorso",
        "Chest",
        "LeftClavicle",
        [-0.227808, 0.157456, 0.140732],
    ),
    (
        "RightHand",
        "RightHand",
        "RightHandIndex1",
        [0.0583055, -0.0470015, -0.046868],
    ),
    (
        "RightHand",
        "RightHandIndex1",
        "RightHandIndex2",
        [0.0504298, -0.118521, -0.046625],
    ),
    (
        "RightHand",
        "RightHandIndex2",
        "RightHandIndex3",
        [-0.0128751, -0.128598, -0.0319269],
    ),
    (
        "RightHand",
        "RightHand",
        "RightHandMiddle1",
        [0.056977, -0.062858, 0.0351266],
    ),
    (
        "RightHand",
        "RightHandMiddle1",
        "RightHandMiddle2",
        [0.0451174, -0.14536, -0.0180402],
    ),
    (
        "RightHand",
        "RightHandMiddle2",
        "RightHandMiddle3",
        [-0.0491338, -0.14726, -0.0240477],
    ),
    (
        "RightHand",
        "RightHand",
        "RightHandThumb1",
        [-0.0744121, 0.118293, -0.0820459],
    ),
    (
        "RightHand",
        "RightHandThumb1",
        "RightHandThumb2",
        [-0.0075984, -0.117316, -0.0630664],
    ),
    (
        "RightHand",
        "RightHandThumb2",
        "RightHandThumb3",
        [-0.00569868, -0.0879874, -0.0472998],
    ),
    (
        "RightHand",
        "RightHand",
        "RightHandPinky1",
        [0.0195277, -0.0677587, 0.19762],
    ),
    (
        "RightHand",
        "RightHandPinky1",
        "RightHandPinky2",
        [0.00840044, -0.0932219, 0.0147639],
    ),
    (
        "RightHand",
        "RightHandPinky2",
        "RightHandPinky3",
        [-0.0327406, -0.0734305, 0.000806034],
    ),
    (
        "RightHand",
        "RightHand",
        "RightHandRing1",
        [0.0400274, -0.0723187, 0.125075],
    ),
    (
        "RightHand",
        "RightHandRing1",
        "RightHandRing2",
        [0.027916, -0.118863, -0.00419317],
    ),
    (
        "RightHand",
        "RightHandRing2",
        "RightHandRing3",
        [-0.0500574, -0.0978367, -0.00529429],
    ),
    (
        "Head",
        "Head",
        "HeadBase",
        [-1.3411e-07, -0.208503, 0.108884],
    ),
    (
        "LeftHand",
        "LeftHand",
        "LeftHandRing1",
        [-0.0400324, -0.0723183, 0.125075],
    ),
    (
        "LeftHand",
        "LeftHandRing1",
        "LeftHandRing2",
        [-0.02791, -0.118864, -0.0041931],
    ),
    (
        "LeftHand",
        "LeftHandRing2",
        "LeftHandRing3",
        [0.05006, -0.097836, -0.0052943],
    ),
    (
        "LeftHand",
        "LeftHand",
        "LeftHandMiddle1",
        [-0.0569825, -0.0628583, 0.0351271],
    ),
    (
        "LeftHand",
        "LeftHandMiddle1",
        "LeftHandMiddle2",
        [-0.04511, -0.14536, -0.0180402],
    ),
    (
        "LeftHand",
        "LeftHandMiddle2",
        "LeftHandMiddle3",
        [0.04913, -0.14726, -0.0240477],
    ),
    (
        "LeftHand",
        "LeftHand",
        "LeftHandIndex1",
        [-0.0583024, -0.0470012, -0.0468676],
    ),
    (
        "LeftHand",
        "LeftHandIndex1",
        "LeftHandIndex2",
        [-0.0504301, -0.118521, -0.0466251],
    ),
    (
        "LeftHand",
        "LeftHandIndex2",
        "LeftHandIndex3",
        [0.0128701, -0.128598, -0.031927],
    ),
    (
        "LeftHand",
        "LeftHand",
        "LeftHandThumb1",
        [0.0744076, 0.118293, -0.0820457],
    ),
    (
        "LeftHand",
        "LeftHandThumb1",
        "LeftHandThumb2",
        [0.00760007, -0.117316, -0.063066],
    ),
    (
        "LeftHand",
        "LeftHandThumb2",
        "LeftHandThumb3",
        [0.00569987, -0.087988, -0.0473],
    ),
    (
        "LeftHand",
        "LeftHand",
        "LeftHandPinky1",
        [-0.0195324, -0.0677583, 0.19762],
    ),
    (
        "LeftHand",
        "LeftHandPinky1",
        "LeftHandPinky2",
        [-0.00839996, -0.093222, 0.014764],
    ),
    (
        "LeftHand",
        "LeftHandPinky2",
        "LeftHandPinky3",
        [0.0327399, -0.0734309, 0.000806004],
    ),
    (
        "LeftFoot",
        "LeftFoot",
        "LeftToeBase",
        [0.0741573, -0.246424, -0.184102],
    ),
    (
        "RightFoot",
        "RightFoot",
        "RightToeBase",
        [-0.074501, -0.246421, -0.184102],
    ),
];

fn named(dom: &WeakDom, root: Ref, name: &str) -> Option<Ref> {
    let mut queue = vec![root];
    while let Some(at) = queue.pop() {
        let instance = dom.get(at)?;
        if at != root && instance.name() == name {
            return Some(at);
        }
        queue.extend(instance.children());
    }
    None
}

/// Adds the skeleton to a built Mannequin R15 `rig`.
pub(super) fn add(dom: &mut WeakDom, rig: Ref) {
    for (part, parent, name, offset) in BONES {
        let Some(part_ref) = named(dom, rig, part) else {
            continue;
        };
        let host = if parent == part {
            part_ref
        } else {
            named(dom, part_ref, parent).unwrap_or(part_ref)
        };
        let bone = dom.new_instance("Bone", name, Some(host));
        let _ = dom.set_property(bone, "CFrame", Variant::CFrame(Cf::at(offset).data()));
        let _ = dom.set_property(bone, "Visible", Variant::Bool(false));
    }
}
