use gpui_kit::{ParentElement as _, Styled as _};
use rbx_animation::{JointKind, Rig};
use rbx_dom::{Variant, WeakDom};

use super::rig_summary;
use super::widgets::{primary_button, wide_button};

/// Save is the same size as the buttons beside it: one row of buttons.
#[gpui_kit::test]
fn save_matches_its_neighbours(cx: &mut gpui_kit::TestAppContext) {
    let sizes = crate::probe::bounds_of(cx, &["anim-save", "anim-new"], 200., |_, _| {
        gpui_kit::component::h_flex()
            .w(gpui_kit::px(270.))
            .child(primary_button("anim-save", "Save"))
            .child(wide_button("anim-new", "New", true))
    });
    assert_eq!(sizes.len(), 2);
    assert_eq!(sizes[0].1.height, sizes[1].1.height);
    assert!(f32::from(sizes[0].1.height) >= 24.0);
}

/// WCAG 2.5.8: every other panel button keeps the 24px pointer floor.
#[gpui_kit::test]
fn panel_buttons_keep_the_pointer_floor(cx: &mut gpui_kit::TestAppContext) {
    let size = crate::probe::size_of(cx, "anim-key-all", 200., |_, _| {
        wide_button("anim-key-all", "Key all", true)
    });
    assert!(f32::from(size.height) >= 24.0, "{size:?}");
}

fn rig(kinds: &[JointKind]) -> Rig {
    let mut dom = WeakDom::new();
    let model = dom.new_instance("Model", "R", None);
    let mut previous = dom.new_instance("Part", "HumanoidRootPart", Some(model));
    for (i, kind) in kinds.iter().enumerate() {
        let part = dom.new_instance("Part", &format!("P{i}"), Some(model));
        match kind {
            JointKind::Motor6D => {
                let m = dom.new_instance("Motor6D", "J", Some(part));
                dom.set_property(m, "Part0", Variant::Ref(previous))
                    .unwrap();
                dom.set_property(m, "Part1", Variant::Ref(part)).unwrap();
            }
            JointKind::AnimationConstraint => {
                let a0 = dom.new_instance("Attachment", "A0", Some(previous));
                let a1 = dom.new_instance("Attachment", "A1", Some(part));
                let c = dom.new_instance("AnimationConstraint", "J", Some(part));
                dom.set_property(c, "Attachment0", Variant::Ref(a0))
                    .unwrap();
                dom.set_property(c, "Attachment1", Variant::Ref(a1))
                    .unwrap();
            }
        }
        previous = part;
    }
    Rig::read(&dom, model)
}

#[test]
fn the_rig_line_names_its_joint_classes() {
    use JointKind::{AnimationConstraint as C, Motor6D as M};
    assert_eq!(rig_summary(&rig(&[M, M])), "2 joints (Motor6D)");
    assert_eq!(
        rig_summary(&rig(&[C, C, C])),
        "3 joints (AnimationConstraint)"
    );
    assert_eq!(
        rig_summary(&rig(&[M, C])),
        "2 joints (1 Motor6D, 1 AnimationConstraint)"
    );
}

/// Playback writes each joint's `Transform` and nothing else; leaving puts
/// every one back as found (an authored one kept, a new one taken away), so
/// a place saved after playback is byte for byte the one saved before.
#[test]
fn a_save_after_playback_matches_the_save_before() {
    use std::collections::{BTreeMap, HashMap};

    use rbx_animation::Frame;

    use super::pose::{restore_transforms, write_transforms};

    let mut dom = WeakDom::new();
    let model = dom.new_instance("Model", "R", None);
    let root = dom.new_instance("Part", "HumanoidRootPart", Some(model));
    let arm = dom.new_instance("Part", "Arm", Some(model));
    let hand = dom.new_instance("Part", "Hand", Some(model));
    let shoulder = dom.new_instance("Motor6D", "Shoulder", Some(arm));
    dom.set_property(shoulder, "Part0", Variant::Ref(root))
        .unwrap();
    dom.set_property(shoulder, "Part1", Variant::Ref(arm))
        .unwrap();
    // An authored `Transform` the editor must hand back untouched.
    let authored = Variant::CFrame(Frame::angles(0.1, 0.0, 0.0).to_data());
    dom.set_property(shoulder, "Transform", authored.clone())
        .unwrap();
    let a0 = dom.new_instance("Attachment", "A0", Some(arm));
    let a1 = dom.new_instance("Attachment", "A1", Some(hand));
    let wrist = dom.new_instance("AnimationConstraint", "Wrist", Some(hand));
    dom.set_property(wrist, "Attachment0", Variant::Ref(a0))
        .unwrap();
    dom.set_property(wrist, "Attachment1", Variant::Ref(a1))
        .unwrap();
    let rig = Rig::read(&dom, model);
    assert_eq!(rig.joints.len(), 2);
    let before = rbx_binary::serialize(&dom).expect("serialize");

    let mut originals = HashMap::new();
    for t in [0.2_f32, 0.6, 1.0] {
        let pose = BTreeMap::from([
            ("Arm".to_owned(), Frame::angles(t, 0.0, 0.0)),
            ("Hand".to_owned(), Frame::angles(0.0, t, 0.0)),
        ]);
        assert!(write_transforms(&mut dom, &rig, &pose, &mut originals));
    }
    assert_ne!(rbx_binary::serialize(&dom).expect("serialize"), before);
    assert!(dom
        .get(wrist)
        .unwrap()
        .properties()
        .contains_key("Transform"));

    assert!(restore_transforms(&mut dom, &mut originals));
    assert!(originals.is_empty());
    assert_eq!(
        dom.get(shoulder).unwrap().properties().get("Transform"),
        Some(&authored)
    );
    assert!(!dom
        .get(wrist)
        .unwrap()
        .properties()
        .contains_key("Transform"));
    assert_eq!(rbx_binary::serialize(&dom).expect("serialize"), before);
}

