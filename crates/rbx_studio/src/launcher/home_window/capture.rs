//! Home's opening state for screenshot captures, set by environment
//! variables.

use gpui_kit::*;

use super::{Dialog, Games, HomeWindow, LinkState};

/// Home's opening state for a capture. `newexperience` opens the picker's
/// New experience form; `RBX_STUDIO_NEW_EXPERIENCE_MOCK=ok|<text>` makes its
/// Create answer with a canned success or that error text, and its groups a
/// single "Mock Studio Group", instead of calling Roblox.
pub(super) const PAGE_VARIABLE: &str = "RBX_STUDIO_LAUNCHER_PAGE";
const DIALOG_VARIABLE: &str = "RBX_STUDIO_LAUNCHER_DIALOG";
const LINK_VARIABLE: &str = "RBX_STUDIO_LAUNCHER_LINK";

impl HomeWindow {
    pub(super) fn apply_capture_state(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Ok(value) = std::env::var(LINK_VARIABLE) {
            let (text, state) = value.split_once('@').unwrap_or((&value, ""));
            let text = text.to_string();
            self.link
                .update(cx, |input, cx| input.set_value(text.clone(), window, cx));
            let id = rbx_cloud::place_id_from_link(&text).unwrap_or(0);
            self.link_state = match state {
                "resolving" => LinkState::Resolving,
                "notlink" => LinkState::NotALink,
                "noplace" => LinkState::NoPlace(id),
                "noaccess" => LinkState::NoAccess,
                _ => LinkState::Idle,
            };
            self.link_open = true;
        }
        let first = || match &self.games {
            Games::Loaded(list) => list.experiences.first().cloned(),
            _ => None,
        };
        self.dialog = match std::env::var(DIALOG_VARIABLE).as_deref() {
            Ok("localcopy") => first().map(|experience| Dialog::LocalCopy {
                path: self
                    .recent
                    .first()
                    .map(|place| place.path.clone())
                    .unwrap_or_default(),
                experience,
                replace: false,
            }),
            Ok("downloading") => first().map(|experience| Dialog::Downloading { experience }),
            Ok("error") => first().map(|experience| Dialog::Error {
                title: format!("Couldn\u{2019}t download {}", experience.name),
                experience: Some(experience),
                status: Some(403),
                reason: "legacy-asset:manage is not granted for this experience".to_string(),
            }),
            _ => None,
        };
        if std::env::var(DIALOG_VARIABLE).as_deref() == Ok("newexperience") {
            self.open_new_experience(window, cx);
        }
    }
}
