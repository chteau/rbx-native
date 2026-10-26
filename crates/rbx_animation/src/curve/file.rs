//! Reads and writes a saved `CurveAnimation`. The tree is the one catalog emotes ship
//! with: a `Folder` per joint, nested the way the rig is (`HumanoidRootPart`
//! › `LowerTorso` › …) and named after the joint's `Part1`; in each, a
//! `Vector3Curve` "Position" and an `EulerRotationCurve` "Rotation" (with
//! its `RotationOrder`), each holding `FloatCurve`s X, Y and Z. Face channels
//! are `FloatCurve`s in a `FaceControls` folder under the head's folder.
//! Keyframe names travel as a `MarkerCurve` named "NamedKeyframeEvent";
//! any other `MarkerCurve` is a marker named after the curve, wherever in
//! the tree it sits. A quaternion `RotationCurve` reads as Euler channels
//! sampled from it (see [`rotation`](super::rotation)), and is written back
//! as an `EulerRotationCurve`.

use std::collections::BTreeMap;

use rbx_dom::{Ref, Variant, WeakDom};

use super::values::{self, seconds};
use super::{bake_times, to_clip, CurveClip, FloatCurve, Interpolation, JointCurves};
use super::{rotation, RotationOrder};
use crate::clip::{Clip, Marker, Priority};
use crate::easing::Style;
use crate::facs::FaceKey;
use crate::Rig;

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
                if let Some(b) = bytes(dom, child) {
                    joint.order = RotationOrder::XYZ;
                    joint.rotation = rotation::to_euler(&values::read_rotation(b)?);
                }
            }
            "MarkerCurve" => markers(dom, child, out)?,
            _ => {}
        }
    }
    if !joint.is_empty() {
        out.curves.joints.insert(instance.name().to_owned(), joint);
    }
    Ok(())
}

fn markers(dom: &WeakDom, curve: Ref, out: &mut CurveAnimation) -> Result<(), String> {
    let (Some(c), Some(b)) = (dom.get(curve), bytes(dom, curve)) else {
        return Ok(());
    };
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
            "MarkerCurve" => markers(dom, child, &mut out)?,
            _ => {}
        }
    }
    out.keyframe_names.sort_by(|a, b| a.0.total_cmp(&b.0));
    Ok(out)
}

fn set(dom: &mut WeakDom, of: Ref, name: &str, value: Variant) {
    let _ = dom.set_property(of, name, value);
}

/// Root-first part names from the rig's root down to `joint`.
fn chain(rig: &Rig, joint: &str) -> Vec<String> {
    let mut out = vec![joint.to_owned()];
    let mut at = joint.to_owned();
    while let Some(parent) = rig.parent_name(&at) {
        if out.iter().any(|n| n == parent) {
            break;
        }
        out.push(parent.to_owned());
        at = parent.to_owned();
    }
    out.reverse();
    out
}

/// The folder for `joint`, made (with its ancestors) once and shared.
fn joint_folder(
    dom: &mut WeakDom,
    made: &mut Vec<(String, Ref)>,
    animation: Ref,
    rig: Option<&Rig>,
    joint: &str,
) -> Ref {
    let path = match rig {
        Some(rig) if rig.joint_for(joint).is_some() => chain(rig, joint),
        _ => vec![joint.to_owned()],
    };
    let mut under = animation;
    for name in &path {
        if let Some((_, existing)) = made.iter().find(|(n, _)| n == name) {
            under = *existing;
            continue;
        }
        under = dom.new_instance("Folder", name, Some(under));
        made.push((name.clone(), under));
    }
    under
}

fn write_curve(dom: &mut WeakDom, parent: Ref, name: &str, curve: &FloatCurve) {
    let c = dom.new_instance("FloatCurve", name, Some(parent));
    let raw = values::write_float(&values::from_curve(curve));
    set(
        dom,
        c,
        "ValuesAndTimes",
        Variant::Unknown { type_id: 0x01, raw },
    );
}

