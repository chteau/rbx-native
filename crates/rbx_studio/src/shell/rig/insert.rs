//! Putting a rig in the place: the dialog's Insert button, the "My Avatar"
//! download behind it, and the undo step around both.

use gpui_kit::Context;
use rbx_dom::{Ref, Variant, WeakDom};

use crate::command_bar::Feedback;
use crate::explorer;
use crate::shell::Shell;

use super::avatar::{self, Worn};
use super::dialog::{parse_user_id, Character};
use super::{build_rig, JointStyle, RigOptions, V3};

const SOURCE: &str = "Rig";

impl Shell {
    pub(crate) fn confirm_rig_dialog(&mut self, cx: &mut Context<Self>) {
        let Some(dialog) = self.rig_dialog.take() else {
            return;
        };
        let joints = JointStyle::of_place(&self.dom);
        match dialog.character {
            Character::Mannequin => {
                let options = RigOptions::new(dialog.rig_type, dialog.shape, dialog.scale, joints);
                self.place_rig(options, &[], 0, &[], cx);
            }
            Character::MyAvatar => self.insert_avatar(None, joints, cx),
            Character::Player => match parse_user_id(&self.rig_user.read(cx).value()) {
                Ok(id) => self.insert_avatar(Some(id), joints, cx),
                // The dialog stays up so the id can be fixed.
                Err(message) => {
                    self.rig_dialog = Some(dialog);
                    self.rig_user_focus = true;
                    self.rig_report(Err(message), cx);
                }
            },
        }
        cx.notify();
    }

    /// `user` is `None` for the signed-in user ("My Avatar").
    fn insert_avatar(&mut self, user: Option<u64>, joints: JointStyle, cx: &mut Context<Self>) {
        let label = user.map_or_else(|| "My Avatar".to_string(), |id| format!("Player {id}"));
        self.rig_report(Ok(format!("Fetching {label}\u{2026}")), cx);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { avatar::fetch(user) })
                .await;
            let _ = this.update(cx, |shell, cx| match result {
                Ok(fetched) => {
                    let mut options = avatar::options_for(&fetched.avatar, joints, [0.; 3]);
                    let packages = avatar::apply_packages(&mut options, &fetched.worn);
                    let notes = avatar::unapplied(&fetched.avatar, &fetched.worn);
                    shell.place_rig(options, &fetched.worn, packages, &notes, cx);
                }
                // Nothing is inserted: a default body here would pass for
                // the player's own.
                Err(err) => shell.rig_report(Err(format!("{label}: {err}")), cx),
            });
        })
        .detach();
    }

    /// One undo step: the rig, then whatever it wears.
    fn place_rig(
        &mut self,
        mut options: RigOptions,
        worn: &[Worn],
        packages: usize,
        notes: &[String],
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = explorer::find_by_name(&self.dom, "Workspace") else {
            return self.rig_report(Err("The place has no Workspace".into()), cx);
        };
        options.feet = spawn_top(&self.dom, workspace);
        self.push_history();
        let rig = build_rig(&mut self.dom, &options, workspace);
        let dressed = packages + avatar::dress(&mut self.dom, rig, worn);
        let changes = self.dom.take_changes();
        self.rebuild_explorer(cx);
        self.reselect(vec![rig], cx);
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);
        for note in notes {
            self.output.push(SOURCE, Feedback::Warning(note.clone()));
        }
        let name = self
            .dom
            .get(rig)
            .map_or_else(String::new, |i| i.name().to_owned());
        let wearing = if worn.is_empty() {
            String::new()
        } else {
            format!(", wearing {dressed} of {} items", worn.len())
        };
        self.rig_report(Ok(format!("Inserted {name}{wearing}")), cx);
    }

    fn rig_report(&mut self, result: Result<String, String>, cx: &mut Context<Self>) {
        let feedback = match result {
            Ok(message) => Feedback::Output(message),
            Err(message) => Feedback::Error(message),
        };
        self.output.push_once(SOURCE, feedback.clone());
        self.command_bar.set_feedback(feedback);
        cx.notify();
    }
}

/// The top of the first `SpawnLocation`, or the origin: where a rig's feet go.
pub(super) fn spawn_top(dom: &WeakDom, workspace: Ref) -> V3 {
    let mut pending = vec![workspace];
    while let Some(next) = pending.pop() {
        let Some(instance) = dom.get(next) else {
            continue;
        };
        pending.extend(instance.children());
        if instance.class() != "SpawnLocation" {
            continue;
        }
        if let (Some(Variant::CFrame(at)), Some(Variant::Vector3(size))) = (
            instance.properties().get("CFrame"),
            instance.properties().get("size"),
        ) {
            return [at.position.x, at.position.y + size.y / 2., at.position.z];
        }
    }
    [0.; 3]
}
