//! Reading a saved `CurveAnimation`. The tree is the one catalog emotes ship
//! with: a `Folder` per joint, nested the way the rig is (`HumanoidRootPart`
//! › `LowerTorso` › …) and named after the joint's `Part1`; in each, a
//! `Vector3Curve` "Position" and an `EulerRotationCurve` "Rotation" (with
//! its `RotationOrder`), each holding `FloatCurve`s X, Y and Z. Face channels
//! are `FloatCurve`s in a `FaceControls` folder under the head's folder.
//! Keyframe names travel as a `MarkerCurve` named "NamedKeyframeEvent";
//! any other `MarkerCurve` is a marker named after the curve.

use std::collections::BTreeMap;

use rbx_dom::{Ref, Variant, WeakDom};

use super::values::{self, seconds};
use super::RotationOrder;
use super::{bake_times, to_clip, CurveClip, FloatCurve, Interpolation, JointCurves};
use crate::clip::{Clip, Marker, Priority};
use crate::easing::Style;
use crate::facs::FaceKey;

/// The `MarkerCurve` that carries `Keyframe` names.
pub const KEYFRAME_NAMES: &str = "NamedKeyframeEvent";

/// Everything a `CurveAnimation` holds that the editor can use.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CurveAnimation {
    pub curves: CurveClip,
    /// By `FaceControls` property name.
    pub faces: BTreeMap<String, FloatCurve>,
    /// Sorted by time.
    pub keyframe_names: Vec<(f32, String)>,
}

fn bytes(dom: &WeakDom, of: Ref) -> Option<&[u8]> {
    match dom.get(of)?.properties().get("ValuesAndTimes")? {
        Variant::String(s) => Some(s.as_bytes()),
        Variant::Unknown { raw, .. } => Some(raw),
        _ => None,
    }
}

fn float_curve(dom: &WeakDom, of: Ref) -> Result<FloatCurve, String> {
    match bytes(dom, of) {
        Some(b) => Ok(values::to_curve(&values::read_float(b)?)),
        None => Ok(FloatCurve::default()),
    }
}

fn axes(dom: &WeakDom, of: Ref, into: &mut [FloatCurve; 3]) -> Result<(), String> {
    for &child in dom.get(of).map(|i| i.children()).unwrap_or_default() {
        let Some(instance) = dom.get(child) else {
            continue;
        };
        let axis = match instance.name() {
            "X" => 0,
            "Y" => 1,
            "Z" => 2,
            _ => continue,
        };
        if instance.class() == "FloatCurve" {
            into[axis] = float_curve(dom, child)?;
        }
    }
    Ok(())
}

fn folder(dom: &WeakDom, of: Ref, out: &mut CurveAnimation) -> Result<(), String> {
    let Some(instance) = dom.get(of) else {
        return Ok(());
    };
    let mut joint = JointCurves::default();
    for &child in instance.children() {
        let Some(c) = dom.get(child) else { continue };
        match c.class() {
            "Folder" if c.name() == "FaceControls" => {
                for &face in c.children() {
                    if let Some(f) = dom.get(face).filter(|f| f.class() == "FloatCurve") {
                        out.faces
                            .insert(f.name().to_owned(), float_curve(dom, face)?);
                    }
                }
            }
            "Folder" => folder(dom, child, out)?,
            "Vector3Curve" => axes(dom, child, &mut joint.position)?,
            "EulerRotationCurve" => {
                if let Some(Variant::Enum(v)) = c.properties().get("RotationOrder") {
                    joint.order = RotationOrder::from_value(*v);
                }
                axes(dom, child, &mut joint.rotation)?;
            }
            "RotationCurve" => {
                return Err(format!(
                    "{} uses a quaternion RotationCurve, which the editor cannot read yet",
                    instance.name()
                ))
            }
            _ => {}
        }
    }
    if !joint.is_empty() {
        out.curves.joints.insert(instance.name().to_owned(), joint);
    }
    Ok(())
}

pub fn read(dom: &WeakDom, animation: Ref) -> Result<CurveAnimation, String> {
    let instance = dom.get(animation).ok_or("no such CurveAnimation")?;
    let props = instance.properties();
    let mut out = CurveAnimation {
        curves: CurveClip {
            name: instance.name().to_owned(),
            looped: matches!(props.get("Loop"), Some(Variant::Bool(true))),
            priority: match props.get("Priority") {
                Some(Variant::Enum(v)) => Priority::from_value(*v),
                _ => Priority::Core,
            },
            ..CurveClip::default()
        },
        ..CurveAnimation::default()
    };
    for &child in instance.children() {
        let Some(c) = dom.get(child) else { continue };
        match c.class() {
            "Folder" => folder(dom, child, &mut out)?,
            "MarkerCurve" => {
                let Some(b) = bytes(dom, child) else { continue };
                for m in values::read_markers(b)? {
                    let value = String::from_utf8_lossy(&m.value).into_owned();
                    let time = seconds(m.ticks);
                    if c.name() == KEYFRAME_NAMES {
                        out.keyframe_names.push((time, value));
                    } else {
                        let name = c.name().to_owned();
                        out.curves.add_marker(time, Marker { name, value });
                    }
                }
            }
            _ => {}
        }
    }
    out.keyframe_names.sort_by(|a, b| a.0.total_cmp(&b.0));
    Ok(out)
}

/// Bakes into a `KeyframeSequence` clip: joints through [`to_clip`], each face
/// channel keyed where its curve is, with cubic segments sampled at `fps`.
pub fn bake(animation: &CurveAnimation, fps: f32) -> Clip {
    let mut clip = to_clip(&animation.curves, fps);
    for (channel, curve) in &animation.faces {
        for (i, key) in curve.keys.iter().enumerate() {
            let mut face = FaceKey::new(key.value);
            face.style = match key.interpolation {
                Interpolation::Constant => Style::Constant,
                _ => Style::Linear,
            };
            clip.set_face(key.time, channel, face);
            let Some(next) = curve.keys.get(i + 1) else {
                continue;
            };
            if key.interpolation == Interpolation::Cubic {
                for t in bake_times(key.time, next.time, fps) {
                    let value = curve.eval(t).unwrap_or(key.value);
                    clip.set_face(t, channel, FaceKey::new(value));
                }
            }
        }
    }
    for (time, name) in &animation.keyframe_names {
        clip.keyframe_mut(*time).name = name.clone();
    }
    clip
}
