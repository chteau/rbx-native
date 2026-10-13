//! The CurveAnimation writer: `values::from_curve`, `file::write` and
//! `file::overwrite`, checked against the emote fixtures and a small R15-ish
//! rig. Every case that matters goes through the same `read` the editor uses,
//! so a write that reads back differently is a failed case, not a skipped one.

use std::collections::BTreeMap;

use glam::Vec3;
use rbx_animation::curve::{
    file, values, CurveClip, FloatCurve, FloatKey, Interpolation, JointCurves, RotationOrder,
};
use rbx_animation::{Frame, Marker, Priority, Rig};
use rbx_dom::{Ref, Variant, WeakDom};

fn fixture(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

const EMOTES: [&str; 3] = [
    "emote_curve_85200395117668.rbxm",
    "emote_curve_71620783740586.rbxm",
    "emote_curve_105317314539586.rbxm",
];

fn part(dom: &mut WeakDom, name: &str, parent: Ref, at: Vec3) -> Ref {
    let p = dom.new_instance("Part", name, Some(parent));
    dom.set_property(p, "CFrame", Variant::CFrame(Frame::at(at).to_data()))
        .unwrap();
    p
}

fn motor(dom: &mut WeakDom, name: &str, p0: Ref, p1: Ref, c0: Vec3, c1: Vec3) {
    let m = dom.new_instance("Motor6D", name, Some(p1));
    dom.set_property(m, "Part0", Variant::Ref(p0)).unwrap();
    dom.set_property(m, "Part1", Variant::Ref(p1)).unwrap();
    dom.set_property(m, "C0", Variant::CFrame(Frame::at(c0).to_data()))
        .unwrap();
    dom.set_property(m, "C1", Variant::CFrame(Frame::at(c1).to_data()))
        .unwrap();
}

/// HumanoidRootPart > LowerTorso > UpperTorso > Head, and UpperTorso > LeftUpperArm.
fn rig_dom() -> (WeakDom, Ref, Rig) {
    let mut dom = WeakDom::new();
    let model = dom.new_instance("Model", "Dummy", None);
    let hrp = part(&mut dom, "HumanoidRootPart", model, Vec3::new(0., 3., 0.));
    let lower = part(&mut dom, "LowerTorso", model, Vec3::new(0., 2.5, 0.));
    let upper = part(&mut dom, "UpperTorso", model, Vec3::new(0., 3.5, 0.));
    let head = part(&mut dom, "Head", model, Vec3::new(0., 4.5, 0.));
    let arm = part(&mut dom, "LeftUpperArm", model, Vec3::new(-1., 3.5, 0.));
    motor(&mut dom, "Root", hrp, lower, Vec3::ZERO, Vec3::ZERO);
    motor(&mut dom, "Waist", lower, upper, Vec3::ZERO, Vec3::ZERO);
    motor(&mut dom, "Neck", upper, head, Vec3::ZERO, Vec3::ZERO);
    motor(&mut dom, "LeftShoulder", upper, arm, Vec3::ZERO, Vec3::ZERO);
    let rig = Rig::read(&dom, model);
    (dom, model, rig)
}

fn lin(points: &[(f32, f32)]) -> FloatCurve {
    FloatCurve::new(
        points
            .iter()
            .map(|&(t, v)| FloatKey::new(t, v, Interpolation::Linear))
            .collect(),
    )
}

fn sample_animation(name: &str, looped: bool, priority: Priority) -> file::CurveAnimation {
    let head = JointCurves {
        position: [
            lin(&[(0.25, 0.0), (0.5, 1.0)]),
            lin(&[(0.5, 0.5)]),
            FloatCurve::new(vec![]),
        ],
        rotation: [
            lin(&[(0.0, 0.0), (1.0, 0.3)]),
            FloatCurve::new(vec![]),
            lin(&[(0.75, -0.2)]),
        ],
        order: RotationOrder::from_value(2),
    };
    let arm = JointCurves {
        position: [
            FloatCurve::new(vec![]),
            FloatCurve::new(vec![]),
            FloatCurve::new(vec![]),
        ],
        rotation: [
            lin(&[(0.5, 1.0)]),
            FloatCurve::new(vec![]),
            FloatCurve::new(vec![]),
        ],
        order: RotationOrder::from_value(0),
    };
    let mut joints = BTreeMap::new();
    joints.insert("Head".to_string(), head);
    joints.insert("LeftUpperArm".to_string(), arm);
    let mut faces = BTreeMap::new();
    faces.insert("JawDrop".to_string(), lin(&[(0.5, 0.0), (1.0, 1.0)]));
    file::CurveAnimation {
        curves: CurveClip {
            name: name.to_string(),
            looped,
            priority,
            joints,
            markers: vec![
                (
                    0.25,
                    Marker {
                        name: "Step".to_string(),
                        value: "left".to_string(),
                    },
                ),
                (
                    0.75,
                    Marker {
                        name: "Step".to_string(),
                        value: "right".to_string(),
                    },
                ),
            ],
        },
        faces,
        keyframe_names: vec![(0.0, "Start".to_string()), (1.0, "End".to_string())],
    }
}

fn descendants(dom: &WeakDom) -> Vec<Ref> {
    let mut out = Vec::new();
    let mut stack = dom.root_refs().to_vec();
    while let Some(r) = stack.pop() {
        out.push(r);
        stack.extend(dom.get(r).unwrap().children().iter().copied());
    }
    out
}

fn of_class(dom: &WeakDom, class: &str) -> Vec<Ref> {
    descendants(dom)
        .into_iter()
        .filter(|&r| class_name(dom, r) == class)
        .collect()
}

fn class_name(dom: &WeakDom, r: Ref) -> String {
    dom.get(r).unwrap().class().to_string()
}

fn child(dom: &WeakDom, parent: Ref, name: &str) -> Ref {
    dom.get(parent)
        .unwrap()
        .children()
        .iter()
        .copied()
        .find(|&c| dom.get(c).unwrap().name() == name)
        .unwrap_or_else(|| panic!("no child {name:?}"))
}

fn assert_near(a: f32, b: f32, what: &str) {
    assert!((a - b).abs() < 1e-4, "{what}: {a} != {b}");
}

#[test]
fn emotes_survive_write_serialize_and_read() {
    let mut found = 0;
    for file_name in EMOTES {
        let dom = rbx_binary::deserialize(&fixture(file_name)).unwrap();
        for r in of_class(&dom, "CurveAnimation") {
            found += 1;
            let a = file::read(&dom, r).unwrap();

            let mut out = WeakDom::new();
            file::write(&mut out, None, &a, None);
            let bytes = rbx_binary::serialize(&out).unwrap();
            let back = rbx_binary::deserialize(&bytes).unwrap();

            let anims = of_class(&back, "CurveAnimation");
            assert_eq!(anims.len(), 1, "{file_name}");
            assert_eq!(file::read(&back, anims[0]).unwrap(), a, "{file_name}");
        }
    }
    assert!(
        found >= 3,
        "expected a CurveAnimation per emote, found {found}"
    );
}

#[test]
fn from_curve_writes_ticks_modes_and_slopes() {
    let c = FloatCurve::new(vec![
        FloatKey::new(0.0, 0.0, Interpolation::Linear),
        FloatKey::new(0.5, 1.0, Interpolation::Cubic),
        FloatKey::new(1.25, 3.0, Interpolation::Constant),
        FloatKey::new(2.0, 4.0, Interpolation::Linear),
    ]);
    let raw = values::from_curve(&c);
    let ticks: Vec<u32> = raw.iter().map(|k| k.ticks).collect();
    assert_eq!(ticks, vec![0, 1200, 3000, 4800]);
    let modes: Vec<u8> = raw.iter().map(|k| k.interpolation).collect();
    assert_eq!(modes, vec![1, 2, 0, 1]);
    assert!(raw.iter().all(|k| k.flags == 0));

    assert_near(raw[0].right, 2.0, "key 0 slope");
    assert_near(raw[1].right, 2.0 / 0.75, "key 1 slope");
    assert_near(raw[2].right, 1.0 / 0.75, "key 2 slope");
    assert_eq!(raw[3].right, 0.0);
    assert_eq!(raw[3].left, 0.0);

    assert_eq!(values::to_curve(&raw), c);
}

#[test]
fn from_curve_keeps_set_tangents_and_flags_them() {
    let c = FloatCurve::new(vec![
        FloatKey {
            time: 0.0,
            value: 0.0,
            interpolation: Interpolation::Cubic,
            left_tangent: Some(0.5),
            right_tangent: Some(-1.0),
        },
        FloatKey {
            time: 1.0,
            value: 2.0,
            interpolation: Interpolation::Cubic,
            left_tangent: None,
            right_tangent: Some(0.25),
        },
    ]);
    let raw = values::from_curve(&c);
    assert_eq!(raw[0].flags, 3);
    assert_eq!(raw[0].left, 0.5);
    assert_eq!(raw[0].right, -1.0);
    assert_eq!(raw[1].flags, 2);
    assert_eq!(raw[1].right, 0.25);
    assert_eq!(values::to_curve(&raw), c);
}

/// Everything the model keeps comes back bit for bit. The slope stored in an
/// unset tangent slot is the engine's own auto slope, which only sometimes
/// equals the slope to the next key that `from_curve` writes, so those slots
/// are not compared (`to_curve` ignores them on read anyway).
#[test]
fn every_fixture_curve_rebuilds_its_bytes() {
    let mut total = 0;
    let mut matched = 0;
    for file_name in EMOTES {
        let dom = rbx_binary::deserialize(&fixture(file_name)).unwrap();
        for r in of_class(&dom, "FloatCurve") {
            let Some(Variant::Unknown { raw: bytes, .. }) = dom
                .get(r)
                .unwrap()
                .properties()
                .get("ValuesAndTimes")
                .cloned()
            else {
                continue;
            };
            total += 1;
            let keys = values::read_float(&bytes).unwrap();
            let name = dom.get(r).unwrap().name().to_owned();
            let curve = values::to_curve(&keys);
            let raw = values::from_curve(&curve);
            assert_eq!(raw.len(), keys.len(), "{file_name} {name}");
            for (i, (got, want)) in raw.iter().zip(&keys).enumerate() {
                let at = format!("{file_name} {name} key {i}");
                assert_eq!(got.ticks, want.ticks, "{at} ticks");
                assert_eq!(got.interpolation, want.interpolation, "{at} mode");
                assert_eq!(got.flags, want.flags, "{at} flags");
                assert_eq!(got.value.to_bits(), want.value.to_bits(), "{at} value");
                if want.flags & 1 != 0 {
                    assert_eq!(got.left.to_bits(), want.left.to_bits(), "{at} left");
                }
                if want.flags & 2 != 0 {
                    assert_eq!(got.right.to_bits(), want.right.to_bits(), "{at} right");
                }
            }
            let rebuilt = values::write_float(&raw);
            assert_eq!(rebuilt.len(), bytes.len(), "{file_name} {name}");
            assert_eq!(
                values::to_curve(&values::read_float(&rebuilt).unwrap()),
                curve,
                "{file_name} {name}"
            );
            if rebuilt == bytes {
                matched += 1;
            }
        }
    }
    eprintln!("{matched} of {total} fixture curves rebuilt byte-for-byte");
    assert!(total > 0);
}

#[test]
fn rig_tree_nests_joints_faces_and_markers() {
    let (mut dom, model, rig) = rig_dom();
    let saves = dom.new_instance("Folder", "Saves", Some(model));
    let a = sample_animation("Wave", true, Priority::Core);
    let anim = file::write(&mut dom, Some(saves), &a, Some(&rig));
    assert_eq!(file::read(&dom, anim).unwrap(), a);

    let hrp = child(&dom, anim, "HumanoidRootPart");
    let lower = child(&dom, hrp, "LowerTorso");
    let upper = child(&dom, lower, "UpperTorso");
    let head = child(&dom, upper, "Head");
    assert_eq!(class_name(&dom, head), "Folder");

    let position = child(&dom, head, "Position");
    assert_eq!(class_name(&dom, position), "Vector3Curve");
    for axis in ["X", "Y", "Z"] {
        assert_eq!(class_name(&dom, child(&dom, position, axis)), "FloatCurve");
    }
    let rotation = child(&dom, head, "Rotation");
    assert_eq!(class_name(&dom, rotation), "EulerRotationCurve");
    assert_eq!(
        dom.get(rotation).unwrap().properties().get("RotationOrder"),
        Some(&Variant::Enum(2))
    );
    for axis in ["X", "Y", "Z"] {
        assert_eq!(class_name(&dom, child(&dom, rotation, axis)), "FloatCurve");
    }

    let faces = child(&dom, head, "FaceControls");
    assert_eq!(class_name(&dom, faces), "Folder");
    assert_eq!(
        class_name(&dom, child(&dom, faces, "JawDrop")),
        "FloatCurve"
    );

    let arm = child(&dom, upper, "LeftUpperArm");
    assert_eq!(
        class_name(&dom, child(&dom, arm, "Rotation")),
        "EulerRotationCurve"
    );

    let keyframes = child(&dom, anim, "NamedKeyframeEvent");
    assert_eq!(class_name(&dom, keyframes), "MarkerCurve");
    assert_eq!(class_name(&dom, child(&dom, anim, "Step")), "MarkerCurve");
}

#[test]
fn rig_written_animation_survives_binary() {
    let (mut dom, model, rig) = rig_dom();
    let a = sample_animation("Wave", false, Priority::Idle);
    file::write(&mut dom, Some(model), &a, Some(&rig));
    let bytes = rbx_binary::serialize(&dom).unwrap();
    let back = rbx_binary::deserialize(&bytes).unwrap();
    let anims = of_class(&back, "CurveAnimation");
    assert_eq!(anims.len(), 1);
    assert_eq!(file::read(&back, anims[0]).unwrap(), a);
}

#[test]
fn overwrite_keeps_ref_and_parent_and_replaces_children() {
    let (mut dom, model, rig) = rig_dom();
    let saves = dom.new_instance("Folder", "Saves", Some(model));
    let first = file::write(
        &mut dom,
        Some(saves),
        &sample_animation("Wave", true, Priority::Core),
        Some(&rig),
    );

    let next = sample_animation("Nod", false, Priority::Idle);
    file::overwrite(&mut dom, first, &next, Some(&rig));

    assert_eq!(dom.parent(first), Some(saves));
    assert_eq!(dom.get(first).unwrap().name().to_string(), "Nod");
    assert_eq!(file::read(&dom, first).unwrap(), next);
    assert_eq!(
        dom.get(first).unwrap().properties().get("Loop"),
        Some(&Variant::Bool(false))
    );

    let fresh = file::write(&mut dom, Some(saves), &next, Some(&rig));
    assert_eq!(
        dom.get(first).unwrap().children().len(),
        dom.get(fresh).unwrap().children().len()
    );
    assert_eq!(file::read(&dom, fresh).unwrap(), next);
}
