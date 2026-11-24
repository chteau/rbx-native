//! Video through Import, behind the `video-uploads` beta switch, as
//! Studio's beta of that name puts video in the Asset Manager's Import
//! (measured with the beta on: its file picker gains "Video Files (*.mp4
//! *.mov)"). Here it is Import › Video… on the ribbon, Model › Import
//! Video…, or a video dropped on the window.
//!
//! Roblox's documented limits (see `rbx_cloud::video`) are checked on the
//! file before anything is sent, then the price is put to the user: each
//! video upload costs 2,000 Robux, which Studio's queue never says up
//! front. The upload is Import's own (`RBX_STUDIO_IMPORT_MOCK` stands in for
//! it the same way, answering `RBX_STUDIO_IMPORT_MOCK_ID`), and the video
//! lands as a `VideoFrame` in the selected GUI, or in a new `ScreenGui` in
//! `StarterGui`, since a `VideoFrame` plays only under a `ScreenGui`,
//! `SurfaceGui` or `BillboardGui` (`creator-docs`, `ui/video-frames.md`).

use std::path::{Path, PathBuf};

use gpui_kit::*;
use rbx_cloud::video::{self, UploadedVideo, VideoInfo};
use rbx_cloud::ModelFile;
use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::freeze::authorize;
use super::import3d::{MOCK_ERROR_VARIABLE, MOCK_ID_VARIABLE, MOCK_VARIABLE};
use super::Shell;
use crate::command_bar::Feedback;
use crate::explorer;
use crate::launcher::ui::{self, Weight};

pub(crate) const BETA_ID: &str = "video-uploads";
const SOURCE: &str = "Import";

pub(crate) fn enabled() -> bool {
    crate::beta::enabled(BETA_ID)
}

pub(super) fn is_video(path: &Path) -> bool {
    video::content_type(path).is_some()
}

/// A checked video waiting for the user to agree to its price.
pub(super) struct PendingVideo {
    path: PathBuf,
    name: String,
    info: VideoInfo,
}

impl Shell {
    /// Checks `path` against Roblox's limits off the UI thread, then asks.
    pub(super) fn import_video(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let name = stem(&path);
        cx.spawn(async move |this, cx| {
            let file = path.clone();
            let checked = cx
                .background_spawn(async move { video::check(&file) })
                .await;
            let _ = this.update(cx, |shell, cx| match checked {
                Ok(info) => {
                    shell.pending_video = Some(PendingVideo { path, name, info });
                    cx.notify();
                }
                Err(why) => {
                    shell.video_report(Err(format!("{name} can\u{2019}t be uploaded: {why}")), cx)
                }
            });
        })
        .detach();
    }

