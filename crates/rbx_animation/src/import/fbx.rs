//! Reads a skeletal animation from binary FBX (7.x), the format Roblox's own
//! importer takes and Blender's FBX exporter writes.
//!
//! Every `Model` node is a bone, parented by the file's `OO` connections. A
//! bone's local transform is `T · Rpre · R · Rpost⁻¹`, which is FBX's full
//! formula with its pivots and offsets left out (Blender writes them as
//! zero). Euler angles are in degrees, in the order `RotationOrder` names,
//! defaulting to XYZ (X applied first). Animated channels are read from every
//! `AnimationCurveNode` bound to a model's `Lcl Translation` or
//! `Lcl Rotation`, whichever stack it belongs to, so a file should carry one
//! take. Units and up axis are not read: retargeting fits them from the rig.

use std::collections::HashMap;

use fbxcel::low::v7400::AttributeValue;
use fbxcel::tree::any::AnyTree;
use fbxcel::tree::v7400::NodeHandle;
use glam::{Quat, Vec3};

use crate::frame::Frame;
use crate::import::skeleton::{Bone, Skeleton};
use crate::import::ImportError;

/// FBX time is in these ticks per second.
const TICKS: f64 = 46_186_158_000.;

fn number(value: &AttributeValue) -> Option<f64> {
    match value {
        AttributeValue::F64(v) => Some(*v),
        AttributeValue::F32(v) => Some(f64::from(*v)),
        AttributeValue::I32(v) => Some(f64::from(*v)),
        AttributeValue::I64(v) => Some(*v as f64),
        _ => None,
    }
}

/// `Properties70` as name → numbers.
fn properties(node: NodeHandle<'_>) -> HashMap<String, Vec<f64>> {
    let mut out = HashMap::new();
    for list in node.children_by_name("Properties70") {
        for p in list.children_by_name("P") {
            let attrs = p.attributes();
            let Some(name) = attrs.first().and_then(|a| a.get_string()) else {
                continue;
            };
            let values = attrs.iter().skip(4).filter_map(number).collect();
            out.insert(name.to_owned(), values);
        }
    }
    out
}

fn vec3(values: Option<&Vec<f64>>) -> Option<Vec3> {
    let v = values?;
    (v.len() >= 3).then(|| Vec3::new(v[0] as f32, v[1] as f32, v[2] as f32))
}

/// FBX `EOrder` (0 = XYZ … 5 = ZYX, the first letter applied first).
fn euler(degrees: Vec3, order: i32) -> Quat {
    let r = degrees * (std::f32::consts::PI / 180.);
    let (x, y, z) = (
        Quat::from_rotation_x(r.x),
        Quat::from_rotation_y(r.y),
        Quat::from_rotation_z(r.z),
    );
    match order {
        1 => y * z * x,
        2 => x * z * y,
        3 => z * x * y,
        4 => y * x * z,
        5 => x * y * z,
        _ => z * y * x,
    }
}

fn id(node: NodeHandle<'_>) -> Option<i64> {
    node.attributes().first()?.get_i64()
}

/// `"Name\0\x01Model"` → `"Name"`.
fn object_name(node: NodeHandle<'_>) -> String {
    node.attributes()
        .get(1)
        .and_then(|a| a.get_string())
        .map(|s| s.split('\0').next().unwrap_or(s).to_owned())
        .unwrap_or_default()
}

struct Model {
    name: String,
    translation: Vec3,
    rotation: Vec3,
    pre: Quat,
    post: Quat,
    order: i32,
    scale: f32,
}

impl Model {
    fn local(&self, translation: Vec3, rotation: Vec3) -> Frame {
        Frame::new(
            translation,
            (self.pre * euler(rotation, self.order) * self.post.conjugate()).normalize(),
        )
    }
}

/// One animated component: key times (seconds) and values.
type Curve = (Vec<f32>, Vec<f32>);

fn sample(curve: &Curve, time: f32) -> Option<f32> {
    let (times, values) = curve;
    let n = times.len().min(values.len());
    if n == 0 {
        return None;
    }
    if time <= times[0] {
        return Some(values[0]);
    }
    for i in 1..n {
        if time <= times[i] {
            let span = times[i] - times[i - 1];
            let a = if span > 0. { (time - times[i - 1]) / span } else { 1. };
            return Some(values[i - 1] + (values[i] - values[i - 1]) * a);
        }
    }
    Some(values[n - 1])
}

