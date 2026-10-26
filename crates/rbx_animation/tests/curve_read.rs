//! Reading CurveAnimations shaped the way Studio saves them, beyond what the
//! catalog emote fixtures cover: a quaternion `RotationCurve` and a
//! `MarkerCurve` inside a joint folder.

use glam::{Quat, Vec3};
use rbx_animation::curve::{file, values, RotationOrder};
use rbx_dom::{Ref, Variant, WeakDom};

fn hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

/// Saved by Studio from `RotationCurveKey`s at 0 s (identity, cubic),
/// 0.5 s (`CFrame.Angles(0.3, 0.6, -0.2)`, linear), 1 s (`Angles(0, 1.2,
/// 0)`, constant), 2 s (`Angles(0, 0.1, 0)`, cubic, left tangent 0.25)
/// and 3 s (`Angles(0, 0.2, 0)`, cubic, right tangent -0.5).
const STUDIO_ROTATION: &str = "01000000050000000e0000000000000000000000000000803f00000000000000000d172de73d41289c3e94484ebd91bd713f00000000000000000c00000000698c103f000000003249533f00000000000000000e00000000f6b64c3d0000000019ae7f3f0000803e000000000e000000007575cc3d0000000098b87e3f00000000000000bf010000000500000000000000b004000060090000c0120000201c0000";

fn blob(dom: &mut WeakDom, class: &str, name: &str, parent: Ref, raw: Vec<u8>) {
    let r = dom.new_instance(class, name, Some(parent));
    dom.set_property(r, "ValuesAndTimes", Variant::Unknown { type_id: 0x01, raw })
        .unwrap();
}

fn studio_animation() -> (WeakDom, Ref) {
    let mut dom = WeakDom::new();
    let anim = dom.new_instance("CurveAnimation", "QuatProbe", None);
    let root = dom.new_instance("Folder", "HumanoidRootPart", Some(anim));
    let lower = dom.new_instance("Folder", "LowerTorso", Some(root));
    blob(
        &mut dom,
        "RotationCurve",
        "Rotation",
        lower,
        hex(STUDIO_ROTATION),
    );
    let steps = values::write_markers(&[
        values::RawMarker {
            ticks: 600,
            value: b"left".to_vec(),
        },
        values::RawMarker {
            ticks: 1800,
            value: b"right".to_vec(),
        },
    ]);
    blob(&mut dom, "MarkerCurve", "Footstep", lower, steps);
    (dom, anim)
}

#[test]
fn reads_a_studio_quaternion_rotation_curve() {
    let (dom, anim) = studio_animation();
    let read = file::read(&dom, anim).unwrap();
    let joint = &read.curves.joints["LowerTorso"];
    assert_eq!(joint.order, RotationOrder::XYZ);
    let at = |t: f32| Vec3::from_array(joint.rotation.each_ref().map(|c| c.eval(t).unwrap()));
    let want = Quat::from_euler(glam::EulerRot::XYZ, 0.3, 0.6, -0.2);
    assert!(RotationOrder::XYZ.compose(at(0.5)).angle_between(want) < 1e-3);
    assert!(
        (at(1.5) - Vec3::new(0., 1.2, 0.)).length() < 1e-4,
        "constant key holds"
    );
    assert!((at(3.) - Vec3::new(0., 0.2, 0.)).length() < 1e-4);
}

#[test]
fn markers_in_joint_folders_are_kept() {
    let (dom, anim) = studio_animation();
    let read = file::read(&dom, anim).unwrap();
    let markers: Vec<(f32, &str, &str)> = read
        .curves
        .markers
        .iter()
        .map(|(t, m)| (*t, m.name.as_str(), m.value.as_str()))
        .collect();
    assert_eq!(
        markers,
        [(0.25, "Footstep", "left"), (0.75, "Footstep", "right")]
    );
}
