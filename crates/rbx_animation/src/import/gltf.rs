//! Reads a skeletal animation from glTF (`.glb`, or `.gltf` with its buffers),
//! the format Blender's built-in exporter writes without any of the scaling
//! settings an FBX for Roblox needs.
//!
//! Every node counts as a bone, which keeps armatures with meshes or empties
//! between bones working. A node's scale becomes the bone's uniform scale
//! (the mean of its three components). `STEP` samplers are read as linear,
//! which over a baked export (a key on every frame) differs by at most a
//! frame. `CUBICSPLINE` samplers keep their values and drop their tangents.

use base64::Engine;
use glam::{Quat, Vec3};
use gltf::animation::util::ReadOutputs;
use gltf::animation::{Interpolation, Property};
use gltf::buffer::Source;
use gltf::Gltf;

use crate::frame::Frame;
use crate::import::skeleton::{Bone, Skeleton};
use crate::import::ImportError;

/// The animations in a file, by name (or "Animation N" when unnamed).
pub fn animation_names(bytes: &[u8]) -> Result<Vec<String>, ImportError> {
    let gltf = Gltf::from_slice(bytes).map_err(|e| ImportError::Format(e.to_string()))?;
    Ok(gltf
        .animations()
        .map(|a| {
            a.name()
                .map(str::to_owned)
                .unwrap_or_else(|| format!("Animation {}", a.index() + 1))
        })
        .collect())
}

fn buffers(
    gltf: &Gltf,
    external: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Result<Vec<Vec<u8>>, ImportError> {
    gltf.buffers()
        .map(|buffer| match buffer.source() {
            Source::Bin => gltf
                .blob
                .clone()
                .ok_or_else(|| ImportError::Format("GLB has no binary chunk".into())),
            Source::Uri(uri) => {
                if let Some(data) = uri.strip_prefix("data:") {
                    let payload = data.split_once(";base64,").map(|(_, p)| p).ok_or_else(|| {
                        ImportError::Format("only base64 data URIs are supported".into())
                    })?;
                    base64::engine::general_purpose::STANDARD
                        .decode(payload)
                        .map_err(|_| ImportError::Format("bad base64 buffer".into()))
                } else {
                    external(uri)
                        .ok_or_else(|| ImportError::Format(format!("missing buffer file {uri}")))
                }
            }
        })
        .collect()
}

/// Every key time across `channels`, merged.
fn merged(lists: &[&Vec<f32>]) -> Vec<f32> {
    let mut times: Vec<f32> = lists.iter().flat_map(|l| l.iter().copied()).collect();
    times.sort_by(f32::total_cmp);
    times.dedup_by(|a, b| (*a - *b).abs() < crate::clip::TIME_EPSILON);
    times
}

fn lerp_at<T: Copy>(keys: &[(f32, T)], time: f32, mix: impl Fn(T, T, f32) -> T) -> Option<T> {
    let first = keys.first()?;
    if time <= first.0 {
        return Some(first.1);
    }
    for pair in keys.windows(2) {
        if time <= pair[1].0 {
            let span = pair[1].0 - pair[0].0;
            let alpha = if span > 0. {
                (time - pair[0].0) / span
            } else {
                1.
            };
            return Some(mix(pair[0].1, pair[1].1, alpha));
        }
    }
    keys.last().map(|k| k.1)
}

/// Reads animation `which` (an index into [`animation_names`]) and the node
/// hierarchy it animates. `external` supplies a `.gltf`'s separate buffer
/// files by URI.
pub fn read(
    bytes: &[u8],
    which: usize,
    external: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> Result<Skeleton, ImportError> {
    let gltf = Gltf::from_slice(bytes).map_err(|e| ImportError::Format(e.to_string()))?;
    let data = buffers(&gltf, external)?;
    let animation = gltf
        .animations()
        .nth(which)
        .ok_or_else(|| ImportError::Format("the file has no animation".into()))?;

    // Parents first, so `Skeleton` can place every bone in one pass.
    let count = gltf.nodes().count();
    let mut parent = vec![None; count];
    for node in gltf.nodes() {
        for child in node.children() {
            parent[child.index()] = Some(node.index());
        }
    }
    let mut order: Vec<usize> = (0..count).filter(|&i| parent[i].is_none()).collect();
    let mut i = 0;
    while i < order.len() {
        let node = gltf.nodes().nth(order[i]).expect("index from this file");
        order.extend(node.children().map(|c| c.index()));
        i += 1;
    }
    let slot: Vec<usize> = {
        let mut s = vec![0; count];
        for (at, &node) in order.iter().enumerate() {
            s[node] = at;
        }
        s
    };

    let mut translations: Vec<Vec<(f32, Vec3)>> = vec![Vec::new(); count];
    let mut rotations: Vec<Vec<(f32, Quat)>> = vec![Vec::new(); count];
    for channel in animation.channels() {
        let node = channel.target().node().index();
        let cubic = channel.sampler().interpolation() == Interpolation::CubicSpline;
        let reader = channel.reader(|b| data.get(b.index()).map(Vec::as_slice));
        let Some(inputs) = reader.read_inputs() else {
            continue;
        };
        let times: Vec<f32> = inputs.collect();
        // A cubic spline stores (in-tangent, value, out-tangent) per key.
        let pick = |i: usize| if cubic { i % 3 == 1 } else { true };
        match (channel.target().property(), reader.read_outputs()) {
            (Property::Translation, Some(ReadOutputs::Translations(values))) => {
                let values = values
                    .enumerate()
                    .filter(|(i, _)| pick(*i))
                    .map(|(_, v)| Vec3::from(v));
                translations[node] = times.iter().copied().zip(values).collect();
            }
            (Property::Rotation, Some(ReadOutputs::Rotations(values))) => {
                let values = values
                    .into_f32()
                    .enumerate()
                    .filter(|(i, _)| pick(*i))
                    .map(|(_, v)| Quat::from_array(v).normalize());
                rotations[node] = times.iter().copied().zip(values).collect();
            }
            _ => {}
        }
    }

    let nodes: Vec<gltf::Node> = gltf.nodes().collect();
    let bones = order
        .iter()
        .map(|&n| {
            let node = &nodes[n];
            let (t, r, s) = node.transform().decomposed();
            let rest = Frame::new(Vec3::from(t), Quat::from_array(r).normalize());
            let times = merged(&[
                &translations[n].iter().map(|k| k.0).collect(),
                &rotations[n].iter().map(|k| k.0).collect(),
            ]);
            let keys = times
                .into_iter()
                .map(|time| {
                    let position = lerp_at(&translations[n], time, |a, b, t| a.lerp(b, t))
                        .unwrap_or(rest.position);
                    let rotation = lerp_at(&rotations[n], time, |a, b, t| a.slerp(b, t))
                        .unwrap_or(rest.rotation);
                    (time, Frame::new(position, rotation))
                })
                .collect();
            Bone {
                name: node
                    .name()
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("Node{n}")),
                parent: parent[n].map(|p| slot[p]),
                rest,
                scale: (s[0] + s[1] + s[2]) / 3.,
                keys,
            }
        })
        .collect();
    Ok(Skeleton { bones })
}

#[cfg(test)]
mod tests;
