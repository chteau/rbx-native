//! Published animations, read the way the editor's AnimationId import reads
//! them. The fixtures are the bytes Roblox's asset delivery served:
//! the default R15 walk, run and idle from the `Animate` script
//! (507777826, 507767714, 507766951) and three catalog emotes'
//! `CurveAnimation`s (85200395117668, 71620783740586, 105317314539586).
//! Expected counts were taken from the files independently of this crate.

use rbx_animation::curve::{file, values};
use rbx_animation::import::asset::clip_from_model;
use rbx_animation::{Clip, Priority};
use rbx_dom::{Variant, WeakDom};

fn fixture(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn pose_count(clip: &Clip) -> usize {
    clip.keyframes.iter().map(|k| k.poses.len()).sum()
}

#[test]
fn default_r15_animations_import() {
    // (file, name, keyframes, poses, joints, length)
    let cases = [
        ("walk_507777826.rbxm", "NewRun", 17, 255, 15, 0.666672),
        ("run_507767714.rbxm", "ImportedAnimation", 14, 210, 15, 0.65),
        ("idle_507766951.rbxm", "", 49, 735, 15, 2.0417),
    ];
    for (file, name, keyframes, poses, joints, length) in cases {
        let clip = clip_from_model(&fixture(file), "").unwrap();
        if !name.is_empty() {
            assert_eq!(clip.name, name, "{file}");
        }
        assert_eq!(clip.keyframes.len(), keyframes, "{file}");
        assert_eq!(pose_count(&clip), poses, "{file}");
        assert_eq!(clip.joints().len(), joints, "{file}");
        assert!((clip.length() - length).abs() < 1e-3, "{file}");
        assert!(clip.looped, "{file}");
        assert_eq!(clip.priority, Priority::Core, "{file}");
    }
}

fn curve_dom(file: &str) -> WeakDom {
    rbx_binary::deserialize(&fixture(file)).unwrap()
}

const EMOTES: [&str; 3] = [
    "emote_curve_85200395117668.rbxm",
    "emote_curve_71620783740586.rbxm",
    "emote_curve_105317314539586.rbxm",
];

#[test]
fn every_values_and_times_writes_back_byte_for_byte() {
    let mut checked = 0;
    for file in EMOTES {
        let dom = curve_dom(file);
        let mut stack = dom.root_refs().to_vec();
        while let Some(r) = stack.pop() {
            let instance = dom.get(r).unwrap();
            stack.extend(instance.children());
            let bytes = match instance.properties().get("ValuesAndTimes") {
                Some(Variant::String(s)) => s.as_bytes().to_vec(),
                Some(Variant::Unknown { raw, .. }) => raw.clone(),
                _ => continue,
            };
            let back = match instance.class() {
                "FloatCurve" => values::write_float(&values::read_float(&bytes).unwrap()),
                "MarkerCurve" => values::write_markers(&values::read_markers(&bytes).unwrap()),
                other => panic!("{file}: ValuesAndTimes on a {other}"),
            };
            assert_eq!(back, bytes, "{file}: {}", instance.name());
            checked += 1;
        }
    }
    // 90 + 68 + 90 FloatCurves and one MarkerCurve each.
    assert_eq!(checked, 251);
}

#[test]
fn catalog_emote_curves_read() {
    // (file, joints, keys over all joint curves, length, face channels)
    let cases = [
        (EMOTES[0], 15, 1026, 1.7, 0),
        (EMOTES[1], 11, 60, 10.0, 2),
        (EMOTES[2], 15, 991, 0.6667, 0),
    ];
    for (file, joints, keys, length, faces) in cases {
        let dom = curve_dom(file);
        let root = dom.root_refs()[0];
        let animation = file::read(&dom, root).unwrap();
        let curves = &animation.curves;
        assert_eq!(curves.joints.len(), joints, "{file}");
        let count: usize = curves
            .joints
            .values()
            .flat_map(|j| j.position.iter().chain(&j.rotation))
            .map(|c| c.keys.len())
            .sum();
        assert_eq!(count, keys, "{file}");
        assert_eq!(animation.faces.len(), faces, "{file}");
        let end = curves.length().max(
            animation
                .faces
                .values()
                .map(|c| c.length())
                .fold(0., f32::max),
        );
        assert!((end - length).abs() < 1e-3, "{file}: {end}");
    }

    // Head pitched 30° at the start of "MM2 Fake Death Pose (Sit)2".
    let dom = curve_dom(EMOTES[1]);
    let animation = file::read(&dom, dom.root_refs()[0]).unwrap();
    let head = &animation.curves.joints["Head"];
    assert!((head.rotation[0].keys[0].value - 30f32.to_radians()).abs() < 1e-6);
    // The one keyframe name, "End", at 4080 ticks.
    let dom = curve_dom(EMOTES[0]);
    let animation = file::read(&dom, dom.root_refs()[0]).unwrap();
    assert_eq!(animation.keyframe_names, [(1.7, "End".to_owned())]);
}

#[test]
fn catalog_emotes_import_as_clips() {
    for file in EMOTES {
        let clip = clip_from_model(&fixture(file), "Emote").unwrap();
        assert!(clip.joints().len() >= 11, "{file}");
        assert!(pose_count(&clip) > 0, "{file}");
    }
    let clip = clip_from_model(&fixture(EMOTES[0]), "Emote").unwrap();
    let end = clip.keyframes.iter().find(|k| k.name == "End").unwrap();
    assert!((end.time - 1.7).abs() < 1e-4);
    let clip = clip_from_model(&fixture(EMOTES[1]), "Emote").unwrap();
    assert_eq!(clip.face_channels(), ["LeftEyeClosed", "RightEyeClosed"]);
}
