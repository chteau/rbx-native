use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, PI};

use glam::{Quat, Vec3};
use rbx_dom::{Ref, Variant, WeakDom};

use super::*;
use crate::import::skeleton::Bone;

/// A small R15-shaped rig, arms hanging, facing -Z as Roblox characters do.
fn rig() -> Rig {
    let mut dom = WeakDom::new();
    let model = dom.new_instance("Model", "Rig", None);
    let mut parts: HashMap<&str, (Ref, Vec3)> = HashMap::new();
    for (name, at) in [
        ("HumanoidRootPart", Vec3::new(0., 3., 0.)),
        ("LowerTorso", Vec3::new(0., 2.8, 0.)),
        ("UpperTorso", Vec3::new(0., 3.6, 0.)),
        ("Head", Vec3::new(0., 4.7, 0.)),
        ("LeftUpperArm", Vec3::new(-1.5, 3.6, 0.)),
        ("LeftLowerArm", Vec3::new(-1.5, 2.9, 0.)),
        ("LeftUpperLeg", Vec3::new(-0.5, 1.8, 0.)),
    ] {
        let p = dom.new_instance("Part", name, Some(model));
        dom.set_property(p, "CFrame", Variant::CFrame(Frame::at(at).to_data()))
            .unwrap();
        parts.insert(name, (p, at));
    }
    for (p0, p1, pivot) in [
        ("HumanoidRootPart", "LowerTorso", Vec3::new(0., 2.6, 0.)),
        ("LowerTorso", "UpperTorso", Vec3::new(0., 3.2, 0.)),
        ("UpperTorso", "Head", Vec3::new(0., 4.2, 0.)),
        ("UpperTorso", "LeftUpperArm", Vec3::new(-1.5, 4.0, 0.)),
        ("LeftUpperArm", "LeftLowerArm", Vec3::new(-1.5, 3.2, 0.)),
        ("LowerTorso", "LeftUpperLeg", Vec3::new(-0.5, 2.3, 0.)),
    ] {
        let ((r0, a0), (r1, a1)) = (parts[p0], parts[p1]);
        let m = dom.new_instance("Motor6D", p1, Some(r1));
        dom.set_property(m, "Part0", Variant::Ref(r0)).unwrap();
        dom.set_property(m, "Part1", Variant::Ref(r1)).unwrap();
        dom.set_property(m, "C0", Variant::CFrame(Frame::at(pivot - a0).to_data()))
            .unwrap();
        dom.set_property(m, "C1", Variant::CFrame(Frame::at(pivot - a1).to_data()))
            .unwrap();
    }
    Rig::read(&dom, model)
}

/// The same body as Mixamo exports it: centimetres, facing +Z, a T-pose,
/// three spine bones, `mixamorig:` names. Keyed at 0 (rest) and at 1 (arm
/// lowered to the side, spine bent 30°).
fn mixamo() -> Skeleton {
    let file = |p: Vec3| Quat::from_rotation_y(PI) * p * 100.0;
    let heads: [(&str, Option<usize>, Vec3); 9] = [
        ("mixamorig:Hips", None, Vec3::new(0., 2.6, 0.)),
        ("mixamorig:Spine", Some(0), Vec3::new(0., 2.9, 0.)),
        ("mixamorig:Spine1", Some(1), Vec3::new(0., 3.05, 0.)),
        ("mixamorig:Spine2", Some(2), Vec3::new(0., 3.2, 0.)),
        ("mixamorig:Head", Some(3), Vec3::new(0., 4.2, 0.)),
        ("mixamorig:LeftArm", Some(3), Vec3::new(-1.5, 4.0, 0.)),
        // T-pose: the elbow straight out from the shoulder.
        ("mixamorig:LeftForeArm", Some(5), Vec3::new(-2.3, 4.0, 0.)),
        ("mixamorig:LeftUpLeg", Some(0), Vec3::new(-0.5, 2.3, 0.)),
        ("mixamorig:LeftLeg", Some(7), Vec3::new(-0.5, 1.4, 0.)),
    ];
    let world: Vec<Frame> = heads.iter().map(|(_, _, p)| Frame::at(file(*p))).collect();
    let mut bones: Vec<Bone> = heads
        .iter()
        .enumerate()
        .map(|(i, (name, parent, _))| Bone {
            name: (*name).into(),
            parent: *parent,
            rest: parent.map_or(world[i], |p| world[p].inverse().mul(&world[i])),
            scale: 1.0,
            keys: Vec::new(),
        })
        .collect();
    // Arm down: the file's world turn taking its T-pose direction to -Y.
    let out = (file(Vec3::new(-2.3, 4.0, 0.)) - file(Vec3::new(-1.5, 4.0, 0.))).normalize();
    let lower = Quat::from_rotation_arc(out, Vec3::NEG_Y);
    let lowered = Frame::new(bones[5].rest.position, lower * bones[5].rest.rotation);
    bones[5].keys = vec![(0., bones[5].rest), (1., lowered)];
    let bent = Frame::new(
        bones[2].rest.position,
        Quat::from_rotation_x(PI / 6.) * bones[2].rest.rotation,
    );
    bones[2].keys = vec![(0., bones[2].rest), (1., bent)];
    Skeleton { bones }
}

fn angle(f: &Frame) -> f32 {
    f.rotation.to_axis_angle().1.to_degrees()
}

#[test]
fn names_match_through_prefixes_and_aliases() {
    assert_eq!(normalise("mixamorig:Left_Fore Arm"), "leftforearm");
    let index: HashMap<String, usize> =
        [("leftforearm".to_owned(), 7), ("leftarm".to_owned(), 3)].into();
    assert_eq!(bone_for(&index, "LeftLowerArm"), Some(7));
    assert_eq!(bone_for(&index, "Left Arm"), Some(3), "R6 spelling");
    assert_eq!(bone_for(&index, "RightFoot"), None);
}

#[test]
fn a_tpose_mixamo_walk_drives_an_arms_down_rig() {
    let rig = rig();
    let clip = retarget(&mixamo(), &rig, Frame::at(Vec3::new(0., 3., 0.)), "Mixamo");
    // At the file's rest the arm is straight out: a quarter turn up from
    // the rig's hanging rest.
    let rest = clip.pose(0., "LeftUpperArm").expect("arm keyed").cframe;
    assert!((angle(&rest) - 90.).abs() < 1.0, "{}", angle(&rest));
    // Lowered to the side, it matches the rig's own rest.
    let down = clip.pose(1., "LeftUpperArm").unwrap().cframe;
    assert!(angle(&down) < 1.0, "{}", angle(&down));
    // The bend on an unmatched spine bone reaches UpperTorso.
    let torso = clip.pose(1., "UpperTorso").expect("torso keyed").cframe;
    assert!((angle(&torso) - 30.).abs() < 1.0, "{}", angle(&torso));
    assert!(angle(&clip.pose(0., "UpperTorso").unwrap().cframe) < 1.0);
    // Nothing moves the leg or the head relative to their parents.
    assert!(clip.pose(1., "LeftUpperLeg").is_none());
    assert!(clip.pose(1., "Head").is_none());
}

#[test]
fn an_unmatched_skeleton_gives_an_empty_clip() {
    let mut s = mixamo();
    for b in &mut s.bones {
        b.name = format!("Bone{}", b.name.len());
    }
    assert!(retarget(&s, &rig(), Frame::IDENTITY, "x")
        .keyframes
        .is_empty());
    let _ = FRAC_PI_2;
}
