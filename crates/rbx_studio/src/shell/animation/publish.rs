//! The clip leaving the editor: written to an `.rbxm` file, or uploaded to
//! Roblox as an Animation asset through Open Cloud. Both carry the same
//! bytes, one `KeyframeSequence` at the root of a model file, which is what
//! Studio's own "Save to File" writes and what the assets API takes for
//! `assetType: "Animation"`.
//!
//! `RBX_STUDIO_ANIM_PUBLISH_MOCK_ID=<id>` stands in for a successful upload
//! and `RBX_STUDIO_ANIM_PUBLISH_MOCK_ERROR=<message>` for a refused one, each
//! after a short wait so the busy state can be seen; neither touches the
//! network or the key.

use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_animation::{Clip, Rig};
use rbx_cloud::Owner;
use rbx_dom::WeakDom;

use crate::command_bar::Feedback;
use crate::launcher::ui::{self, Weight};
use crate::shell::rig::{PublishOutcome, PublishResult};
use crate::tokens;

use super::Shell;

const MOCK_ID_VARIABLE: &str = "RBX_STUDIO_ANIM_PUBLISH_MOCK_ID";
const MOCK_ERROR_VARIABLE: &str = "RBX_STUDIO_ANIM_PUBLISH_MOCK_ERROR";
/// Where the Output lines of a publish or export are filed.
const SOURCE: &str = "Animation";

/// The clip as a model file, and what in it may not play on the rig.
pub(super) struct Packed {
    pub(super) bytes: Vec<u8>,
    pub(super) warning: Option<String>,
}

/// Writes `clip` as the only root of a fresh place file. Poses nest under
/// their parent joints when `rig` is given, which is what Roblox needs to
/// play them; joints `rig` lacks are written flat.
pub(super) fn pack(clip: &Clip, rig: Option<&Rig>) -> Result<Packed, String> {
    if clip
        .keyframes
        .iter()
        .all(|k| k.poses.is_empty() && k.faces.is_empty())
    {
        return Err("Nothing to publish: the clip has no keys".into());
    }
    let mut dom = WeakDom::new();
    rbx_animation::sequence::write(&mut dom, None, clip, rig);
    let bytes = crate::save::Format::Binary.encode(&dom)?;
    Ok(Packed {
        bytes,
        warning: warning(clip, rig),
    })
}

/// What a player of this clip would miss, said in a sentence.
fn warning(clip: &Clip, rig: Option<&Rig>) -> Option<String> {
    let Some(rig) = rig else {
        return Some(
            "No rig was bound, so the poses were not nested under their parent joints; Roblox may not play them. Select the rig and publish again."
                .into(),
        );
    };
    let missing: Vec<String> = clip
        .joints()
        .into_iter()
        .filter(|joint| rig.joint_for(joint).is_none())
        .collect();
    match missing.len() {
        0 => None,
        n => {
            let shown: Vec<&str> = missing.iter().take(4).map(String::as_str).collect();
            let more = if n > 4 { ", \u{2026}" } else { "" };
            Some(format!(
                "{n} keyed joint{} the rig doesn\u{2019}t have ({}{more}) will not play.",
                if n == 1 { "" } else { "s" },
                shown.join(", ")
            ))
        }
    }
}

/// The Publish form: what the upload is called and who owns it.
pub(in crate::shell) struct PublishForm {
    name: Entity<InputState>,
    description: Entity<InputState>,
    groups: Vec<rbx_cloud::Group>,
    /// `None` is the signed-in account.
    group: Option<u64>,
}

impl Shell {
    fn anim_rig(&self) -> Option<Rig> {
        self.anim.bound.as_ref().map(|b| b.rig.clone())
    }

    /// Opens the Publish form for the current clip.
    pub(in crate::shell) fn open_animation_publish(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.anim.publish_form.is_some() {
            return;
        }
        let clip = &self.anim.editor.clip;
        if let Err(err) = pack(clip, None) {
            self.anim.warn(err);
            cx.notify();
            return;
        }
        let seed = clip.name.clone();
        let name = cx.new(|cx| InputState::new(window, cx).default_value(seed));
        let description = cx
            .new(|cx| InputState::new(window, cx).placeholder("Optional: shown on the asset page"));
        self.anim.publish_form = Some(PublishForm {
            name,
            description,
            groups: crate::launcher::cached_groups(),
            group: None,
        });
        cx.notify();
    }

