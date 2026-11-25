//! Recent's per-place menu, the one Studio's start page has: Configure
//! Experience, Configure Place, Open Place page, Archive (Restore for an
//! archived game), Remove from list; and the list/icons switch.

use gpui_kit::*;

use super::{Dialog, HomeWindow};
use crate::cli::Configure;
use crate::home::{self, RecentPlace};
use crate::settings::Settings;

/// The menu, while it is up, and where it was opened.
pub(crate) struct RecentMenu {
    pub(super) place: RecentPlace,
    pub(super) at: Point<Pixels>,
    /// Focused while the menu is up, so Escape reaches it: Home has no
    /// focused element of its own for a key to land on otherwise.
    pub(super) focus: FocusHandle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RecentAction {
    ConfigureExperience,
    ConfigurePlace,
    OpenPage,
    Archive,
    Restore,
    Remove,
}

impl RecentAction {
    /// The menu, in order; like Studio's Archive tab, an archived game
    /// gets Restore where the others have Archive.
    pub(super) fn menu(archived: bool) -> [RecentAction; 5] {
        [
            RecentAction::ConfigureExperience,
            RecentAction::ConfigurePlace,
            RecentAction::OpenPage,
            if archived {
                RecentAction::Restore
            } else {
                RecentAction::Archive
            },
            RecentAction::Remove,
        ]
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            RecentAction::ConfigureExperience => "Configure Experience",
            RecentAction::ConfigurePlace => "Configure Place",
            RecentAction::OpenPage => "Open Place page",
            RecentAction::Archive => "Archive",
            RecentAction::Restore => "Restore",
            RecentAction::Remove => "Remove from list",
        }
    }

    /// Why the entry is greyed out for `place`, or `None` when it works.
    /// Everything but Remove is about the Roblox place, which a file that
    /// was never linked doesn't have.
    /// Configure opens Game Settings, and Archive/Restore call Roblox, so
    /// those are off `offline` too.
    pub(super) fn unavailable(
        self,
        place: &RecentPlace,
        signed_in: bool,
        offline: bool,
    ) -> Option<&'static str> {
        let linked = place.universe_id.is_some() && place.place_id.is_some();
        match self {
            RecentAction::Remove => None,
            _ if !linked => Some("Local only: this file isn't linked to a Roblox place"),
            RecentAction::OpenPage => None,
            _ if offline => Some(crate::offline::TIP),
            RecentAction::Archive if !signed_in => Some(
                "Archiving needs a Roblox sign-in: in the editor, Settings \u{203a} Account \u{203a} Roblox sign-in",
            ),
            RecentAction::Restore if !signed_in => Some(
                "Restoring needs a Roblox sign-in: in the editor, Settings \u{203a} Account \u{203a} Roblox sign-in",
            ),
            _ => None,
        }
    }
}

/// `RBX_STUDIO_LAUNCHER_ARCHIVE=ok|<reason>` answers Archive or Restore without
/// calling Roblox, for captures and for trying the flow on a real game
/// without archiving it.
const ARCHIVE_VARIABLE: &str = "RBX_STUDIO_LAUNCHER_ARCHIVE";

/// `RBX_STUDIO_LAUNCHER_RECENT=list|icons`, plus `@menu`, `@archive` or
/// `@restore` for the first place's menu or Archive/Restore dialog: Recent's opening state for a
/// capture. Nothing is saved.
const RECENT_VARIABLE: &str = "RBX_STUDIO_LAUNCHER_RECENT";

/// The Archive/Restore dialog's line for `err`, shown as is: a refusal already
/// says Roblox refused it. A 401 means the stored sign-in expired or was
/// signed out elsewhere, which Roblox's "User is not authenticated" doesn't
/// say how to fix.
fn archive_failure(err: &rbx_cloud::CloudError) -> String {
    match err {
        rbx_cloud::CloudError::Refused { status: 401, .. } => format!(
            "{err}. Your Roblox sign-in has expired: sign in again in the editor, Settings \u{203a} Account \u{203a} Roblox sign-in."
        ),
        _ => err.to_string(),
    }
}

