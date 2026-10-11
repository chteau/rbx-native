use std::f32::consts::FRAC_PI_2;

use glam::Vec3;

use super::*;
use crate::fixture::Fixture;

fn both() -> [Fixture; 2] {
    [Fixture::new(JointKind::Motor6D), Fixture::new(JointKind::AnimationConstraint)]
}

#[test]
fn finds_joints_parents_first_either_class() {
    for f in both() {
        let rig = Rig::read(&f.dom, f.model);
        assert_eq!(rig.root, Some(f.root));
        let order: Vec<_> = rig.joints.iter().map(|j| j.part1).collect();
        assert_eq!(order, [f.torso, f.arm]);
        assert_eq!(rig.joints[1].instance, f.joints[1]);
        assert_eq!(rig.parent_name("Right Arm"), Some("Torso"));
        assert_eq!(rig.parent_name("Torso"), Some("HumanoidRootPart"));
    }
}

#[test]
fn constraint_rig_reads_offsets_from_attachments() {
    let f = Fixture::new(JointKind::AnimationConstraint);
    let rig = Rig::read(&f.dom, f.model);
    let shoulder = rig.joint_for("Right Arm").unwrap();
    assert_eq!(shoulder.kind, JointKind::AnimationConstraint);
    assert!(shoulder.c0.position.abs_diff_eq(Vec3::new(1., 0.5, 0.), 1e-6));
    assert!(shoulder.c1.position.abs_diff_eq(Vec3::new(-0.5, 0.5, 0.), 1e-6));
}

#[test]
fn rest_layout_matches_the_parts() {
    for f in both() {
        let rig = Rig::read(&f.dom, f.model);
        let world = rig.layout(Frame::at(Vec3::new(0., 3., 0.)), &|_| None);
        assert!(world[&f.arm].position.abs_diff_eq(Vec3::new(1.5, 3., 0.), 1e-5));
    }
}

#[test]
fn transform_turns_about_the_joint_and_solve_inverts_it() {
    let f = Fixture::new(JointKind::Motor6D);
    let rig = Rig::read(&f.dom, f.model);
    let raise = Frame::angles(0., 0., FRAC_PI_2);
    let world = rig.layout(Frame::at(Vec3::new(0., 3., 0.)), &|name| {
        (name == "Right Arm").then_some(raise)
    });
    // The shoulder pivot is torso + (1, 0.5, 0); C1 hangs the arm 0.5 below
    // it, so a quarter turn about Z swings the arm's centre out sideways.
    let arm = world[&f.arm];
    assert!(arm.position.abs_diff_eq(Vec3::new(1.5, 4., 0.), 1e-5), "{arm:?}");
    let shoulder = rig.joint_for("Right Arm").unwrap();
    assert!(shoulder.solve(&world[&f.torso], &arm).approx_eq(&raise, 1e-5));
}

#[test]
fn disconnected_joints_are_dropped_and_empty_models_are_empty() {
    let mut f = Fixture::new(JointKind::Motor6D);
    let stray = f.dom.new_instance("Part", "Stray", Some(f.model));
    let lone = f.dom.new_instance("Part", "Lone", Some(f.model));
    let m = f.dom.new_instance("Motor6D", "Loose", Some(stray));
    f.dom.set_property(m, "Part0", Variant::Ref(lone)).unwrap();
    f.dom.set_property(m, "Part1", Variant::Ref(stray)).unwrap();
    assert_eq!(Rig::read(&f.dom, f.model).joints.len(), 2);

    let empty = f.dom.new_instance("Model", "Empty", None);
    assert!(Rig::read(&f.dom, empty).is_empty());
}
