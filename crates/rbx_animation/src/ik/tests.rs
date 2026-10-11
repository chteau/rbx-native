use std::collections::{BTreeMap, HashMap, HashSet};

use glam::{Quat, Vec3};
use rbx_dom::{Ref, Variant, WeakDom};

use super::pose::reject;
use super::*;
use crate::rig::JointKind;

struct Body {
    rig: Rig,
    parts: HashMap<String, Ref>,
}

impl Body {
    fn part(&self, name: &str) -> Ref {
        self.parts[name]
    }

    fn joint(&self, part1: &str) -> usize {
        let part = self.part(part1);
        self.rig
            .joints
            .iter()
            .position(|j| j.part1 == part)
            .unwrap()
    }

    fn world(&self, transforms: &BTreeMap<String, Frame>) -> HashMap<Ref, Frame> {
        self.rig.layout(ROOT, &|n| transforms.get(n).copied())
    }

    fn pivot(&self, transforms: &BTreeMap<String, Frame>, part1: &str) -> Vec3 {
        let j = &self.rig.joints[self.joint(part1)];
        let t = transforms.get(part1).copied().unwrap_or_default();
        self.world(transforms)[&j.part0]
            .mul(&j.c0)
            .transform_point(t.position)
    }

    fn point(&self, transforms: &BTreeMap<String, Frame>, part: &str, local: Vec3) -> Vec3 {
        self.world(transforms)[&self.part(part)].transform_point(local)
    }
}

const ROOT: Frame = Frame {
    rotation: Quat::IDENTITY,
    position: Vec3::new(0., 3., 0.),
};

/// Grabbed a little below the hand's centre, so the effector is off-pivot.
const GRIP: Vec3 = Vec3::new(0., -0.2, 0.);

/// Builds a rig from part centres and joint pivots, all axis-aligned, so each
/// joint's C0/C1 is the pivot's offset from either part.
fn build<S: AsRef<str>>(kind: JointKind, parts: &[(S, Vec3)], joints: &[(S, S, Vec3)]) -> Body {
    let mut dom = WeakDom::new();
    let model = dom.new_instance("Model", "Rig", None);
    let mut refs = HashMap::new();
    let mut centre = HashMap::new();
    for (name, at) in parts {
        let (name, at) = (name.as_ref(), *at);
        let p = dom.new_instance("Part", name, Some(model));
        dom.set_property(p, "CFrame", Variant::CFrame(Frame::at(at).to_data()))
            .unwrap();
        refs.insert(name.to_owned(), p);
        centre.insert(name, at);
    }
    for (p0, p1, pivot) in joints {
        let (p0, p1, pivot) = (p0.as_ref(), p1.as_ref(), *pivot);
        let (r0, r1) = (refs[p0], refs[p1]);
        let c0 = Variant::CFrame(Frame::at(pivot - centre[p0]).to_data());
        let c1 = Variant::CFrame(Frame::at(pivot - centre[p1]).to_data());
        match kind {
            JointKind::Motor6D => {
                let m = dom.new_instance("Motor6D", p1, Some(r1));
                dom.set_property(m, "Part0", Variant::Ref(r0)).unwrap();
                dom.set_property(m, "Part1", Variant::Ref(r1)).unwrap();
                dom.set_property(m, "C0", c0).unwrap();
                dom.set_property(m, "C1", c1).unwrap();
            }
            JointKind::AnimationConstraint => {
                let a0 = dom.new_instance("Attachment", "A0", Some(r0));
                dom.set_property(a0, "CFrame", c0).unwrap();
                let a1 = dom.new_instance("Attachment", "A1", Some(r1));
                dom.set_property(a1, "CFrame", c1).unwrap();
                let c = dom.new_instance("AnimationConstraint", p1, Some(r1));
                dom.set_property(c, "Attachment0", Variant::Ref(a0))
                    .unwrap();
                dom.set_property(c, "Attachment1", Variant::Ref(a1))
                    .unwrap();
            }
        }
    }
    Body {
        rig: Rig::read(&dom, model),
        parts: refs,
    }
}