impl HomeWindow {
    pub(super) fn set_recent_icons(&mut self, on: bool, cx: &mut Context<Self>) {
        self.recent_icons = on;
        // The editor saves the settings it booted with, so its copy has to agree.
        if let Some(boot) = self.boot.borrow_mut().as_mut() {
            boot.settings.recent_icons = on;
        }
        let mut settings = Settings::load();
        settings.recent_icons = on;
        if let Err(err) = settings.save() {
            eprintln!("rbxstudio: could not save the Recent view: {err}");
        }
        cx.notify();
    }

    pub(super) fn open_recent_menu(
        &mut self,
        place: RecentPlace,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        self.recent_menu = Some(RecentMenu { place, at, focus });
        cx.notify();
        self.load_session(cx);
    }

    /// Reads the editor's Roblox web sign-in, what Archive needs, from the
    /// keyring. Again on every menu, since it can sign in meanwhile.
    pub(super) fn load_session(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let session = crate::key_store::read_session(cx).await.ok().flatten();
            let _ = this.update(cx, |this, cx| {
                this.session = session;
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn run_recent_action(&mut self, action: RecentAction, cx: &mut Context<Self>) {
        let Some(RecentMenu { place, .. }) = self.recent_menu.take() else {
            return;
        };
        cx.notify();
        if action
            .unavailable(&place, self.session.is_some(), crate::offline::blocked())
            .is_some()
        {
            return;
        }
        match action {
            RecentAction::ConfigureExperience => self.configure(&place, Configure::Experience, cx),
            RecentAction::ConfigurePlace => self.configure(&place, Configure::Place, cx),
            RecentAction::OpenPage => {
                if let Some(id) = place.place_id {
                    cx.open_url(&format!("https://www.roblox.com/games/{id}"));
                }
            }
            RecentAction::Archive | RecentAction::Restore => {
                self.dialog = Some(Dialog::Archive {
                    place,
                    restore: action == RecentAction::Restore,
                    busy: false,
                    error: None,
                });
            }
            RecentAction::Remove => self.forget_recent(&place),
        }
    }

    /// Game Settings lives in the editor, where the place file's own
    /// settings are, so this opens the place and the dialog on top of it.
    fn configure(&mut self, place: &RecentPlace, page: Configure, cx: &mut Context<Self>) {
        if let Some(boot) = self.boot.borrow_mut().as_mut() {
            boot.launch.configure = Some(page);
        }
        self.open_recent(place, cx);
    }

    fn forget_recent(&mut self, place: &RecentPlace) {
        match home::forget(&place.path) {
            Ok(()) => self.recent.retain(|p| p.path != place.path),
            Err(err) => {
                self.dialog = Some(Dialog::Error {
                    experience: None,
                    title: "Couldn\u{2019}t update Recent".to_string(),
                    status: None,
                    reason: err.to_string(),
                })
            }
        }
    }

    pub(super) fn confirm_archive(&mut self, cx: &mut Context<Self>) {
        let Some(Dialog::Archive {
            place,
            restore,
            busy,
            error,
        }) = &mut self.dialog
        else {
            return;
        };
        let archived = !*restore;
        let Some(universe_id) = place.universe_id else {
            return;
        };
        let mock = std::env::var(ARCHIVE_VARIABLE).ok();
        let session = self.session.clone();
        *busy = true;
        *error = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    match (mock.as_deref(), session) {
                        (Some("ok"), _) => Ok(()),
                        (Some(reason), _) => Err(reason.to_string()),
                        (None, Some(cookie)) => {
                            rbx_cloud::set_experience_archived(&cookie, universe_id, archived)
                                .map_err(|err| archive_failure(&err))
                        }
                        (None, None) => Err("Not signed in to Roblox.".to_string()),
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                let Some(Dialog::Archive { busy, error, .. }) = &mut this.dialog else {
                    return;
                };
                match result {
                    Ok(()) => {
                        this.dialog = None;
                        this.set_archived(universe_id, archived);
                    }
                    Err(reason) => {
                        *busy = false;
                        *error = Some(reason);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn apply_recent_capture(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Ok(value) = std::env::var(RECENT_VARIABLE) else {
            return;
        };
        let (view, then) = value.split_once('@').unwrap_or((&value, ""));
        self.recent_icons = view == "icons";
        let Some(place) = self.recent.first().cloned() else {
            return;
        };
        match then {
            "menu" => {
                let size = window.viewport_size();
                let at = point(size.width * 0.45, size.height * 0.5);
                self.open_recent_menu(place, at, window, cx);
            }
            "archive" | "restore" => {
                self.dialog = Some(Dialog::Archive {
                    place,
                    restore: then == "restore",
                    busy: false,
                    error: None,
                })
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    // Not `super::*`: gpui's own `test` attribute would come along with it.
    use super::{archive_failure, RecentAction, RecentPlace};
    use rbx_cloud::CloudError;

    fn place(linked: bool) -> RecentPlace {
        RecentPlace {
            path: "a.rbxl".into(),
            universe_id: linked.then_some(1),
            place_id: linked.then_some(2),
            name: None,
            opened: None,
        }
    }

    #[test]
    fn a_local_file_can_only_be_removed() {
        let local = place(false);
        for action in RecentAction::menu(false) {
            let off = action.unavailable(&local, true, false).is_some();
            assert_eq!(off, action != RecentAction::Remove, "{action:?}");
        }
    }

    #[test]
    fn a_linked_place_needs_a_sign_in_only_to_archive_or_restore() {
        let linked = place(true);
        for action in RecentAction::menu(false)
            .into_iter()
            .chain(RecentAction::menu(true))
        {
            assert_eq!(action.unavailable(&linked, true, false), None, "{action:?}");
            let off = action.unavailable(&linked, false, false).is_some();
            let needs = matches!(action, RecentAction::Archive | RecentAction::Restore);
            assert_eq!(off, needs, "{action:?}");
        }
    }

    #[test]
    fn offline_only_the_page_and_remove_stay_live() {
        let linked = place(true);
        for action in RecentAction::menu(false)
            .into_iter()
            .chain(RecentAction::menu(true))
        {
            let off = action.unavailable(&linked, true, true);
            let live = matches!(action, RecentAction::OpenPage | RecentAction::Remove);
            assert_eq!(off.is_none(), live, "{action:?}");
            if !live {
                assert_eq!(off, Some(crate::offline::TIP), "{action:?}");
            }
        }
    }

    #[test]
    fn an_archived_game_offers_restore_instead_of_archive() {
        let archived = RecentAction::menu(true);
        assert!(archived.contains(&RecentAction::Restore));
        assert!(!archived.contains(&RecentAction::Archive));
        let live = RecentAction::menu(false);
        assert!(live.contains(&RecentAction::Archive));
        assert!(!live.contains(&RecentAction::Restore));
    }

    #[test]
    fn a_refusal_names_roblox_once() {
        let line = archive_failure(&CloudError::Refused {
            status: 400,
            message: "The universe is already archived.".into(),
        });
        assert_eq!(
            line,
            "Roblox refused it (400): The universe is already archived."
        );
    }

    /// What develop.roblox.com answered a stale cookie with, live.
    #[test]
    fn an_expired_sign_in_says_to_sign_in_again() {
        let line = archive_failure(&CloudError::Refused {
            status: 401,
            message: "User is not authenticated".into(),
        });
        assert!(
            line.starts_with("Roblox refused it (401): User is not authenticated."),
            "{line}"
        );
        assert!(line.contains("Roblox sign-in"), "{line}");
    }
}