pub fn read(bytes: &[u8]) -> Result<Skeleton, ImportError> {
    let tree = match AnyTree::from_seekable_reader(std::io::Cursor::new(bytes)) {
        Ok(AnyTree::V7400(_, tree, _)) => tree,
        Ok(_) => return Err(ImportError::Format("unsupported FBX version".into())),
        Err(_) => {
            return Err(ImportError::Format(
                "not a binary FBX (export as binary, not ASCII)".into(),
            ))
        }
    };
    let root = tree.root();

    let mut models: HashMap<i64, Model> = HashMap::new();
    let mut curve_nodes: HashMap<i64, ()> = HashMap::new();
    let mut curves: HashMap<i64, Curve> = HashMap::new();
    for objects in root.children_by_name("Objects") {
        for node in objects.children() {
            let Some(id) = id(node) else { continue };
            match node.name() {
                "Model" => {
                    let p = properties(node);
                    let s = vec3(p.get("Lcl Scaling")).unwrap_or(Vec3::ONE);
                    models.insert(
                        id,
                        Model {
                            name: object_name(node),
                            translation: vec3(p.get("Lcl Translation")).unwrap_or_default(),
                            rotation: vec3(p.get("Lcl Rotation")).unwrap_or_default(),
                            pre: euler(vec3(p.get("PreRotation")).unwrap_or_default(), 0),
                            post: euler(vec3(p.get("PostRotation")).unwrap_or_default(), 0),
                            order: p.get("RotationOrder").and_then(|v| v.first()).map_or(0, |v| *v as i32),
                            scale: (s.x + s.y + s.z) / 3.,
                        },
                    );
                }
                "AnimationCurveNode" => {
                    curve_nodes.insert(id, ());
                }
                "AnimationCurve" => {
                    let child = |name: &str| node.children_by_name(name).next();
                    let times = child("KeyTime")
                        .and_then(|n| n.attributes().first()?.get_arr_i64().map(<[i64]>::to_vec))
                        .unwrap_or_default();
                    let values = child("KeyValueFloat")
                        .and_then(|n| match n.attributes().first()? {
                            AttributeValue::ArrF32(v) => Some(v.clone()),
                            AttributeValue::ArrF64(v) => Some(v.iter().map(|x| *x as f32).collect()),
                            _ => None,
                        })
                        .unwrap_or_default();
                    curves.insert(
                        id,
                        (times.iter().map(|t| (*t as f64 / TICKS) as f32).collect(), values),
                    );
                }
                _ => {}
            }
        }
    }

    let mut parent: HashMap<i64, i64> = HashMap::new();
    // Curve node → (model, "T" or "R"), and curve → (curve node, axis).
    let mut bound: HashMap<i64, (i64, char)> = HashMap::new();
    let mut axis: Vec<(i64, i64, usize)> = Vec::new();
    for connections in root.children_by_name("Connections") {
        for c in connections.children_by_name("C") {
            let a = c.attributes();
            let (Some(kind), Some(child), Some(to)) = (
                a.first().and_then(|v| v.get_string()),
                a.get(1).and_then(|v| v.get_i64()),
                a.get(2).and_then(|v| v.get_i64()),
            ) else {
                continue;
            };
            let property = a.get(3).and_then(|v| v.get_string()).unwrap_or("");
            match kind {
                "OO" if models.contains_key(&child) && models.contains_key(&to) => {
                    parent.insert(child, to);
                }
                "OP" if curve_nodes.contains_key(&child) && models.contains_key(&to) => {
                    match property {
                        "Lcl Translation" => drop(bound.insert(child, (to, 'T'))),
                        "Lcl Rotation" => drop(bound.insert(child, (to, 'R'))),
                        _ => {}
                    }
                }
                "OP" if curves.contains_key(&child) && curve_nodes.contains_key(&to) => {
                    let slot = match property {
                        "d|X" => 0,
                        "d|Y" => 1,
                        "d|Z" => 2,
                        _ => continue,
                    };
                    axis.push((child, to, slot));
                }
                _ => {}
            }
        }
    }

    // Per model: [T x/y/z, R x/y/z] curves.
    let mut animated: HashMap<i64, [Option<&Curve>; 6]> = HashMap::new();
    for (curve, node, slot) in &axis {
        let Some(&(model, kind)) = bound.get(node) else {
            continue;
        };
        let offset = if kind == 'T' { 0 } else { 3 };
        animated.entry(model).or_insert([None; 6])[offset + slot] = curves.get(curve);
    }

    // Parents first: a breadth-first walk from the models with no parent.
    let mut ids: Vec<i64> = models.keys().copied().filter(|m| !parent.contains_key(m)).collect();
    ids.sort();
    let mut i = 0;
    while i < ids.len() {
        let mut children: Vec<i64> = parent
            .iter()
            .filter(|(_, p)| **p == ids[i])
            .map(|(c, _)| *c)
            .collect();
        children.sort();
        ids.extend(children);
        i += 1;
    }
    let slot: HashMap<i64, usize> = ids.iter().enumerate().map(|(i, m)| (*m, i)).collect();

    let bones = ids
        .iter()
        .map(|m| {
            let model = &models[m];
            let channels = animated.get(m);
            let mut times: Vec<f32> = channels
                .into_iter()
                .flatten()
                .flatten()
                .flat_map(|c| c.0.iter().copied())
                .collect();
            times.sort_by(f32::total_cmp);
            times.dedup_by(|a, b| (*a - *b).abs() < crate::clip::TIME_EPSILON);
            let keys = times
                .into_iter()
                .map(|time| {
                    let at = |k: usize, rest: f32| {
                        channels
                            .and_then(|c| c[k])
                            .and_then(|c| sample(c, time))
                            .unwrap_or(rest)
                    };
                    let t = Vec3::new(
                        at(0, model.translation.x),
                        at(1, model.translation.y),
                        at(2, model.translation.z),
                    );
                    let r = Vec3::new(
                        at(3, model.rotation.x),
                        at(4, model.rotation.y),
                        at(5, model.rotation.z),
                    );
                    (time, model.local(t, r))
                })
                .collect();
            Bone {
                name: model.name.clone(),
                parent: parent.get(m).and_then(|p| slot.get(p)).copied(),
                rest: model.local(model.translation, model.rotation),
                scale: model.scale,
                keys,
            }
        })
        .collect();
    Ok(Skeleton { bones })
}

#[cfg(test)]
mod tests;
