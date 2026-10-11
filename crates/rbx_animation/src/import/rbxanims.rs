//! The clipboard text the Blender "Rbx Animations" addon's *Export animation*
//! produces, and that its Studio plugin pastes in.
//!
//! The text is `base64(zlib(json))`, with JSON of the form
//! `{"t": length, "kfs": [{"t": time, "kf": {"<Part1 name>": pose}}]}`. A
//! pose is the joint's `Transform` as the twelve numbers of
//! `CFrame:GetComponents()` (position, then the rotation row by row), so no
//! axis or unit conversion is needed. Newer versions of the addon may also
//! wrap those twelve as `[[…], "Style", "Direction"]` or
//! `{"components": […], "easingStyle": …, "easingDirection": …}`. They may
//! give times in frames (`export_info.time_unit`), and may add fields this
//! reader ignores (face controls, deform-bone markers).
//!
//! A full bake leaves a bone out of a frame when its value is the identity.
//! The original addon does that, so a bone the clip poses elsewhere but
//! leaves out of a frame is read as the identity there, not as "no key".

use std::io::{Read, Write};

use base64::Engine;
use serde_json::{json, Map, Value};

use crate::clip::{Clip, PoseKey};
use crate::easing::{Direction, Style};
use crate::frame::Frame;
use crate::import::ImportError;
use rbx_dom::{CFrameData, Vector3Data};

fn components(value: &Value) -> Option<[f32; 12]> {
    let list = value.as_array()?;
    if list.len() != 12 {
        return None;
    }
    let mut out = [0.; 12];
    for (slot, v) in out.iter_mut().zip(list) {
        *slot = v.as_f64()? as f32;
    }
    Some(out)
}

fn style_named(name: Option<&str>) -> Style {
    match name.unwrap_or("Linear") {
        "Constant" => Style::Constant,
        "Elastic" => Style::Elastic,
        "Cubic" => Style::Cubic,
        "CubicV2" => Style::CubicV2,
        "Bounce" => Style::Bounce,
        _ => Style::Linear,
    }
}

fn direction_named(name: Option<&str>) -> Direction {
    match name.unwrap_or("Out") {
        "In" => Direction::In,
        "InOut" => Direction::InOut,
        _ => Direction::Out,
    }
}

fn pose(value: &Value) -> Option<PoseKey> {
    let (c, style, direction) = if let Some(c) = components(value) {
        (c, None, None)
    } else if let Some(list) = value.as_array() {
        (
            components(list.first()?)?,
            list.get(1).and_then(Value::as_str),
            list.get(2).and_then(Value::as_str),
        )
    } else {
        let object = value.as_object()?;
        (
            components(object.get("components")?)?,
            object.get("easingStyle").and_then(Value::as_str),
            object.get("easingDirection").and_then(Value::as_str),
        )
    };
    let frame = Frame::from_data(&CFrameData {
        position: Vector3Data {
            x: c[0],
            y: c[1],
            z: c[2],
        },
        rotation: [c[3], c[4], c[5], c[6], c[7], c[8], c[9], c[10], c[11]],
    });
    let mut key = PoseKey::new(frame);
    key.style = style_named(style);
    key.direction = direction_named(direction);
    Some(key)
}

/// Decodes pasted text into a clip named `name`.
pub fn decode(text: &str, name: &str) -> Result<Clip, ImportError> {
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    let packed = base64::engine::general_purpose::STANDARD
        .decode(compact.as_bytes())
        .map_err(|_| ImportError::Format("not base64".into()))?;
    let mut json = String::new();
    flate2::read::ZlibDecoder::new(packed.as_slice())
        .read_to_string(&mut json)
        .map_err(|_| ImportError::Format("not zlib-compressed text".into()))?;
    let root: Value =
        serde_json::from_str(&json).map_err(|e| ImportError::Format(format!("bad JSON: {e}")))?;
    from_json(&root, name)
}

fn from_json(root: &Value, name: &str) -> Result<Clip, ImportError> {
    let frames = root
        .get("kfs")
        .and_then(Value::as_array)
        .ok_or_else(|| ImportError::Format("no \"kfs\" keyframe list".into()))?;
    let info = root.get("export_info");
    let in_frames = info
        .and_then(|i| i.get("time_unit"))
        .and_then(Value::as_str)
        == Some("frames");
    let fps = info
        .and_then(|i| i.get("fps"))
        .and_then(Value::as_f64)
        .filter(|f| *f > 0.)
        .unwrap_or(30.) as f32;
    let seconds = |t: f32| if in_frames { t / fps } else { t };

    let mut clip = Clip {
        name: name.to_owned(),
        ..Clip::default()
    };
    let mut bones: Vec<String> = Vec::new();
    for frame in frames {
        let time = seconds(frame.get("t").and_then(Value::as_f64).unwrap_or(0.) as f32);
        let poses = frame.get("kf").and_then(Value::as_object);
        for (bone, value) in poses.into_iter().flatten() {
            if bone.ends_with("_deform") {
                continue;
            }
            let key = pose(value)
                .ok_or_else(|| ImportError::Format(format!("bad pose for {bone}")))?;
            clip.set_pose(time, bone, key);
            if !bones.contains(bone) {
                bones.push(bone.clone());
            }
        }
        clip.keyframe_mut(time);
    }
    // The identity-elision rule above: fill each bone's gaps back in.
    let times: Vec<f32> = clip.keyframes.iter().map(|k| k.time).collect();
    for time in times {
        for bone in &bones {
            if clip.pose(time, bone).is_none() {
                clip.set_pose(time, bone, PoseKey::new(Frame::IDENTITY));
            }
        }
    }
    if let Some(length) = root.get("t").and_then(Value::as_f64) {
        let length = seconds(length as f32);
        if length > clip.length() + crate::clip::TIME_EPSILON {
            clip.keyframe_mut(length);
        }
    }
    clip.keyframes.retain(|k| !k.poses.is_empty() || !k.markers.is_empty() || k.time > 0.);
    Ok(clip)
}

