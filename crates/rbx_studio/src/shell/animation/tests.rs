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