    /// Asks for the Publish form from a place without a `Window`.
    pub(in crate::shell) fn request_animation_publish(&mut self, cx: &mut Context<Self>) {
        self.anim.dialog_requested = Some(super::AnimDialog::Publish);
        cx.notify();
    }

    /// Opens whichever dialog was asked for since the last frame.
    pub(in crate::shell) fn realize_animation_dialogs(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.anim.dialog_requested.take() {
            Some(super::AnimDialog::Publish) => self.open_animation_publish(window, cx),
            Some(super::AnimDialog::ImportId) => self.open_animation_id_prompt(window, cx),
            None => {}
        }
    }

    /// Escape's half of the form; returns whether it was open. A running
    /// upload keeps the form up until it lands.
    pub(in crate::shell) fn close_animation_publish(&mut self) -> bool {
        if self.anim.publishing && self.anim.publish_form.is_some() {
            return true;
        }
        self.anim.publish_form.take().is_some()
    }

    /// Picks the creator by group id (`None`: the signed-in account). The
    /// scripted run names a group by id or by its first cached entry.
    pub(super) fn set_publish_group(&mut self, group: Option<u64>) {
        if let Some(form) = self.anim.publish_form.as_mut() {
            form.group = group.filter(|id| form.groups.iter().any(|g| g.id == *id));
        }
    }

    pub(super) fn first_publish_group(&self) -> Option<u64> {
        self.anim
            .publish_form
            .as_ref()
            .and_then(|f| f.groups.first().map(|g| g.id))
    }

    /// Uploads the clip with the form's name, description and creator.
    /// Refused while another publish is still running.
    pub(in crate::shell) fn publish_animation(&mut self, cx: &mut Context<Self>) {
        if self.anim.publishing {
            self.anim.warn("A publish is already running");
            cx.notify();
            return;
        }
        let Some(form) = self.anim.publish_form.as_ref() else {
            return;
        };
        let name = form.name.read(cx).value().trim().to_string();
        let description = form.description.read(cx).value().trim().to_string();
        let group = form.group;
        if name.is_empty() {
            self.anim.warn("Give the animation a name first");
            cx.notify();
            return;
        }
        let rig = self.anim_rig();
        let mut clip = self.anim.editor.clip.clone();
        clip.name = name.clone();
        let packed = match pack(&clip, rig.as_ref()) {
            Ok(packed) => packed,
            Err(err) => {
                self.anim.fail(err);
                cx.notify();
                return;
            }
        };
        self.anim.publishing = true;
        self.output.push_once(
            SOURCE,
            Feedback::Output(format!("Publishing {name}\u{2026}")),
        );
        cx.notify();
        let warning = packed.warning.clone();
        cx.spawn(async move |this, cx| {
            let result = upload(cx, name.clone(), description, group, packed.bytes).await;
            let _ = this.update(cx, |shell, cx| {
                shell.anim.publishing = false;
                shell.anim.publish_form = None;
                let (outcome, feedback) = match result {
                    Ok(id) => {
                        shell
                            .anim
                            .say(format!("Published {name} as rbxassetid://{id}"));
                        let line = format!("Published {name} as Animation rbxassetid://{id}");
                        (
                            PublishOutcome::AnimationPublished {
                                id,
                                warning: warning.clone(),
                            },
                            match &warning {
                                Some(w) => Feedback::Warning(format!("{line}. {w}")),
                                None => Feedback::Output(line),
                            },
                        )
                    }
                    Err(err) => {
                        shell.anim.fail(format!("Publish failed: {err}"));
                        (
                            PublishOutcome::AnimationFailed(err.clone()),
                            Feedback::Error(format!("{name} was not published: {err}")),
                        )
                    }
                };
                shell.output.push_once(SOURCE, feedback.clone());
                shell.command_bar.set_feedback(feedback);
                shell.anim.publish_result = Some(PublishResult { name, outcome });
                cx.notify();
            });
            let _ = this.update(cx, |shell, cx| shell.resume_debug_steps(cx));
        })
        .detach();
    }