/// Encodes `clip` in the same format, sampled at every keyframe, so it can be
/// pasted into anything that reads the addon's export.
pub fn encode(clip: &Clip) -> String {
    let kfs: Vec<Value> = clip
        .keyframes
        .iter()
        .map(|k| {
            let mut kf = Map::new();
            for (joint, key) in &k.poses {
                let d = key.cframe.to_data();
                let mut c = vec![d.position.x, d.position.y, d.position.z];
                c.extend(d.rotation);
                kf.insert(
                    joint.clone(),
                    json!([c, format!("{:?}", key.style), key.direction.label()]),
                );
            }
            json!({"t": k.time, "kf": kf})
        })
        .collect();
    let text = json!({"t": clip.length(), "kfs": kfs}).to_string();
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    // Writing into a Vec cannot fail.
    let _ = encoder.write_all(text.as_bytes());
    let packed = encoder.finish().unwrap_or_default();
    base64::engine::general_purpose::STANDARD.encode(packed)
}

#[cfg(test)]
mod tests {
    use glam::Vec3;
    use serde_json::json;

    use super::*;

    fn pack(value: &Value) -> String {
        let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
        e.write_all(value.to_string().as_bytes()).unwrap();
        base64::engine::general_purpose::STANDARD.encode(e.finish().unwrap())
    }

    #[test]
    fn reads_the_original_flat_format_and_fills_elided_identities() {
        let raised = [0, 1, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1];
        let text = pack(&json!({
            "t": 1.0,
            "kfs": [
                {"t": 0, "kf": {"Head": raised}},
                {"t": 0.5, "kf": {}},
                {"t": 1.0, "kf": {"Head": raised, "LeftHand": raised}},
            ]
        }));
        let clip = decode(&format!("{}\n", text), "Wave").unwrap();
        assert_eq!(clip.name, "Wave");
        assert_eq!(clip.keyframes.len(), 3);
        assert!(clip.pose(0., "Head").unwrap().cframe.position.abs_diff_eq(Vec3::Y, 1e-6));
        // Left out of frame 0.5: identity, not a gap.
        assert_eq!(clip.pose(0.5, "Head").unwrap().cframe, Frame::IDENTITY);
        assert_eq!(clip.pose(0., "LeftHand").unwrap().cframe, Frame::IDENTITY);
    }

    #[test]
    fn reads_wrapped_poses_with_easing_and_frame_times() {
        let c = [0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1];
        let text = pack(&json!({
            "t": 30,
            "export_info": {"fps": 30, "time_unit": "frames"},
            "kfs": [
                {"t": 0, "kf": {"Head": [c, "CubicV2", "InOut"]}},
                {"t": 15, "kf": {"Head": {"components": c, "easingStyle": "Bounce"},
                                 "Head_deform": c}},
            ]
        }));
        let clip = decode(&text, "x").unwrap();
        let first = clip.pose(0., "Head").unwrap();
        assert_eq!((first.style, first.direction), (Style::CubicV2, Direction::InOut));
        let second = clip.pose(0.5, "Head").unwrap();
        assert_eq!((second.style, second.direction), (Style::Bounce, Direction::Out));
        assert_eq!(clip.joints(), ["Head"]);
        assert!((clip.length() - 1.).abs() < 1e-5);
    }

    #[test]
    fn encode_round_trips() {
        let mut clip = Clip::default();
        let mut key = PoseKey::new(Frame::new(Vec3::new(1., 2., 3.), glam::Quat::from_rotation_x(0.4)));
        key.style = Style::Elastic;
        key.direction = Direction::Out;
        clip.set_pose(0.25, "RightUpperArm", key);
        let back = decode(&encode(&clip), "Animation").unwrap();
        let read = back.pose(0.25, "RightUpperArm").unwrap();
        assert!(read.cframe.approx_eq(&key.cframe, 1e-5));
        assert_eq!(read.style, Style::Elastic);
    }

    #[test]
    fn rejects_garbage() {
        assert!(decode("not base64 !!", "x").is_err());
        assert!(decode(&base64::engine::general_purpose::STANDARD.encode(b"plain"), "x").is_err());
        assert!(decode(&pack(&json!({"nope": 1})), "x").is_err());
    }
}
