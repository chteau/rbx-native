//! Putting a rig in the place: the dialog's Insert button, the "My Avatar"
//! download behind it, and the undo step around both.

use gpui_kit::Context;
use rbx_dom::{Ref, Variant, WeakDom};

use crate::command_bar::Feedback;
use crate::explorer;
use crate::shell::Shell;

use rbx_cloud::Avatar;

use super::avatar::{self, Fate, Worn};
use super::dialog::{parse_user_id, Character};
use super::{build_rig, JointStyle, RigOptions, RigType, V3};

/// A downloaded avatar and the fates [`avatar::apply_packages`] gave its items.
type Import<'a> = (&'a Avatar, &'a [Worn], Vec<Fate>);

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
                self.place_rig(options, None, cx);
            }
            Character::MyAvatar => self.insert_avatar(None, dialog.avatar_type, joints, cx),
            Character::Player => match parse_user_id(&self.rig_user.read(cx).value()) {
                Ok(id) => self.insert_avatar(Some(id), dialog.avatar_type, joints, cx),
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
    /// `rig_type` is `None` to keep the avatar's own type.
    fn insert_avatar(
        &mut self,
        user: Option<u64>,
        rig_type: Option<RigType>,
        joints: JointStyle,
        cx: &mut Context<Self>,
    ) {
        let label = user.map_or_else(|| "My Avatar".to_string(), |id| format!("Player {id}"));
        self.rig_report(Ok(format!("Fetching {label}\u{2026}")), cx);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { avatar::fetch(user) })
                .await;
            let _ = this.update(cx, |shell, cx| match result {
                Ok(fetched) => {
                    let mut options =
                        avatar::options_for(&fetched.avatar, joints, [0.; 3], rig_type);
                    let early = avatar::apply_packages(&mut options, &fetched.worn);
                    let own = avatar::own_type(&fetched.avatar);
                    if options.rig_type != own {
                        let message = format!(
                            "{label} is {own:?}; built onto the {:?} body, as picked",
                            options.rig_type
                        );
                        shell.output.push(SOURCE, Feedback::Output(message));
                    }
                    shell.place_rig(options, Some((&fetched.avatar, &fetched.worn, early)), cx);
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
        import: Option<Import>,
        cx: &mut Context<Self>,
    ) {
        let Some(workspace) = explorer::find_by_name(&self.dom, "Workspace") else {
            return self.rig_report(Err("The place has no Workspace".into()), cx);
        };
        options.feet = spawn_top(&self.dom, workspace);
        self.push_history();
        let rig = build_rig(&mut self.dom, &options, workspace);
        let (dressed, total, notes) = match import {
            Some((avatar, worn, early)) => {
                let late = avatar::dress(&mut self.dom, rig, worn);
                let (used, notes) = avatar::settle(avatar, worn, &avatar::merge(early, late));
                (used, avatar.assets.len(), notes)
            }
            None => (0, 0, Vec::new()),
        };
        let changes = self.dom.take_changes();
        self.rebuild_explorer(cx);
        self.reselect(vec![rig], cx);
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);
        for note in notes {
            self.output
                .push(SOURCE, Feedback::Warning(format!("Not applied: {note}")));
        }
        let name = self
            .dom
            .get(rig)
            .map_or_else(String::new, |i| i.name().to_owned());
        let wearing = if total == 0 {
            String::new()
        } else {
            format!(", wearing {dressed} of {total} items")
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