/// R15, arms hanging straight, facing -Z.
fn r15(kind: JointKind) -> Body {
    let n0 = str::to_owned;
    let mut parts = vec![
        (n0("HumanoidRootPart"), Vec3::new(0., 3., 0.)),
        (n0("LowerTorso"), Vec3::new(0., 2.8, 0.)),
        (n0("UpperTorso"), Vec3::new(0., 3.6, 0.)),
        (n0("Head"), Vec3::new(0., 4.7, 0.)),
    ];
    let mut joints = vec![
        (
            n0("HumanoidRootPart"),
            n0("LowerTorso"),
            Vec3::new(0., 2.6, 0.),
        ),
        (n0("LowerTorso"), n0("UpperTorso"), Vec3::new(0., 3.2, 0.)),
        (n0("UpperTorso"), n0("Head"), Vec3::new(0., 4.2, 0.)),
    ];
    for (side, x) in [("Left", -1.), ("Right", 1.)] {
        let n = |s: &str| format!("{side}{s}");
        let (arm, leg) = (1.5 * x, 0.5 * x);
        parts.extend([
            (n("UpperArm"), Vec3::new(arm, 3.6, 0.)),
            (n("LowerArm"), Vec3::new(arm, 2.9, 0.)),
            (n("Hand"), Vec3::new(arm, 2.3, 0.)),
            (n("UpperLeg"), Vec3::new(leg, 1.8, 0.)),
            (n("LowerLeg"), Vec3::new(leg, 1.0, 0.)),
            (n("Foot"), Vec3::new(leg, 0.3, 0.)),
        ]);
        joints.extend([
            (n0("UpperTorso"), n("UpperArm"), Vec3::new(arm, 4.0, 0.)),
            (n("UpperArm"), n("LowerArm"), Vec3::new(arm, 3.2, 0.)),
            (n("LowerArm"), n("Hand"), Vec3::new(arm, 2.5, 0.)),
            (n0("LowerTorso"), n("UpperLeg"), Vec3::new(leg, 2.3, 0.)),
            (n("UpperLeg"), n("LowerLeg"), Vec3::new(leg, 1.4, 0.)),
            (n("LowerLeg"), n("Foot"), Vec3::new(leg, 0.5, 0.)),
        ]);
    }
    build(kind, &parts, &joints)
}

fn r6() -> Body {
    let parts = [
        ("HumanoidRootPart", Vec3::new(0., 3., 0.)),
        ("Torso", Vec3::new(0., 3., 0.)),
        ("Head", Vec3::new(0., 4.5, 0.)),
        ("Right Arm", Vec3::new(1.5, 3., 0.)),
        ("Left Arm", Vec3::new(-1.5, 3., 0.)),
        ("Right Leg", Vec3::new(0.5, 1., 0.)),
    ];
    let joints = [
        ("HumanoidRootPart", "Torso", Vec3::new(0., 3., 0.)),
        ("Torso", "Head", Vec3::new(0., 4., 0.)),
        ("Torso", "Right Arm", Vec3::new(1.5, 3.5, 0.)),
        ("Torso", "Left Arm", Vec3::new(-1.5, 3.5, 0.)),
        ("Torso", "Right Leg", Vec3::new(0.5, 2., 0.)),
    ];
    build(JointKind::Motor6D, &parts, &joints)
}

fn drag(
    body: &Body,
    from: &BTreeMap<String, Frame>,
    part: &str,
    target: Vec3,
    mode: Mode,
) -> BTreeMap<String, Frame> {
    solve(
        &body.rig,
        ROOT,
        from,
        body.part(part),
        GRIP,
        target,
        mode,
        &HashSet::new(),
    )
}

/// The elbow's offset from the shoulder-hand line, and the side it may bend
/// toward (+Z in the upper arm's own space).
fn elbow_side(body: &Body, pose: &BTreeMap<String, Frame>) -> (Vec3, Vec3) {
    let a = body.pivot(pose, "RightUpperArm");
    let b = body.pivot(pose, "RightLowerArm");
    let c = body.point(pose, "RightHand", GRIP);
    let back = body.world(pose)[&body.part("RightUpperArm")].rotation * Vec3::Z;
    (reject(b - a, (c - a).normalize()), back)
}

#[test]
fn two_bone_reaches_reachable_targets_exactly() {
    for kind in [JointKind::Motor6D, JointKind::AnimationConstraint] {
        let body = r15(kind);
        for target in [
            Vec3::new(1.5, 3.0, -0.8),
            Vec3::new(2.0, 3.5, -1.0),
            Vec3::new(1.0, 2.8, 0.5),
            Vec3::new(2.6, 4.2, 0.3),
        ] {
            let pose = drag(&body, &BTreeMap::new(), "RightHand", target, Mode::BodyPart);
            let hand = body.point(&pose, "RightHand", GRIP);
            assert!(hand.distance(target) < 1e-3, "{kind:?} {target} got {hand}");
        }
    }
}