    fn upload_pending_video(&mut self, cx: &mut Context<Self>) {
        let Some(PendingVideo { path, name, .. }) = self.pending_video.take() else {
            return;
        };
        self.command_bar
            .set_feedback(Feedback::Output(format!("Uploading {name}\u{2026}")));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let label = name.clone();
            let result = cx
                .background_spawn(async move { upload(&path, &label) })
                .await;
            let _ = this.update(cx, |shell, cx| shell.finish_video(name, result, cx));
        })
        .detach();
    }

    fn finish_video(
        &mut self,
        name: String,
        result: Result<UploadedVideo, String>,
        cx: &mut Context<Self>,
    ) {
        let uploaded = match result {
            Ok(uploaded) => uploaded,
            Err(err) => return self.video_report(Err(format!("{name}: {err}")), cx),
        };
        let selected = self.selected();
        self.push_history();
        let placed = place(
            &mut self.dom,
            &self.database,
            selected,
            &name,
            uploaded.asset_id,
        );
        let changes = self.dom.take_changes();
        self.rebuild_explorer(cx);
        if let Ok(frame) = placed {
            self.reselect(vec![frame], cx);
        }
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);
        if uploaded.moderation_pending {
            self.output.push(
                SOURCE,
                Feedback::Warning(format!(
                    "Roblox is still reviewing {name}; it plays once moderation approves it."
                )),
            );
        }
        let id = uploaded.asset_id;
        self.video_report(
            placed.map(|_| format!("Added {name} as a VideoFrame (rbxassetid://{id})")),
            cx,
        );
    }

    fn video_report(&mut self, result: Result<String, String>, cx: &mut Context<Self>) {
        let feedback = match result {
            Ok(message) => Feedback::Output(message),
            Err(message) => Feedback::Error(message),
        };
        self.output.push(SOURCE, feedback.clone());
        self.command_bar.set_feedback(feedback);
        cx.notify();
    }

    /// The price, put before the upload.
    pub(super) fn video_upload_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let pending = self.pending_video.as_ref()?;
        let info = pending.info;
        let text = format!(
            "{} ({}, {}\u{d7}{}, {:.1} MB) goes up as a video asset. Roblox charges 2,000 Robux for every video upload, paid by the account your API key belongs to, and takes them only from 13+ ID-verified accounts.",
            pending.name,
            video::clock(info.duration),
            info.width,
            info.height,
            info.bytes as f64 / 1_000_000.0,
        );
        Some(
            div()
                .absolute()
                .inset_0()
                .child(ui::dialog(
                    440.,
                    ui::dialog_glyph("video", ui::accent(), ui::wash()),
                    format!("Upload {}?", pending.name),
                    text,
                    None,
                    vec![
                        ui::button("video-upload-cancel", "Cancel", Weight::Secondary, false)
                            .on_click(cx.listener(|shell, _, _, cx| {
                                shell.pending_video = None;
                                cx.notify();
                            }))
                            .into_any_element(),
                        ui::button(
                            "video-upload-ok",
                            "Upload for 2,000 Robux",
                            Weight::Primary,
                            false,
                        )
                        .on_click(cx.listener(|shell, _, _, cx| shell.upload_pending_video(cx)))
                        .into_any_element(),
                    ],
                ))
                .into_any_element(),
        )
    }
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .map_or_else(|| "Video".into(), |s| s.to_string_lossy().into_owned())
}

/// Blocking: the upload, through the key, as the user who owns it.
fn upload(path: &Path, name: &str) -> Result<UploadedVideo, String> {
    if let Ok(message) = std::env::var(MOCK_ERROR_VARIABLE) {
        return Err(message);
    }
    if std::env::var_os(MOCK_VARIABLE).is_some() {
        return Ok(UploadedVideo {
            asset_id: std::env::var(MOCK_ID_VARIABLE)
                .ok()
                .and_then(|id| id.parse().ok())
                .unwrap_or(0),
            moderation_pending: false,
        });
    }
    let content_type = video::content_type(path).ok_or("Roblox takes video as .mp4 or .mov")?;
    let bytes = std::fs::read(path).map_err(|err| format!("{}: {err}", path.display()))?;
    let (client, user) = authorize()?;
    let file_name = path
        .file_name()
        .map_or_else(|| "video.mp4".into(), |n| n.to_string_lossy().into_owned());
    let file = ModelFile {
        name: &file_name,
        content_type,
        bytes: &bytes,
    };
    client
        .create_video_asset(
            name,
            "Uploaded by rbx-native.",
            rbx_cloud::Owner::User(user),
            &file,
        )
        .map_err(|err| video::explain(&err))
}

/// A `VideoFrame` playing `asset_id` in the selected GUI, or in a new
/// `ScreenGui` in `StarterGui` when the selection is not one.
pub(super) fn place(
    dom: &mut WeakDom,
    database: &ReflectionDatabase,
    selected: Option<Ref>,
    name: &str,
    asset_id: u64,
) -> Result<Ref, String> {
    let in_gui = selected.filter(|&r| {
        dom.get(r).is_some_and(|i| {
            database.is_subclass_of(i.class(), "LayerCollector")
                || database.is_subclass_of(i.class(), "GuiObject")
        })
    });
    let parent = match in_gui {
        Some(gui) => gui,
        None => {
            let starter = explorer::find_by_name(dom, "StarterGui")
                .ok_or("The place has no StarterGui to put the video in")?;
            dom.new_instance("ScreenGui", name, Some(starter))
        }
    };
    let frame = dom.new_instance("VideoFrame", name, Some(parent));
    crate::properties::edit::commit(
        dom,
        database,
        frame,
        "Video",
        &format!("rbxassetid://{asset_id}"),
    )
    .map_err(|err| format!("the VideoFrame took no video: {err}"))?;
    Ok(frame)
}

#[cfg(test)]
#[path = "video_upload/tests.rs"]
mod tests;
