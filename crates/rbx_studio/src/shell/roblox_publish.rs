//! File › Save to Roblox / Publish to Roblox: uploads the open place as a new
//! version of a Roblox place through `rbx_cloud::Client::publish_place`.
//! Roblox's `versionType=Saved` saves the version without publishing it;
//! `Published` saves it and publishes it (`creator-docs`,
//! `reference/cloud/universes-api/v1.json`).
//!
//! The place comes from the game picker — Home's My Games reopened over the
//! editor (`launcher::open_game_picker`), with its listing, cache, owner
//! dropdown and add-by-link — and is remembered as the file's link
//! (`home::Link`, recorded through `home::remember` like a place opened from
//! Home), so a place downloaded from Roblox needs no picking at all.
//!
//! Every upload is confirmed first, in a dialog naming the experience and
//! place and saying what the mode does: an overwrite on Roblox can't be
//! undone from here. Every outcome is a row in the Output dock and the
//! Command Bar's label, like a local save; a failure also opens a dialog
//! with Roblox's answer, because a publish the user believes went through
//! is the costly mistake.
//!
//! Roblox's API doesn't update every class (unions, SurfaceAppearance,
//! wraps, Editable*; see [`NOT_UPDATED_BY_PUBLISH`]), so a successful upload
//! of a place holding any adds a warning row naming them.
//!
//! `RBX_STUDIO_PUBLISH_MOCK=ok|<HTTP status>|network` answers the upload
//! with a canned result instead of the network, and
//! `RBX_STUDIO_ROBLOX=link|save|publish|history` runs that File menu
//! command at open, for scripted captures.
//!
//! File › Version History… lists the linked place's versions and restores
//! one through this same upload; see `history`.

use std::path::Path;

use gpui_kit::component::Root;
use gpui_kit::*;
use rbx_cloud::{Experience, PublishMode};

use crate::command_bar::Feedback;
use crate::home::{self, RecentPlace};

use super::Shell;

mod history;
mod upload;
mod view;

use upload::{not_updated, outcome, upload, verb};

const MOCK_VARIABLE: &str = "RBX_STUDIO_PUBLISH_MOCK";
pub(super) const OPEN_VARIABLE: &str = "RBX_STUDIO_ROBLOX";

/// The Output dock's `source` for every row this module pushes.
const SOURCE: &str = "Roblox";

/// The Roblox place a file publishes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Target {
    universe_id: u64,
    place_id: u64,
}

impl Target {
    /// A picked game's starting place, or the place it was added by.
    fn of(experience: &Experience) -> Self {
        Target {
            universe_id: experience.universe_id,
            place_id: experience.root_place_id,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Dialog {
    /// Asked before every upload; `name` is the experience's, when known.
    Confirm {
        mode: PublishMode,
        target: Target,
        name: Option<String>,
    },
    Failed {
        mode: PublishMode,
        target: Target,
        failure: Failure,
    },
}

/// A failed upload. `unchanged` is whether the place is known to be as it
/// was: true only when the upload never left or Roblox answered with a
/// refusal. A dropped connection, a timeout or an unreadable answer may
/// come after Roblox took the file, so those say so instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Failure {
    pub(super) message: String,
    pub(super) unchanged: bool,
}

impl Failure {
    fn before_sending(message: String) -> Self {
        Failure {
            message,
            unchanged: true,
        }
    }
}

/// The game picker that is open, and the upload it was opened for (`None`
/// when the user only relinks). A pick carries the token of the picker it
/// came from and counts only while that picker is still this one — see
/// [`pick_landed`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Picking {
    token: u64,
    then: Option<PublishMode>,
}

pub(super) struct RobloxPublish {
    pub(super) dialog: Option<Dialog>,
    /// The confirmation's, so Enter and Escape reach it and not whatever
    /// had the keyboard; handed over on the next frame, where a `Window` is
    /// at hand.
    focus: FocusHandle,
    focus_pending: bool,
    /// An upload is in flight; a second one is refused until it answers.
    busy: bool,
    picking: Option<Picking>,
    /// The last picker token handed out.
    picks: u64,
    picker: Option<WindowHandle<Root>>,
    /// Bumped when the file is linked or one of its uploads lands, so
    /// Version History knows to re-read the link and the list.
    pub(super) changes: u64,
}

impl RobloxPublish {
    pub(super) fn new(cx: &mut Context<Shell>) -> Self {
        RobloxPublish {
            dialog: None,
            focus: cx.focus_handle(),
            focus_pending: false,
            busy: false,
            picking: None,
            picks: 0,
            picker: None,
            changes: 0,
        }
    }
}

impl Shell {
    /// The two File menu commands. An unlinked file picks its game first
    /// and then asks to confirm `mode`.
    pub(crate) fn upload_to_roblox(&mut self, mode: PublishMode, cx: &mut Context<Self>) {
        match home::link_of(&self.path) {
            Some(link) => {
                let target = Target {
                    universe_id: link.universe_id,
                    place_id: link.place_id,
                };
                self.ask_to_upload(confirm(mode, target, link.name), cx);
            }
            None => self.open_roblox_link(Some(mode), cx),
        }
    }