#[test]
fn two_bone_straightens_toward_an_unreachable_target() {
    let body = r15(JointKind::Motor6D);
    let target = Vec3::new(1.5, 4., -10.);
    let pose = drag(&body, &BTreeMap::new(), "RightHand", target, Mode::BodyPart);
    let a = body.pivot(&pose, "RightUpperArm");
    let c = body.point(&pose, "RightHand", GRIP);
    // Upper bone 0.8, shoulder-pivot to grip 1.1 below the elbow.
    assert!(((c - a).length() - 1.9).abs() < 1e-4);
    assert!((c - a)
        .normalize()
        .abs_diff_eq((target - a).normalize(), 1e-4));
}

#[test]
fn two_bone_keeps_the_bend_plane() {
    let body = r15(JointKind::Motor6D);
    let mut from = BTreeMap::new();
    from.insert("RightLowerArm".to_owned(), Frame::angles(0.8, 0., 0.));
    // In the arm's own (x = 1.5) plane, so the plane must survive exactly.
    let target = Vec3::new(1.5, 3.6, -1.2);
    let pose = drag(&body, &from, "RightHand", target, Mode::BodyPart);
    assert!(body.point(&pose, "RightHand", GRIP).distance(target) < 1e-3);
    assert!((body.pivot(&pose, "RightLowerArm").x - 1.5).abs() < 1e-4);
    // The forearm only hinged: its turn relative to the upper arm is still
    // about the upper arm's X.
    let elbow = pose["RightLowerArm"].rotation;
    let (axis, angle) = elbow.to_axis_angle();
    assert!(
        angle > 0.05 && axis.abs_diff_eq(Vec3::X, 1e-3),
        "{axis} {angle}"
    );
}

#[test]
fn elbow_never_hyperextends() {
    let body = r15(JointKind::Motor6D);
    let target = Vec3::new(1.5, 2.8, -1.0);
    let mut backwards = BTreeMap::new();
    backwards.insert("RightLowerArm".to_owned(), Frame::angles(-0.6, 0., 0.));
    let (e, back) = elbow_side(&body, &backwards);
    assert!(e.dot(back) < 0., "starts hyperextended");
    for from in [BTreeMap::new(), backwards] {
        let pose = drag(&body, &from, "RightHand", target, Mode::BodyPart);
        assert!(body.point(&pose, "RightHand", GRIP).distance(target) < 1e-3);
        let (e, back) = elbow_side(&body, &pose);
        assert!(e.dot(back) > 1e-3, "elbow bent forward: {e}");
    }
}

#[test]
fn knees_bend_forward() {
    let body = r15(JointKind::Motor6D);
    let target = Vec3::new(0.5, 1.2, 0.);
    let pose = drag(&body, &BTreeMap::new(), "RightFoot", target, Mode::BodyPart);
    assert!(body.point(&pose, "RightFoot", GRIP).distance(target) < 1e-3);
    assert!(body.pivot(&pose, "RightLowerLeg").z < -0.1);
}

#[test]
fn body_part_chains_follow_r15_limbs() {
    let body = r15(JointKind::Motor6D);
    let chain_of = |part: &str| chain(&body.rig, body.part(part), Mode::BodyPart, &HashSet::new());
    let arm = vec![body.joint("RightUpperArm"), body.joint("RightLowerArm")];
    assert_eq!(chain_of("RightHand"), arm);
    assert_eq!(chain_of("RightLowerArm"), arm);
    assert_eq!(chain_of("RightUpperArm"), vec![body.joint("RightUpperArm")]);
    let leg = vec![body.joint("LeftUpperLeg"), body.joint("LeftLowerLeg")];
    assert_eq!(chain_of("LeftFoot"), leg);
    assert_eq!(chain_of("LeftLowerLeg"), leg);
    assert_eq!(chain_of("Head"), vec![body.joint("Head")]);
    assert_eq!(chain_of("UpperTorso"), vec![body.joint("UpperTorso")]);
    assert_eq!(chain_of("LowerTorso"), vec![body.joint("LowerTorso")]);
    assert!(chain_of("HumanoidRootPart").is_empty());
}

