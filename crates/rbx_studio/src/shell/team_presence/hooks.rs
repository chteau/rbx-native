//! What the rest of the editor tells team presence (the camera, every DOM
//! edit) and what it reads back (the marks on Explorer rows and script
//! tabs).

use std::collections::HashMap;

use gpui_kit::{Context, SharedString};
use rbx_dom::{Change, Ref};
use rbx_viewer::Pose;

use super::model::Status;
use super::{now_ms, Shell};

impl Shell {
    /// The viewport's throttled camera pose.
    pub(in crate::shell) fn presence_pose(&mut self, pose: Pose) {
        let c = crate::camera::cframe_from_pose(pose);
        let r = c.rotation;
        self.team_presence.camera = Some([
            f64::from(c.position.x),
            f64::from(c.position.y),
            f64::from(c.position.z),
            f64::from(r[0]),
            f64::from(r[1]),
            f64::from(r[2]),
            f64::from(r[3]),
            f64::from(r[4]),
            f64::from(r[5]),
            f64::from(r[6]),
            f64::from(r[7]),
            f64::from(r[8]),
        ]);
    }

    /// Every DOM edit: property writes go into the edit log; any structural
    /// change re-resolves the collaborators' paths.
    pub(in crate::shell) fn presence_changes(
        &mut self,
        changes: &[Change],
        cx: &mut Context<Self>,
    ) {
        let now = now_ms();
        let mut structural = false;
        for change in changes {
            match change {
                Change::Property { referent, name } => {
                    structural |= name == "Name";
                    let Some(instance) = self.dom.get(*referent) else {
                        continue;
                    };
                    let class = instance.class();
                    if class == "Camera" && (name == "CFrame" || name == "Focus") {
                        continue;
                    }
                    let (Some(value), Some(path)) = (
                        instance.properties().get(name.as_str()),
                        rbx_presence::paths::path_of(&self.dom, *referent),
                    ) else {
                        continue;
                    };
                    let text = self.properties.format(&self.dom, class, name, value);
                    self.team_presence.edits.note(path, name.clone(), text, now);
                }
                _ => structural = true,
            }
        }
        if structural && self.refresh_collaborators(cx) {
            cx.notify();
        }
    }

    /// For the Explorer: each instance a collaborator has selected or open,
    /// with their colour and a tooltip line ("Ana has this selected").
    pub(in crate::shell) fn presence_marks(&self) -> HashMap<Ref, Vec<PresenceMark>> {
        let mut marks: HashMap<Ref, Vec<PresenceMark>> = HashMap::new();
        for who in &self.team_presence.collaborators {
            if who.status != Status::Sending {
                continue;
            }
            for &r in &who.selection {
                marks.entry(r).or_default().push(PresenceMark {
                    color: who.color,
                    initial: initial(&who.name),
                    text: format!("{} has this selected", who.name).into(),
                });
            }
            if let Some((_, Some(r))) = &who.script {
                marks.entry(*r).or_default().push(PresenceMark {
                    color: who.color,
                    initial: initial(&who.name),
                    text: format!("{} has this script open", who.name).into(),
                });
            }
        }
        marks
    }
}

/// One collaborator's mark on an Explorer row or a script tab.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::shell) struct PresenceMark {
    pub(in crate::shell) color: (u8, u8, u8),
    /// The first letter of their name, drawn in the mark so it is not
    /// mistaken for the "unsaved" dot a tab also carries.
    pub(in crate::shell) initial: SharedString,
    pub(in crate::shell) text: SharedString,
}

fn initial(name: &str) -> SharedString {
    name.chars()
        .next()
        .map(|first| first.to_uppercase().collect::<String>())
        .unwrap_or_default()
        .into()
}
