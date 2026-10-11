use glam::Vec3;

use super::*;
use crate::fixture::Fixture;
use crate::rig::JointKind;

fn clip() -> Clip {
    let mut clip = Clip {
        name: "Wave".into(),
        looped: true,
        priority: Priority::Action2,
        keyframes: Vec::new(),
    };
    let mut raised = PoseKey::new(Frame::angles(0., 0., 1.2));
    raised.style = Style::CubicV2;
    raised.direction = Direction::InOut;
    clip.set_pose(0., "Right Arm", PoseKey::new(Frame::IDENTITY));
    clip.set_pose(0.5, "Right Arm", raised);
    clip.set_pose(0.5, "Torso", PoseKey::new(Frame::at(Vec3::Y)));
    clip.keyframe_mut(0.5).markers.push(Marker {
        name: "Clap".into(),
        value: "loud".into(),
    });
    clip
}

fn child_named(dom: &WeakDom, of: Ref, name: &str) -> Option<Ref> {
    dom.get(of)?
        .children()
        .iter()
        .copied()
        .find(|&c| dom.get(c).is_some_and(|i| i.name() == name))
}

#[test]
fn round_trips_through_the_dom() {
    let f = Fixture::new(JointKind::Motor6D);
    let rig = Rig::read(&f.dom, f.model);
    let mut dom = f.dom;
    let original = clip();
    let seq = write(&mut dom, Some(f.model), &original, Some(&rig));
    let back = read(&dom, seq);
    assert_eq!(back.name, "Wave");
    assert!(back.looped);
    assert_eq!(back.priority, Priority::Action2);
    assert_eq!(back.keyframes.len(), 2);
    let key = back.pose(0.5, "Right Arm").unwrap();
    assert_eq!(key.style, Style::CubicV2);
    assert_eq!(key.direction, Direction::InOut);
    assert!(key.cframe.approx_eq(&Frame::angles(0., 0., 1.2), 1e-5));
    assert_eq!(back.keyframes[1].markers, original.keyframes[1].markers);
    // The structural root pose is not read back as a key.
    assert_eq!(back.joints(), ["Right Arm", "Torso"]);
}

#[test]
fn poses_nest_along_the_rig() {
    let f = Fixture::new(JointKind::AnimationConstraint);
    let rig = Rig::read(&f.dom, f.model);
    let mut dom = f.dom;
    let seq = write(&mut dom, None, &clip(), Some(&rig));
    let first = dom.get(seq).unwrap().children()[0];
    let root = child_named(&dom, first, "HumanoidRootPart").expect("root pose");
    let torso = child_named(&dom, root, "Torso").expect("torso under root");
    let arm = child_named(&dom, torso, "Right Arm").expect("arm under torso");
    assert_eq!(
        dom.get(root).unwrap().properties()["Weight"],
        Variant::Float32(0.)
    );
    assert_eq!(
        dom.get(arm).unwrap().properties()["Weight"],
        Variant::Float32(1.)
    );
}

#[test]
fn without_a_rig_poses_sit_flat() {
    let mut dom = WeakDom::new();
    let seq = write(&mut dom, None, &clip(), None);
    let first = dom.get(seq).unwrap().children()[0];
    assert!(child_named(&dom, first, "Right Arm").is_some());
    assert_eq!(read(&dom, seq).joints(), ["Right Arm", "Torso"]);
}

#[test]
fn overwrite_keeps_the_instance() {
    let mut dom = WeakDom::new();
    let seq = write(&mut dom, None, &clip(), None);
    let mut shorter = clip();
    shorter.remove_keyframe(0.5);
    shorter.name = "Idle".into();
    overwrite(&mut dom, seq, &shorter, None);
    let back = read(&dom, seq);
    assert_eq!(back.name, "Idle");
    assert_eq!(back.keyframes.len(), 1);
}

#[test]
fn duplicate_times_and_foreign_classes() {
    let mut dom = WeakDom::new();
    let seq = dom.new_instance("KeyframeSequence", "S", None);
    for joint in ["Head", "Torso"] {
        let k = dom.new_instance("Keyframe", "Keyframe", Some(seq));
        dom.set_property(k, "Time", Variant::Float32(1.)).unwrap();
        dom.new_instance("Pose", joint, Some(k));
    }
    dom.new_instance("Folder", "Junk", Some(seq));
    let clip = read(&dom, seq);
    assert_eq!(clip.keyframes.len(), 1);
    assert_eq!(clip.joints(), ["Head", "Torso"]);
    assert_eq!(clip.priority, Priority::Core);
    let folder = dom.new_instance("Folder", "F", None);
    assert!(read(&dom, folder).keyframes.is_empty());
}