/// A Move drag keys the joint's offset in joint space and keeps its turn.
#[test]
fn a_move_drag_keys_the_translation() {
    use glam::{Quat, Vec3};
    use rbx_animation::Frame;
    let turn = Quat::from_rotation_y(0.4);
    let now = Frame::new(Vec3::new(0.1, 0.0, 0.0), turn);
    // What `Joint::solve` hands back for a pure shift: the same turn, give
    // or take rounding.
    let solved = Frame::new(
        Vec3::new(0.1, 0.5, -0.25),
        turn * Quat::from_rotation_x(1e-5),
    );
    let key = super::pose::drag_key(now, solved);
    assert_eq!(key.position, Vec3::new(0.1, 0.5, -0.25));
    assert_eq!(key.rotation, turn);
}

/// A Rotate drag keys the new turn and leaves the translation as it was.
#[test]
fn a_rotate_drag_keeps_the_translation() {
    use glam::{Quat, Vec3};
    use rbx_animation::Frame;
    let now = Frame::new(Vec3::new(0.0, 0.2, 0.0), Quat::IDENTITY);
    let turned = Quat::from_rotation_z(0.5);
    let solved = Frame::new(Vec3::new(0.3, 0.2, 0.0), turned);
    let key = super::pose::drag_key(now, solved);
    assert_eq!(key.position, Vec3::new(0.0, 0.2, 0.0));
    assert_eq!(key.rotation, turned);
}

/// A plain click while posing takes the rig part under it, not its Model,
/// and only when the part in front is the rig's own.
#[test]
fn a_click_on_a_rig_part_picks_that_part() {
    use super::pose::joint_part;
    use rbx_dom::Ref;
    let (arm, torso, wall, root) = (Ref::new(1), Ref::new(2), Ref::new(3), Ref::new(4));
    let rigged = |part: &Ref| [arm, torso, root].contains(part);
    let some_root = Some(root);
    assert_eq!(joint_part(&[arm, torso], some_root, rigged), Some(arm));
    assert_eq!(joint_part(&[torso, arm], some_root, rigged), Some(torso));
    assert_eq!(joint_part(&[wall, arm], some_root, rigged), None);
    assert_eq!(joint_part(&[], some_root, rigged), None);
    // The invisible root round the torso lets the click through to it...
    assert_eq!(joint_part(&[root, torso], some_root, rigged), Some(torso));
    // ...but is still taken when nothing of the rig stands behind it.
    assert_eq!(joint_part(&[root], some_root, rigged), some_root);
    assert_eq!(joint_part(&[root, wall], some_root, rigged), some_root);
}

/// Ctrl+wheel over the Dope Sheet or the Curve Editor zooms the timeline
/// and nothing else; a plain wheel leaves the timeline alone so the joint
/// list scrolls; Shift or a sideways wheel pans.
#[test]
fn ctrl_wheel_zooms_and_a_plain_wheel_scrolls_the_list() {
    use super::sheet::input::{wheel_intent, Wheel};
    assert_eq!(wheel_intent(false, false, 0.0, 40.0), None);
    assert_eq!(wheel_intent(false, false, 0.0, -40.0), None);
    match wheel_intent(false, true, 0.0, 40.0) {
        Some(Wheel::Zoom(f)) => assert!(f > 1.0, "{f}"),
        other => panic!("{other:?}"),
    }
    match wheel_intent(false, true, 0.0, -40.0) {
        Some(Wheel::Zoom(f)) => assert!(f < 1.0, "{f}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(wheel_intent(true, false, 0.0, 40.0), Some(Wheel::Pan(40.0)));
    assert_eq!(
        wheel_intent(false, false, 30.0, 5.0),
        Some(Wheel::Pan(30.0))
    );
}
