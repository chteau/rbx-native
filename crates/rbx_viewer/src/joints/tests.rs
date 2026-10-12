use rbx_dom::{CFrameData, Change, Ref, Variant, Vector3Data, WeakDom};

use super::JointSolver;

const IDENTITY: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

fn at(x: f32, y: f32, z: f32) -> Variant {
    Variant::CFrame(CFrameData {
        position: Vector3Data { x, y, z },
        rotation: IDENTITY,
    })
}

fn position(dom: &WeakDom, part: Ref) -> [f32; 3] {
    match dom.get(part).unwrap().properties().get("CFrame") {
        Some(Variant::CFrame(c)) => [c.position.x, c.position.y, c.position.z],
        _ => panic!("no CFrame"),
    }
}

fn near(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-4)
}

/// A torso with an arm on a `Motor6D` one stud along x, a handle welded one
/// stud along z from the arm, and a crate standing apart.
struct Rig {
    dom: WeakDom,
    torso: Ref,
    arm: Ref,
    handle: Ref,
    apart: Ref,
    shoulder: Ref,
}

fn rig() -> Rig {
    let mut dom = WeakDom::new();
    let model = dom.new_instance("Model", "Rig", None);
    let torso = dom.new_instance("Part", "Torso", Some(model));
    let arm = dom.new_instance("Part", "Arm", Some(model));
    let handle = dom.new_instance("Part", "Handle", Some(model));
    let apart = dom.new_instance("Part", "Crate", None);
    let shoulder = dom.new_instance("Motor6D", "Shoulder", Some(torso));
    let weld = dom.new_instance("Weld", "Grip", Some(arm));
    dom.set_property(torso, "CFrame", at(0.0, 0.0, 0.0))
        .unwrap();
    dom.set_property(arm, "CFrame", at(1.0, 0.0, 0.0)).unwrap();
    dom.set_property(handle, "CFrame", at(1.0, 0.0, 1.0))
        .unwrap();
    dom.set_property(apart, "CFrame", at(9.0, 0.0, 0.0))
        .unwrap();
    dom.set_property(shoulder, "Part0", Variant::Ref(torso))
        .unwrap();
    dom.set_property(shoulder, "Part1", Variant::Ref(arm))
        .unwrap();
    dom.set_property(shoulder, "C0", at(1.0, 0.0, 0.0)).unwrap();
    dom.set_property(weld, "Part0", Variant::Ref(arm)).unwrap();
    dom.set_property(weld, "Part1", Variant::Ref(handle))
        .unwrap();
    dom.take_changes();
    Rig {
        dom,
        torso,
        arm,
        handle,
        apart,
        shoulder,
    }
}

fn transform(rig: &mut Rig, y: f32) -> Vec<Change> {
    rig.dom
        .set_property(rig.shoulder, "Transform", at(0.0, y, 0.0))
        .unwrap();
    rig.dom.take_changes()
}

#[test]
fn a_transform_poses_the_part_its_joint_drives_and_what_is_welded_to_it() {
    let mut rig = rig();
    let mut solver = JointSolver::new();
    let changes = transform(&mut rig, 2.0);
    let wrote = solver.solve(&mut rig.dom, &changes);
    assert!(near(position(&rig.dom, rig.arm), [1.0, 2.0, 0.0]));
    assert!(near(position(&rig.dom, rig.handle), [1.0, 2.0, 1.0]));
    assert!(near(position(&rig.dom, rig.torso), [0.0, 0.0, 0.0]));
    assert!(near(position(&rig.dom, rig.apart), [9.0, 0.0, 0.0]));
    let named: Vec<Ref> = wrote
        .iter()
        .map(|c| match c {
            Change::Property { referent, name } if name == "CFrame" => *referent,
            other => panic!("unexpected {other:?}"),
        })
        .collect();
    assert_eq!(named.len(), 2);
    assert!(named.contains(&rig.arm) && named.contains(&rig.handle));
}

#[test]
fn a_transform_back_at_identity_puts_the_parts_back_at_rest() {
    let mut rig = rig();
    let mut solver = JointSolver::new();
    let changes = transform(&mut rig, 2.0);
    solver.solve(&mut rig.dom, &changes);
    let changes = transform(&mut rig, 0.0);
    solver.solve(&mut rig.dom, &changes);
    assert!(near(position(&rig.dom, rig.arm), [1.0, 0.0, 0.0]));
    assert!(near(position(&rig.dom, rig.handle), [1.0, 0.0, 1.0]));
    assert!(!solver.posing());

    // And a removed `Transform` is identity too.
    let changes = transform(&mut rig, 3.0);
    solver.solve(&mut rig.dom, &changes);
    rig.dom.remove_property(rig.shoulder, "Transform").unwrap();
    let changes = rig.dom.take_changes();
    solver.solve(&mut rig.dom, &changes);
    assert!(near(position(&rig.dom, rig.arm), [1.0, 0.0, 0.0]));
    assert!(!solver.posing());
}

#[test]
fn a_rest_the_editor_moves_carries_the_pose_with_it() {
    let mut rig = rig();
    let mut solver = JointSolver::new();
    let changes = transform(&mut rig, 2.0);
    solver.solve(&mut rig.dom, &changes);
    // The editor moved the whole rig five studs up: its snapshot lands
    // rest values on every part, posed ones included.
    for (part, x, z) in [
        (rig.torso, 0.0, 0.0),
        (rig.arm, 1.0, 0.0),
        (rig.handle, 1.0, 1.0),
    ] {
        rig.dom.set_property(part, "CFrame", at(x, 5.0, z)).unwrap();
    }
    let changes = rig.dom.take_changes();
    solver.solve(&mut rig.dom, &changes);
    assert!(near(position(&rig.dom, rig.arm), [1.0, 7.0, 0.0]));
    assert!(near(position(&rig.dom, rig.handle), [1.0, 7.0, 1.0]));
    let changes = transform(&mut rig, 0.0);
    solver.solve(&mut rig.dom, &changes);
    assert!(near(position(&rig.dom, rig.arm), [1.0, 5.0, 0.0]));
    assert!(near(position(&rig.dom, rig.handle), [1.0, 5.0, 1.0]));
}

#[test]
fn a_place_with_no_transform_is_left_alone() {
    let mut rig = rig();
    let mut solver = JointSolver::new();
    rig.dom
        .set_property(rig.arm, "CFrame", at(4.0, 0.0, 0.0))
        .unwrap();
    let changes = rig.dom.take_changes();
    assert!(solver.solve(&mut rig.dom, &changes).is_empty());
    assert!(near(position(&rig.dom, rig.arm), [4.0, 0.0, 0.0]));
}
