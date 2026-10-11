//! A three-part rig (root, torso, arm) built either way a rig can be built,
//! shared by every module's tests.

use glam::Vec3;
use rbx_dom::{Ref, Variant, WeakDom};

use crate::frame::Frame;
use crate::rig::JointKind;

pub(crate) struct Fixture {
    pub dom: WeakDom,
    pub model: Ref,
    pub root: Ref,
    pub torso: Ref,
    pub arm: Ref,
    pub joints: [Ref; 2],
}

fn part(dom: &mut WeakDom, name: &str, parent: Ref, at: Vec3) -> Ref {
    let part = dom.new_instance("Part", name, Some(parent));
    let cframe = Variant::CFrame(Frame::at(at).to_data());
    dom.set_property(part, "CFrame", cframe).unwrap();
    part
}

fn join(dom: &mut WeakDom, kind: JointKind, name: &str, p0: Ref, p1: Ref, c0: Frame, c1: Frame) -> Ref {
    match kind {
        JointKind::Motor6D => {
            let m = dom.new_instance("Motor6D", name, Some(p1));
            dom.set_property(m, "Part0", Variant::Ref(p0)).unwrap();
            dom.set_property(m, "Part1", Variant::Ref(p1)).unwrap();
            dom.set_property(m, "C0", Variant::CFrame(c0.to_data())).unwrap();
            dom.set_property(m, "C1", Variant::CFrame(c1.to_data())).unwrap();
            m
        }
        JointKind::AnimationConstraint => {
            let a0 = dom.new_instance("Attachment", &format!("{name}RigAttachment"), Some(p0));
            dom.set_property(a0, "CFrame", Variant::CFrame(c0.to_data())).unwrap();
            let a1 = dom.new_instance("Attachment", &format!("{name}RigAttachment"), Some(p1));
            dom.set_property(a1, "CFrame", Variant::CFrame(c1.to_data())).unwrap();
            let c = dom.new_instance("AnimationConstraint", name, Some(p1));
            dom.set_property(c, "Attachment0", Variant::Ref(a0)).unwrap();
            dom.set_property(c, "Attachment1", Variant::Ref(a1)).unwrap();
            c
        }
    }
}

impl Fixture {
    pub fn new(kind: JointKind) -> Fixture {
        let mut dom = WeakDom::new();
        let model = dom.new_instance("Model", "Rig", None);
        let root = part(&mut dom, "HumanoidRootPart", model, Vec3::new(0., 3., 0.));
        let torso = part(&mut dom, "Torso", model, Vec3::new(0., 3., 0.));
        let arm = part(&mut dom, "Right Arm", model, Vec3::new(1.5, 3., 0.));
        dom.new_instance("Humanoid", "Humanoid", Some(model));
        let waist = join(&mut dom, kind, "RootJoint", root, torso, Frame::IDENTITY, Frame::IDENTITY);
        let shoulder = join(
            &mut dom,
            kind,
            "Right Shoulder",
            torso,
            arm,
            Frame::at(Vec3::new(1., 0.5, 0.)),
            Frame::at(Vec3::new(-0.5, 0.5, 0.)),
        );
        Fixture { dom, model, root, torso, arm, joints: [waist, shoulder] }
    }
}
