use fbxcel::low::FbxVersion;
use fbxcel::writer::v7400::binary::{FbxFooter, Writer};
use glam::{Quat, Vec3};

use super::*;

fn same(a: Quat, b: Quat) -> bool {
    a.dot(b).abs() > 1. - 1e-6
}

type W<'a> = Writer<std::io::Cursor<&'a mut Vec<u8>>>;

fn property(w: &mut W<'_>, name: &str, values: [f64; 3]) {
    let mut a = w.new_node("P").unwrap();
    a.append_string_direct(name).unwrap();
    a.append_string_direct(name).unwrap();
    a.append_string_direct("").unwrap();
    a.append_string_direct("A").unwrap();
    for v in values {
        a.append_f64(v).unwrap();
    }
    drop(a);
    w.close_node().unwrap();
}

fn model(w: &mut W<'_>, id: i64, name: &str, translation: [f64; 3], pre: [f64; 3]) {
    let mut a = w.new_node("Model").unwrap();
    a.append_i64(id).unwrap();
    a.append_string_direct(&format!("{name}\0\u{1}Model")).unwrap();
    a.append_string_direct("LimbNode").unwrap();
    drop(a);
    w.new_node("Properties70").unwrap();
    property(w, "Lcl Translation", translation);
    property(w, "PreRotation", pre);
    w.close_node().unwrap();
    w.close_node().unwrap();
}

fn connect(w: &mut W<'_>, kind: &str, child: i64, parent: i64, prop: Option<&str>) {
    let mut a = w.new_node("C").unwrap();
    a.append_string_direct(kind).unwrap();
    a.append_i64(child).unwrap();
    a.append_i64(parent).unwrap();
    if let Some(p) = prop {
        a.append_string_direct(p).unwrap();
    }
    drop(a);
    w.close_node().unwrap();
}

/// Root → Torso → Right Arm, the arm turning 90° about Z over one second,
/// its rest carrying a pre-rotation the way Blender's bones do.
fn file() -> Vec<u8> {
    let mut out = Vec::new();
    let mut w = Writer::new(std::io::Cursor::new(&mut out), FbxVersion::V7_4).unwrap();
    w.new_node("Objects").unwrap();
    model(&mut w, 1, "HumanoidRootPart", [0., 300., 0.], [0.; 3]);
    model(&mut w, 2, "Torso", [0., 0., 0.], [0.; 3]);
    model(&mut w, 3, "Right Arm", [100., 50., 0.], [90., 0., 0.]);
    {
        let mut a = w.new_node("AnimationCurveNode").unwrap();
        a.append_i64(10).unwrap();
        a.append_string_direct("R\0\u{1}AnimCurveNode").unwrap();
    }
    w.close_node().unwrap();
    {
        let mut a = w.new_node("AnimationCurve").unwrap();
        a.append_i64(20).unwrap();
        a.append_string_direct("\0\u{1}AnimCurve").unwrap();
    }
    {
        let mut a = w.new_node("KeyTime").unwrap();
        a.append_arr_i64_from_iter(None, [0, TICKS as i64]).unwrap();
    }
    w.close_node().unwrap();
    {
        let mut a = w.new_node("KeyValueFloat").unwrap();
        a.append_arr_f32_from_iter(None, [0., 90.]).unwrap();
    }
    w.close_node().unwrap();
    w.close_node().unwrap();
    w.close_node().unwrap();

    w.new_node("Connections").unwrap();
    connect(&mut w, "OO", 2, 1, None);
    connect(&mut w, "OO", 3, 2, None);
    connect(&mut w, "OP", 10, 3, Some("Lcl Rotation"));
    connect(&mut w, "OP", 20, 10, Some("d|Z"));
    w.close_node().unwrap();
    w.finalize_and_flush(&FbxFooter::default()).unwrap();
    out
}

#[test]
fn reads_models_hierarchy_and_curves() {
    let s = read(&file()).unwrap();
    let names: Vec<&str> = s.bones.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, ["HumanoidRootPart", "Torso", "Right Arm"]);
    assert_eq!(s.bones[2].parent, Some(1));
    assert!(s.bones[0].rest.position.abs_diff_eq(Vec3::new(0., 300., 0.), 1e-4));
    let pre = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
    assert!(same(s.bones[2].rest.rotation, pre));
    let end = s.local(2, 1.);
    let expected = pre * Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
    assert!(same(end.rotation, expected), "{end:?}");
    assert!((s.local(2, 0.5).rotation.angle_between(pre) - std::f32::consts::FRAC_PI_4).abs() < 1e-4);
}

#[test]
fn euler_orders_apply_first_letter_first() {
    let d = Vec3::new(90., 90., 0.);
    let (x, y) = (
        Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
        Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
    );
    assert!(same(euler(d, 0), y * x));
    assert!(same(euler(d, 3), x * y));
}

#[test]
fn rejects_non_fbx() {
    assert!(read(b"; FBX 7.4.0 project file").is_err());
}
