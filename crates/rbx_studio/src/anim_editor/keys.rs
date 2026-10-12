//! One way to address any key in a clip, a joint's pose or a face
//! channel's value, so selection, dragging, easing and the clipboard treat
//! the two alike. A face channel's row is named `face:<channel>`. No part
//! name can collide with that, since a `Pose` is named after a part and
//! part names carry no colon.

use rbx_animation::facs::FaceKey;
use rbx_animation::{Clip, Direction, PoseKey, Style};

pub(crate) const FACE: &str = "face:";

pub(crate) fn face_channel(name: &str) -> Option<&str> {
    name.strip_prefix(FACE)
}

pub(crate) fn face_row(channel: &str) -> String {
    format!("{FACE}{channel}")
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum AnyKey {
    Pose(PoseKey),
    Face(FaceKey),
}

impl AnyKey {
    pub(crate) fn easing(&self) -> (Style, Direction) {
        match self {
            AnyKey::Pose(k) => (k.style, k.direction),
            AnyKey::Face(k) => (k.style, k.direction),
        }
    }

    pub(crate) fn set_easing(&mut self, style: Option<Style>, direction: Option<Direction>) {
        let (s, d) = match self {
            AnyKey::Pose(k) => (&mut k.style, &mut k.direction),
            AnyKey::Face(k) => (&mut k.style, &mut k.direction),
        };
        if let Some(style) = style {
            *s = style;
        }
        if let Some(direction) = direction {
            *d = direction;
        }
    }
}

/// The times a row has keys at.
pub(crate) fn times(clip: &Clip, name: &str) -> Vec<f32> {
    match face_channel(name) {
        Some(c) => clip.face_track(c).map(|(t, _)| t).collect(),
        None => clip.track(name).map(|(t, _)| t).collect(),
    }
}

pub(crate) fn get(clip: &Clip, name: &str, time: f32) -> Option<AnyKey> {
    match face_channel(name) {
        Some(c) => {
            let i = clip.keyframe_index(time)?;
            clip.keyframes[i].faces.get(c).copied().map(AnyKey::Face)
        }
        None => clip.pose(time, name).copied().map(AnyKey::Pose),
    }
}

pub(crate) fn take(clip: &mut Clip, name: &str, time: f32) -> Option<AnyKey> {
    match face_channel(name) {
        Some(c) => clip.remove_face(time, c).map(AnyKey::Face),
        None => clip.remove_pose(time, name).map(AnyKey::Pose),
    }
}

pub(crate) fn put(clip: &mut Clip, name: &str, time: f32, key: AnyKey) {
    match (face_channel(name), key) {
        (Some(c), AnyKey::Face(k)) => clip.set_face(time, c, k),
        (None, AnyKey::Pose(k)) => clip.set_pose(time, name, k),
        // A face key onto a joint row, or the reverse, is a paste across
        // kinds; there is nothing it could mean.
        _ => {}
    }
}

pub(crate) fn set_easing(
    clip: &mut Clip,
    name: &str,
    time: f32,
    style: Option<Style>,
    direction: Option<Direction>,
) {
    let Some(i) = clip.keyframe_index(time) else {
        return;
    };
    let k = &mut clip.keyframes[i];
    match face_channel(name) {
        Some(c) => {
            if let Some(key) = k.faces.get_mut(c) {
                let mut any = AnyKey::Face(*key);
                any.set_easing(style, direction);
                if let AnyKey::Face(f) = any {
                    *key = f;
                }
            }
        }
        None => {
            if let Some(key) = k.poses.get_mut(name) {
                let mut any = AnyKey::Pose(*key);
                any.set_easing(style, direction);
                if let AnyKey::Pose(p) = any {
                    *key = p;
                }
            }
        }
    }
}

/// Every key (row name, time) on one keyframe.
pub(crate) fn on_keyframe(clip: &Clip, index: usize) -> Vec<(String, f32)> {
    let k = &clip.keyframes[index];
    k.poses
        .keys()
        .cloned()
        .chain(k.faces.keys().map(|c| face_row(c)))
        .map(|n| (n, k.time))
        .collect()
}
