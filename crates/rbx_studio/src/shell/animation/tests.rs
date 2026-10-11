use rbx_animation::{JointKind, Rig};
use rbx_dom::{Variant, WeakDom};

use super::panel::{primary_button, wide_button};
use super::rig_summary;

/// WCAG 2.5.5: Save, the panels' primary action, is 44 tall in both modes.
#[gpui_kit::test]
fn save_buttons_lay_out_at_least_44_tall(cx: &mut gpui_kit::TestAppContext) {
    for id in ["anim-save", "graph-save"] {
        crate::probe::assert_primary(cx, id, 200., move |_, _| primary_button(id, "Save"));
    }
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