    /// Asks where to write the clip as an `.rbxm` (straight into
    /// `RBX_STUDIO_EXPORT_DIR` when it is set) and writes it there.
    pub(in crate::shell) fn export_animation_rbxm(&mut self, cx: &mut Context<Self>) {
        let suggested = format!("{}.rbxm", file_name(&self.anim.editor.clip.name));
        if let Ok(directory) = std::env::var(crate::shell::export::EXPORT_DIR_VARIABLE) {
            self.write_animation_rbxm(&Path::new(&directory).join(suggested), cx);
            return;
        }
        let directory = self
            .path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let picked = cx.prompt_for_new_path(&directory, Some(&suggested));
        cx.spawn(async move |this, cx| {
            let Ok(picked) = picked.await else {
                return;
            };
            let _ = this.update(cx, |shell, cx| match picked {
                Ok(Some(path)) => shell.write_animation_rbxm(&path, cx),
                Ok(None) => {}
                Err(err) => {
                    shell.anim.fail(format!("No save dialog: {err:#}"));
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn write_animation_rbxm(&mut self, path: &Path, cx: &mut Context<Self>) {
        let rig = self.anim_rig();
        let written = pack(&self.anim.editor.clip, rig.as_ref()).and_then(|packed| {
            crate::save::write_atomic(path, &packed.bytes).map(|()| packed.warning)
        });
        match written {
            Ok(warning) => {
                let line = format!("Exported {}", path.display());
                self.output
                    .push_once(SOURCE, Feedback::Output(line.clone()));
                match warning {
                    Some(w) => self.anim.warn(format!("{line}. {w}")),
                    None => self.anim.say(line),
                }
            }
            Err(err) => self.anim.fail(format!("Export failed: {err}")),
        }
        cx.notify();
    }

    /// The Publish form over the window, if it is open.
    pub(in crate::shell) fn animation_publish_overlay(
        &self,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let form = self.anim.publish_form.as_ref()?;
        let busy = self.anim.publishing;
        let label = |text: &'static str| {
            ui::text(12., 18.)
                .w(px(84.))
                .flex_none()
                .text_color(tokens::text2())
                .child(text)
        };
        let field = |state: &Entity<InputState>, glyph: &'static str| {
            ui::field_frame(None, ui::panel2(), tokens::border(), glyph)
                .flex_1()
                .child(
                    div().flex_1().min_w_0().h_full().child(
                        Input::new(state)
                            .appearance(false)
                            .h_full()
                            .px(px(0.))
                            .text_size(px(12.))
                            .text_color(tokens::text())
                            .disabled(busy),
                    ),
                )
        };
        let creator = |id: &'static str, text: String, group: Option<u64>, on: bool| {
            ui::pill(
                text,
                Some(if group.is_some() { "users" } else { "user" }),
                on,
            )
            .id(SharedString::from(id.to_string()))
            .cursor_pointer()
            .when(!busy, |this| {
                this.on_click(cx.listener(move |shell, _, _, cx| {
                    shell.set_publish_group(group);
                    cx.notify();
                }))
            })
        };
        let mut creators = h_flex().flex_1().flex_wrap().gap(px(6.)).child(creator(
            "anim-publish-creator-user",
            "Your account".into(),
            None,
            form.group.is_none(),
        ));
        for g in &form.groups {
            let id = g.id;
            creators = creators.child(
                ui::pill(g.name.clone(), Some("users"), form.group == Some(id))
                    .id(SharedString::from(format!("anim-publish-creator-{id}")))
                    .cursor_pointer()
                    .when(!busy, |this| {
                        this.on_click(cx.listener(move |shell, _, _, cx| {
                            shell.set_publish_group(Some(id));
                            cx.notify();
                        }))
                    }),
            );
        }
        let preview = warning(
            &self.anim.editor.clip,
            self.anim.bound.as_ref().map(|b| &b.rig),
        );
        let body = v_flex()
            .mt(px(14.))
            .mx(px(20.))
            .ml(px(78.))
            .gap(px(10.))
            .child(
                h_flex()
                    .gap(px(12.))
                    .child(label("Name"))
                    .child(field(&form.name, "clapperboard")),
            )
            .child(
                h_flex()
                    .gap(px(12.))
                    .child(label("Description"))
                    .child(field(&form.description, "text")),
            )
            .child(
                h_flex()
                    .gap(px(12.))
                    .items_start()
                    .child(label("Creator"))
                    .child(creators),
            )
            .when(form.groups.is_empty(), |this| {
                this.child(
                    ui::text(11., 16.)
                        .ml(px(96.))
                        .text_color(tokens::text2())
                        .child("Your groups appear here once Home has listed them."),
                )
            })
            .when_some(preview, |this, text| {
                this.child(
                    h_flex()
                        .gap(px(6.))
                        .items_start()
                        .p(px(8.))
                        .rounded(px(6.))
                        .bg(ui::wash())
                        .text_color(tokens::text2())
                        .child(ui::icon("triangle-alert", 12.).flex_none().mt(px(2.)))
                        .child(ui::text(11.5, 17.).child(text)),
                )
            })
            .into_any_element();
        let cancel = if busy {
            ui::disabled_button("anim-publish-cancel", "Cancel").into_any_element()
        } else {
            ui::button("anim-publish-cancel", "Cancel", Weight::Secondary, false)
                .on_click(cx.listener(|shell, _, _, cx| {
                    shell.close_animation_publish();
                    cx.notify();
                }))
                .into_any_element()
        };
        let go = if busy {
            h_flex()
                .id("anim-publish-go")
                .debug_selector(|| "anim-publish-busy".into())
                .gap(px(6.))
                .items_center()
                .child(ui::spinner("anim-publish-spinner", 13.))
                .child(ui::disabled_button(
                    "anim-publish-busy",
                    "Publishing\u{2026}",
                ))
                .into_any_element()
        } else {
            ui::icon_button(
                "anim-publish-go",
                "cloud-upload",
                "Publish",
                Weight::Primary,
                false,
            )
            .on_click(cx.listener(|shell, _, _, cx| shell.publish_animation(cx)))
            .into_any_element()
        };
        Some(
            ui::dialog(
                520.,
                ui::dialog_glyph("cloud-upload", ui::accent(), tokens::accent_soft()),
                "Publish animation",
                "Uploads the clip to Roblox as an Animation asset you can play from any place.",
                Some(body),
                vec![cancel, go],
            )
            .into_any_element(),
        )
    }
}

/// `name` without the characters a file name cannot hold.
fn file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if "/\\:*?\"<>|".contains(c) { '_' } else { c })
        .collect();
    match cleaned.trim() {
        "" => "Animation".into(),
        trimmed => trimmed.to_string(),
    }
}

/// Runs the upload off the UI thread, or the mock in its place.
async fn upload(
    cx: &mut AsyncApp,
    name: String,
    description: String,
    group: Option<u64>,
    bytes: Vec<u8>,
) -> Result<u64, String> {
    let mock_id = std::env::var(MOCK_ID_VARIABLE).ok();
    let mock_error = std::env::var(MOCK_ERROR_VARIABLE).ok();
    if mock_id.is_some() || mock_error.is_some() {
        cx.background_executor()
            .timer(Duration::from_millis(3000))
            .await;
        if let Some(message) = mock_error {
            return Err(message);
        }
        return mock_id
            .unwrap_or_default()
            .trim()
            .parse()
            .map_err(|_| format!("{MOCK_ID_VARIABLE} is not a number"));
    }
    cx.background_spawn(async move {
        let (client, user) = crate::shell::freeze::authorize()?;
        let creator = match group {
            Some(id) => Owner::Group(id),
            None => Owner::User(user),
        };
        client
            .create_animation_asset(&name, &description, creator, &bytes)
            .map_err(|err| crate::shell::freeze::describe(&err))
    })
    .await
}

#[cfg(test)]
mod tests {
    use rbx_animation::{Clip, Frame, Keyframe, PoseKey};