fn write_axes(dom: &mut WeakDom, parent: Ref, curves: &[FloatCurve; 3]) {
    for (name, curve) in ["X", "Y", "Z"].into_iter().zip(curves) {
        write_curve(dom, parent, name, curve);
    }
}

fn write_markers(dom: &mut WeakDom, parent: Ref, name: &str, items: &[(f32, &str)]) {
    let mut items = items.to_vec();
    items.sort_by(|a, b| a.0.total_cmp(&b.0));
    let raw: Vec<values::RawMarker> = items
        .iter()
        .map(|(t, v)| values::RawMarker {
            ticks: (t * values::TICKS_PER_SECOND).round().max(0.) as u32,
            value: v.as_bytes().to_vec(),
        })
        .collect();
    let m = dom.new_instance("MarkerCurve", name, Some(parent));
    set(
        dom,
        m,
        "ValuesAndTimes",
        Variant::Unknown {
            type_id: 0x01,
            raw: values::write_markers(&raw),
        },
    );
}

fn fill(dom: &mut WeakDom, target: Ref, animation: &CurveAnimation, rig: Option<&Rig>) {
    let mut made: Vec<(String, Ref)> = Vec::new();
    for (joint, curves) in &animation.curves.joints {
        if curves.is_empty() {
            continue;
        }
        let folder = joint_folder(dom, &mut made, target, rig, joint);
        let position = dom.new_instance("Vector3Curve", "Position", Some(folder));
        write_axes(dom, position, &curves.position);
        let rotation = dom.new_instance("EulerRotationCurve", "Rotation", Some(folder));
        set(
            dom,
            rotation,
            "RotationOrder",
            Variant::Enum(curves.order.value()),
        );
        write_axes(dom, rotation, &curves.rotation);
    }
    if !animation.faces.is_empty() {
        let head = joint_folder(dom, &mut made, target, rig, "Head");
        let faces = dom.new_instance("Folder", "FaceControls", Some(head));
        for (channel, curve) in &animation.faces {
            write_curve(dom, faces, channel, curve);
        }
    }
    if !animation.keyframe_names.is_empty() {
        let items: Vec<(f32, &str)> = animation
            .keyframe_names
            .iter()
            .map(|(t, n)| (*t, n.as_str()))
            .collect();
        write_markers(dom, target, KEYFRAME_NAMES, &items);
    }
    let mut groups: BTreeMap<&str, Vec<(f32, &str)>> = BTreeMap::new();
    for (t, m) in &animation.curves.markers {
        groups
            .entry(m.name.as_str())
            .or_default()
            .push((*t, m.value.as_str()));
    }
    for (name, items) in &groups {
        write_markers(dom, target, name, items);
    }
}

fn set_settings(dom: &mut WeakDom, target: Ref, animation: &CurveAnimation) {
    set(dom, target, "Loop", Variant::Bool(animation.curves.looped));
    set(
        dom,
        target,
        "Priority",
        Variant::Enum(animation.curves.priority.value()),
    );
}

/// Writes `animation` as a new `CurveAnimation` under `parent` and returns it.
/// With a rig, joint folders nest along it; without one (or for a joint the
/// rig lacks) they sit directly under the animation.
pub fn write(
    dom: &mut WeakDom,
    parent: Option<Ref>,
    animation: &CurveAnimation,
    rig: Option<&Rig>,
) -> Ref {
    let target = dom.new_instance("CurveAnimation", &animation.curves.name, parent);
    set_settings(dom, target, animation);
    fill(dom, target, animation, rig);
    target
}

/// Replaces the children of an existing `CurveAnimation` with `animation`,
/// keeping the instance itself (its referent and parent).
pub fn overwrite(dom: &mut WeakDom, target: Ref, animation: &CurveAnimation, rig: Option<&Rig>) {
    let old: Vec<Ref> = dom
        .get(target)
        .map(|i| i.children().to_vec())
        .unwrap_or_default();
    for child in old {
        dom.remove(child);
    }
    let _ = dom.set_name(target, &animation.curves.name);
    set_settings(dom, target, animation);
    fill(dom, target, animation, rig);
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