    /// File › Link to Roblox Place…, the first step of an unlinked upload,
    /// and a failed upload's "Change place…": brings the game picker
    /// forward, opening it if it isn't.
    pub(crate) fn open_roblox_link(&mut self, then: Option<PublishMode>, cx: &mut Context<Self>) {
        self.roblox.dialog = None;
        if let (Some(picking), Some(window)) = (&mut self.roblox.picking, self.roblox.picker) {
            if window
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
            {
                picking.then = then;
                return;
            }
        }
        self.roblox.picks += 1;
        let token = self.roblox.picks;
        self.roblox.picking = Some(Picking { token, then });
        cx.notify();
        let shell = cx.entity().downgrade();
        // Deferred: not from inside this update, which may be a render's.
        cx.defer(move |cx| {
            let on_pick = {
                let shell = shell.clone();
                move |experience: Experience, cx: &mut App| {
                    let _ = shell.update(cx, |shell, cx| shell.picked(token, experience, cx));
                }
            };
            let opened = crate::launcher::open_game_picker(on_pick, cx);
            let _ = shell.update(cx, |shell, _| shell.roblox.picker = opened);
        });
    }

    /// `RBX_STUDIO_ROBLOX`; see the module doc.
    pub(super) fn apply_debug_roblox(&mut self, cx: &mut Context<Self>) {
        match std::env::var(OPEN_VARIABLE).as_deref() {
            Ok("link") => self.open_roblox_link(None, cx),
            Ok("save") => self.upload_to_roblox(PublishMode::Saved, cx),
            Ok("publish") => self.upload_to_roblox(PublishMode::Published, cx),
            Ok("history") => self.open_version_history(cx),
            _ => {}
        }
    }

    /// Escape's half of the dialogs; returns whether one was open.
    pub(super) fn close_roblox_dialog(&mut self) -> bool {
        self.roblox.dialog.take().is_some()
    }

