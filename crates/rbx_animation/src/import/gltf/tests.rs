use base64::Engine;
use glam::{Quat, Vec3};
use serde_json::json;

use super::*;

/// A three-node chain whose last node turns a quarter about Z over a second,
/// with its buffer inlined as a data URI.
fn file(cubic: bool) -> Vec<u8> {
    let turn = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
    let mut floats: Vec<f32> = vec![0., 1.];
    let keys = [Quat::IDENTITY, turn];
    for q in keys {
        if cubic {
            floats.extend([0.; 4]);
        }
        floats.extend(q.to_array());
        if cubic {
            floats.extend([0.; 4]);
        }
    }
    let bytes: Vec<u8> = floats.iter().flat_map(|f| f.to_le_bytes()).collect();
    let uri = format!(
        "data:application/octet-stream;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&bytes)
    );
    let count = if cubic { 6 } else { 2 };
    json!({
        "asset": {"version": "2.0"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [
            {"name": "HumanoidRootPart", "children": [1], "translation": [0, 3, 0]},
            {"name": "Torso", "children": [2], "scale": [2, 2, 2]},
            {"name": "Right Arm", "translation": [1, 0.5, 0]},
        ],
        "buffers": [{"byteLength": bytes.len(), "uri": uri}],
        "bufferViews": [
            {"buffer": 0, "byteOffset": 0, "byteLength": 8},
            {"buffer": 0, "byteOffset": 8, "byteLength": bytes.len() - 8},
        ],
        "accessors": [
            {"bufferView": 0, "componentType": 5126, "count": 2, "type": "SCALAR",
             "min": [0.0], "max": [1.0]},
            {"bufferView": 1, "componentType": 5126, "count": count, "type": "VEC4"},
        ],
        "animations": [{
            "name": "Raise",
            "samplers": [{"input": 0, "output": 1,
                          "interpolation": if cubic { "CUBICSPLINE" } else { "LINEAR" }}],
            "channels": [{"sampler": 0, "target": {"node": 2, "path": "rotation"}}],
        }],
    })
    .to_string()
    .into_bytes()
}

#[test]
fn reads_the_hierarchy_and_keys() {
    let bytes = file(false);
    assert_eq!(animation_names(&bytes).unwrap(), ["Raise"]);
    let s = read(&bytes, 0, &|_| None).unwrap();
    let names: Vec<&str> = s.bones.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, ["HumanoidRootPart", "Torso", "Right Arm"]);
    assert_eq!(s.bones[2].parent, Some(1));
    assert_eq!(s.bones[1].scale, 2.);
    assert!(s.bones[0].rest.position.abs_diff_eq(Vec3::new(0., 3., 0.), 1e-6));
    assert_eq!(s.bones[2].keys.len(), 2);
    let half = s.local(2, 0.5);
    assert!((half.rotation.angle_between(Quat::IDENTITY) - std::f32::consts::FRAC_PI_4).abs() < 1e-4);
    // An unanimated translation keeps the rest value.
    assert!(half.position.abs_diff_eq(Vec3::new(1., 0.5, 0.), 1e-6));
}

#[test]
fn cubic_splines_keep_their_values() {
    let s = read(&file(true), 0, &|_| None).unwrap();
    let end = s.local(2, 1.);
    assert!((end.rotation.angle_between(Quat::IDENTITY) - std::f32::consts::FRAC_PI_2).abs() < 1e-4);
}

#[test]
fn errors_are_reported() {
    assert!(read(b"nonsense", 0, &|_| None).is_err());
    assert!(read(&file(false), 3, &|_| None).is_err());
}