    use super::{file_name, pack};

    #[test]
    fn a_packed_clip_reads_back_as_the_same_keys() {
        let mut clip = Clip {
            name: "Wave".into(),
            ..Clip::default()
        };
        let mut first = Keyframe::at(0.0);
        first
            .poses
            .insert("RightUpperArm".into(), PoseKey::new(Frame::IDENTITY));
        clip.keyframes.push(first);
        let mut last = Keyframe::at(0.5);
        last.poses
            .insert("RightUpperArm".into(), PoseKey::new(Frame::IDENTITY));
        clip.keyframes.push(last);
        let packed = pack(&clip, None).expect("packs");
        assert!(packed.warning.is_some(), "no rig: the poses are flat");
        let back = rbx_animation::import::asset::clip_from_model(&packed.bytes, "Wave")
            .expect("reads back");
        assert_eq!(back.keyframes.len(), 2);
        assert_eq!(back.joints(), vec!["RightUpperArm".to_string()]);
        assert!((back.length() - 0.5).abs() < 1e-4);
    }

    #[test]
    fn an_empty_clip_is_refused() {
        assert!(pack(&Clip::default(), None).is_err());
    }

    #[test]
    fn file_names_lose_path_characters() {
        assert_eq!(file_name("Run/Jump"), "Run_Jump");
        assert_eq!(file_name("  "), "Animation");
    }
}
