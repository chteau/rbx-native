//! The dope sheet's channel list: one row per joint, in the rig's own
//! hierarchy (a child under its parent, indented), then any joint the clip
//! keys that the rig does not have, flagged so a mismatched clip is visible
//! rather than silently not playing.

use rbx_animation::{Clip, Rig};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Row {
    /// The joint's `Part1` name: what a `Pose` is called.
    pub(crate) name: String,
    pub(crate) depth: usize,
    pub(crate) in_rig: bool,
}

pub(crate) fn rows(rig: Option<&Rig>, clip: &Clip) -> Vec<Row> {
    let mut out = Vec::new();
    if let Some(rig) = rig {
        let root = rig.root.map(|r| rig.part_name(r).to_owned());
        // Depth-first from the root, children in the rig's own order.
        let mut stack: Vec<(String, usize)> = Vec::new();
        let children = |of: &str| -> Vec<String> {
            rig.joints
                .iter()
                .filter(|j| rig.part_name(j.part0) == of)
                .map(|j| rig.part_name(j.part1).to_owned())
                .collect()
        };
        if let Some(root) = root {
            for child in children(&root).into_iter().rev() {
                stack.push((child, 0));
            }
        }
        while let Some((name, depth)) = stack.pop() {
            if out.iter().any(|r: &Row| r.name == name) {
                continue;
            }
            for child in children(&name).into_iter().rev() {
                stack.push((child, depth + 1));
            }
            out.push(Row {
                name,
                depth,
                in_rig: true,
            });
        }
    }
    for channel in clip.face_channels() {
        out.push(Row {
            name: super::keys::face_row(&channel),
            depth: 0,
            in_rig: true,
        });
    }
    for joint in clip.joints() {
        if !out.iter().any(|r| r.name == joint) {
            out.push(Row {
                name: joint,
                depth: 0,
                in_rig: rig.is_none(),
            });
        }
    }
    out
}
