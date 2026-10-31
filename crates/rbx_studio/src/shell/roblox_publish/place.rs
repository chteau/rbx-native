//! Pure steps of the pick, confirm and link flow, kept out of `Shell` so
//! they can be tested without a window.

use std::path::Path;

use rbx_cloud::PublishMode;

use super::{
    upload::{outcome, upload},
    Dialog, Picking, Target,
};
use crate::command_bar::Feedback;
use crate::home::{self, RecentPlace};

/// What a pick from picker `token` does to `picking`. Only the picker still
/// open counts: it hands back the upload it was opened for and stops
/// waiting, so a late duplicate can't run a second upload. A pick from a
/// picker that was since replaced changes nothing.
pub(super) fn pick_landed(
    picking: &mut Option<Picking>,
    token: u64,
) -> Option<Option<PublishMode>> {
    picking.take_if(|p| p.token == token).map(|p| p.then)
}

pub(super) fn confirm(mode: PublishMode, target: Target, name: Option<String>) -> Dialog {
    Dialog::Confirm { mode, target, name }
}

/// What follows a pick: the confirmation of the upload the picker was
/// opened for, if any, never the upload itself.
pub(super) fn after_pick(
    then: Option<PublishMode>,
    target: Target,
    name: String,
) -> Option<Dialog> {
    then.map(|mode| confirm(mode, target, Some(name)))
}

/// The upload a confirmation agrees to: closes it and hands back its place
/// and mode. Anything else open, a failure, or nothing because Cancel or
/// Escape closed it, is left alone and uploads nothing.
pub(super) fn confirmed(dialog: &mut Option<Dialog>) -> Option<(Target, PublishMode)> {
    match dialog.take() {
        Some(Dialog::Confirm { mode, target, .. }) => Some((target, mode)),
        other => {
            *dialog = other;
            None
        }
    }
}

/// Uploads a place file's `bytes` to `link`, for a place no editor has open;
/// blocking. The answer is the line Output would show.
pub(in crate::shell) fn upload_file(
    link: &home::Link,
    bytes: &[u8],
    mode: PublishMode,
) -> Feedback {
    let target = Target {
        universe_id: link.universe_id,
        place_id: link.place_id,
    };
    outcome(target, mode, &upload(target, bytes, mode))
}

/// Stores the link, and moves the file to the top of Recent with it.
pub(super) fn link(path: &Path, target: Target, name: Option<String>) -> Result<(), String> {
    home::remember(RecentPlace {
        path: std::fs::canonicalize(path).unwrap_or(path.to_path_buf()),
        universe_id: Some(target.universe_id),
        place_id: Some(target.place_id),
        name,
        opened: None,
    })
    .map_err(|err| format!("the link couldn\u{2019}t be saved: {err}"))
}