#[test]
fn body_part_chains_for_r6_are_one_joint() {
    let body = r6();
    for part in ["Left Arm", "Right Leg", "Head", "Torso"] {
        let found = chain(&body.rig, body.part(part), Mode::BodyPart, &HashSet::new());
        assert_eq!(found, vec![body.joint(part)], "{part}");
    }
}

#[test]
fn single_joints_aim_at_the_target() {
    let body = r6();
    let target = Vec3::new(3., 4.5, -1.);
    let pose = drag(&body, &BTreeMap::new(), "Right Arm", target, Mode::BodyPart);
    let shoulder = body.pivot(&pose, "Right Arm");
    let hand = body.point(&pose, "Right Arm", GRIP);
    assert!((hand - shoulder)
        .normalize()
        .abs_diff_eq((target - shoulder).normalize(), 1e-4));
    assert!(pose.keys().eq(["Right Arm"]));
}

#[test]
fn full_body_chain_runs_to_the_root_and_stops_at_pins() {
    let body = r15(JointKind::Motor6D);
    let hand = body.part("RightHand");
    let whole: Vec<usize> = [
        "LowerTorso",
        "UpperTorso",
        "RightUpperArm",
        "RightLowerArm",
        "RightHand",
    ]
    .map(|p| body.joint(p))
    .into();
    assert_eq!(
        chain(&body.rig, hand, Mode::FullBody, &HashSet::new()),
        whole
    );
    let foot: HashSet<Ref> = [body.part("LeftFoot")].into();
    assert_eq!(chain(&body.rig, hand, Mode::FullBody, &foot), whole[1..]);
    let both: HashSet<Ref> = [body.part("LeftFoot"), body.part("LeftHand")].into();
    assert_eq!(chain(&body.rig, hand, Mode::FullBody, &both), whole[2..]);
    assert!(chain(&body.rig, hand, Mode::FullBody, &[hand].into()).is_empty());
}

#[test]
fn full_body_reaches_past_the_arm_and_leaves_pins_in_place() {
    for kind in [JointKind::Motor6D, JointKind::AnimationConstraint] {
        let body = r15(kind);
        let pinned: HashSet<Ref> = [body.part("LeftFoot")].into();
        let from = BTreeMap::new();
        let before = body.world(&from);
        // 2.55 studs from the shoulder: the arm alone tops out at 1.9.
        let target = Vec3::new(1.0, 2.5, -2.0);
        let hand = body.part("RightHand");
        let pose = solve(
            &body.rig,
            ROOT,
            &from,
            hand,
            GRIP,
            target,
            Mode::FullBody,
            &pinned,
        );
        let after = body.world(&pose);
        let reached = body.point(&pose, "RightHand", GRIP);
        assert!(reached.distance(target) < 1e-3, "{kind:?} got {reached}");
        for part in ["LeftFoot", "LowerTorso", "LeftUpperLeg"] {
            let r = body.part(part);
            assert!(after[&r].approx_eq(&before[&r], 1e-5), "{part} moved");
        }
        assert!(!pose.contains_key("LowerTorso"));
    }
}

#[test]
fn only_chain_rotations_change() {
    let body = r15(JointKind::Motor6D);
    let mut from = BTreeMap::new();
    from.insert("Head".to_owned(), Frame::angles(0.2, 0.3, 0.));
    from.insert(
        "RightUpperArm".to_owned(),
        Frame::new(Vec3::new(0.1, 0., 0.), Quat::from_rotation_z(0.2)),
    );
    from.insert("LeftLowerArm".to_owned(), Frame::angles(0.5, 0., 0.));
    for mode in [Mode::BodyPart, Mode::FullBody] {
        let target = Vec3::new(1.8, 3.4, -1.0);
        let pose = drag(&body, &from, "RightHand", target, mode);
        let moved: HashSet<String> =
            chain(&body.rig, body.part("RightHand"), mode, &HashSet::new())
                .into_iter()
                .map(|j| body.rig.part_name(body.rig.joints[j].part1).to_owned())
                .collect();
        for (name, frame) in &pose {
            match from.get(name) {
                Some(old) if !moved.contains(name) => assert_eq!(frame, old, "{name}"),
                Some(old) => assert!(frame.position.abs_diff_eq(old.position, 1e-6), "{name}"),
                None => {
                    assert!(moved.contains(name), "{name} keyed outside the chain");
                    assert!(frame.position.abs_diff_eq(Vec3::ZERO, 1e-6));
                }
            }
        }
        assert!(from.keys().all(|k| pose.contains_key(k)));
    }
}