    /// Runs from render, where a `Window` is at hand; see [`RobloxPublish::focus`].
    pub(super) fn focus_roblox_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.roblox.focus_pending) {
            self.roblox.focus.focus(window, cx);
        }
    }

    /// The game picker's answer: links the file, then asks to confirm the
    /// upload the picker was opened for — a pick never uploads by itself.
    fn picked(&mut self, token: u64, experience: Experience, cx: &mut Context<Self>) {
        let Some(then) = pick_landed(&mut self.roblox.picking, token) else {
            return;
        };
        let target = Target::of(&experience);
        if let Err(message) = link(&self.path, target, Some(experience.name.clone())) {
            let feedback = Feedback::Error(format!(
                "Linking to place {} failed: {message}",
                target.place_id
            ));
            self.output.push(SOURCE, feedback.clone());
            self.command_bar.set_feedback(feedback);
            cx.notify();
            return;
        }
        self.title = experience.name.clone().into();
        self.retitle = true;
        self.roblox.changes += 1;
        self.output.push(
            SOURCE,
            Feedback::Output(format!(
                "Linked to place {} of {}",
                target.place_id, experience.name
            )),
        );
        if let Some(dialog) = after_pick(then, target, experience.name) {
            self.ask_to_upload(dialog, cx);
        }
        cx.notify();
    }

    fn ask_to_upload(&mut self, dialog: Dialog, cx: &mut Context<Self>) {
        self.roblox.dialog = Some(dialog);
        self.roblox.focus_pending = true;
        cx.notify();
    }

    /// The confirmation's Save/Publish button, and Enter while it is open.
    pub(super) fn confirm_roblox_upload(&mut self, cx: &mut Context<Self>) {
        if let Some((target, mode)) = confirmed(&mut self.roblox.dialog) {
            self.start_upload(target, mode, cx);
        }
    }

    pub(super) fn start_upload(
        &mut self,
        target: Target,
        mode: PublishMode,
        cx: &mut Context<Self>,
    ) {
        if self.roblox.busy {
            self.output.push(
                SOURCE,
                Feedback::Error(format!(
                    "{} place {} didn\u{2019}t start: an upload is already in progress.",
                    verb(mode).0,
                    target.place_id
                )),
            );
            cx.notify();
            return;
        }
        self.roblox.dialog = None;
        // What is on screen, not what the DOM held when typing last paused.
        self.flush_script_edits(cx);
        let bytes = match self.format.encode(&self.dom) {
            Ok(bytes) => bytes,
            Err(message) => {
                let failed = Err(Failure::before_sending(message));
                return self.finish_upload(target, mode, failed, None, cx);
            }
        };
        // Checked on the tree that was encoded: edits made while a slow
        // upload runs aren't in it.
        let warning = not_updated(&self.dom);
        self.roblox.busy = true;
        self.command_bar.set_feedback(Feedback::Output(format!(
            "{} place {}\u{2026}",
            verb(mode).0,
            target.place_id
        )));
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { upload(target, &bytes, mode) })
                .await;
            let _ = this.update(cx, |shell, cx| {
                shell.finish_upload(target, mode, result, warning, cx)
            });
        })
        .detach();
    }

    fn finish_upload(
        &mut self,
        target: Target,
        mode: PublishMode,
        result: Result<u64, Failure>,
        warning: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.roblox.busy = false;
        let feedback = outcome(target, mode, &result);
        self.output.push(SOURCE, feedback.clone());
        self.command_bar.set_feedback(feedback);
        if let Some(warning) = warning.filter(|_| result.is_ok()) {
            self.output.push(SOURCE, Feedback::Warning(warning));
        }
        self.roblox.changes += u64::from(result.is_ok());
        if let Err(failure) = result {
            self.roblox.dialog = Some(Dialog::Failed {
                mode,
                target,
                failure,
            });
        }
        cx.notify();
    }
}

/// What a pick from picker `token` does to `picking`. Only the picker still
/// open counts: it hands back the upload it was opened for and stops
/// waiting, so a late duplicate can't run a second upload. A pick from a
/// picker that was since replaced changes nothing.
fn pick_landed(picking: &mut Option<Picking>, token: u64) -> Option<Option<PublishMode>> {
    picking.take_if(|p| p.token == token).map(|p| p.then)
}

fn confirm(mode: PublishMode, target: Target, name: Option<String>) -> Dialog {
    Dialog::Confirm { mode, target, name }
}

/// What follows a pick: the confirmation of the upload the picker was
/// opened for, if any — never the upload itself.
fn after_pick(then: Option<PublishMode>, target: Target, name: String) -> Option<Dialog> {
    then.map(|mode| confirm(mode, target, Some(name)))
}

/// The upload a confirmation agrees to: closes it and hands back its place
/// and mode. Anything else open — a failure, or nothing because Cancel or
/// Escape closed it — is left alone and uploads nothing.
fn confirmed(dialog: &mut Option<Dialog>) -> Option<(Target, PublishMode)> {
    match dialog.take() {
        Some(Dialog::Confirm { mode, target, .. }) => Some((target, mode)),
        other => {
            *dialog = other;
            None
        }
    }
}

/// Stores the link, and moves the file to the top of Recent with it.
fn link(path: &Path, target: Target, name: Option<String>) -> Result<(), String> {
    home::remember(RecentPlace {
        path: std::fs::canonicalize(path).unwrap_or(path.to_path_buf()),
        universe_id: Some(target.universe_id),
        place_id: Some(target.place_id),
        name,
        opened: None,
    })
    .map_err(|err| format!("the link couldn\u{2019}t be saved: {err}"))
}

#[cfg(test)]
#[path = "roblox_publish/tests.rs"]
mod tests;
