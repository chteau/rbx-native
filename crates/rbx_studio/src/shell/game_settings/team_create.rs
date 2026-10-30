//! What runs as the editor's Roblox sign-in rather than the API key:
//! Basic Info's Team Create switch and who is in the place's live session,
//! and Security's Allow Mesh / Image APIs switch. Each switch keeps its own
//! draft beside the cloud one and joins the same Save and Cancel.
//!
//! Under `RBX_STUDIO_GAME_SETTINGS_MOCK` (see `fetch`) all of it answers as
//! signed in, with a canned session, and `=signed-out` answers as signed out.

use std::time::Duration;

use gpui_kit::*;
use rbx_cloud::heartbeats::SessionCookie;
use rbx_cloud::{CloudError, MeshImageApis, SessionMember};

use super::model::SwitchDraft;
use super::{fetch, GameSettingsWindow, Loading};
use crate::home::Link;

/// How often the session list is read again while the window is open.
const MEMBERS_POLL: Duration = Duration::from_secs(15);

/// Who the calls run as: the stored sign-in, or the capture mock's stand-in.
#[derive(Clone)]
pub(super) enum Web {
    Live(SessionCookie),
    Mock,
}

impl Web {
    fn of(session: Option<SessionCookie>) -> Option<Web> {
        match fetch::mock_mode() {
            Some(mode) => (mode.trim() != "signed-out").then_some(Web::Mock),
            None => session.map(Web::Live),
        }
    }
}

pub(super) struct TeamCreate {
    /// `None` signed out.
    pub(super) web: Option<Web>,
    pub(super) draft: SwitchDraft,
    pub(super) loading: Loading,
    /// Bumped by every read and save of the switch, so a reply that lost the
    /// race is dropped.
    pub(super) generation: u64,
    /// Who is in the place's live session, once read.
    pub(super) members: Option<Result<Vec<SessionMember>, String>>,
    /// Security's Allow Mesh / Image APIs, read beside Team Create.
    pub(super) mesh_apis: SwitchDraft,
    pub(super) mesh_apis_loading: Loading,
    /// False when a group owns the experience and the sign-in isn't the
    /// group's owner; Studio greys the switch out then.
    pub(super) mesh_apis_editable: bool,
}

/// What a save sends as the sign-in: who as, the universe, and each edited
/// switch's new value.
#[derive(Clone)]
pub(super) struct WebSave {
    web: Web,
    universe_id: u64,
    team_create: Option<bool>,
    mesh_apis: Option<bool>,
}

/// What Roblox holds after a [`WebSave`], per switch that was sent.
pub(super) struct WebSaved {
    pub(super) team_create: Option<Result<bool, String>>,
    pub(super) mesh_apis: Option<Result<bool, String>>,
}

impl TeamCreate {
    pub(super) fn new(session: Option<SessionCookie>) -> Self {
        TeamCreate {
            web: Web::of(session),
            draft: SwitchDraft::default(),
            loading: Loading::Idle,
            generation: 0,
            members: None,
            mesh_apis: SwitchDraft::default(),
            mesh_apis_loading: Loading::Idle,
            mesh_apis_editable: true,
        }
    }

    pub(super) fn is_dirty(&self) -> bool {
        self.draft.pending().is_some() || self.mesh_apis.pending().is_some()
    }

    pub(super) fn revert(&mut self) {
        self.draft.revert();
        self.mesh_apis.revert();
    }

    /// What a save sends; `None` with nothing edited, unlinked or signed out.
    pub(super) fn pending(&self, link: Option<&Link>) -> Option<WebSave> {
        let save = WebSave {
            web: self.web.clone()?,
            universe_id: link?.universe_id,
            team_create: self.draft.pending(),
            mesh_apis: self.mesh_apis.pending(),
        };
        (save.team_create.is_some() || save.mesh_apis.is_some()).then_some(save)
    }

    /// Takes in what a save left on Roblox; a refused switch keeps its edit.
    pub(super) fn saved(&mut self, saved: WebSaved, refusals: &mut Vec<String>) {
        for (draft, reply) in [
            (&mut self.draft, saved.team_create),
            (&mut self.mesh_apis, saved.mesh_apis),
        ] {
            match reply {
                Some(Ok(on)) => draft.rebase(on),
                Some(Err(text)) => refusals.push(text),
                None => {}
            }
        }
    }
}

impl GameSettingsWindow {
    /// Reads the session list again every [`MEMBERS_POLL`] for as long as
    /// the window lives.
    pub(super) fn poll_members(cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(MEMBERS_POLL).await;
            if this
                .update(cx, |this, cx| this.refresh_members(cx))
                .is_err()
            {
                break;
            }
        })
        .detach();
    }

    /// Catches up with the editor signing in or out meanwhile.
    pub(super) fn follow_sign_in(
        &mut self,
        session: Option<SessionCookie>,
        cx: &mut Context<Self>,
    ) {
        let web = Web::of(session);
        if web.is_some() != self.team_create.web.is_some() {
            self.team_create.web = web;
            self.load_team_create(cx);
        }
    }

    /// Reads both switches and the session list for the current link,
    /// dropping the switches' edits (nothing unlinked or signed out).
    pub(super) fn load_team_create(&mut self, cx: &mut Context<Self>) {
        let tc = &mut self.team_create;
        tc.generation += 1;
        let generation = tc.generation;
        tc.draft = SwitchDraft::default();
        tc.mesh_apis = SwitchDraft::default();
        tc.mesh_apis_editable = true;
        tc.members = None;
        let (Some(link), Some(web)) = (self.link.clone(), tc.web.clone()) else {
            tc.loading = Loading::Idle;
            tc.mesh_apis_loading = Loading::Idle;
            cx.notify();
            return;
        };
        tc.loading = Loading::Busy;
        tc.mesh_apis_loading = Loading::Busy;
        self.refresh_members(cx);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let (team_create, mesh_apis) = cx
                .background_spawn(async move {
                    (
                        read(&web, link.universe_id),
                        read_mesh_apis(&web, link.universe_id),
                    )
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                let tc = &mut this.team_create;
                if generation != tc.generation {
                    return;
                }
                match team_create {
                    Ok(on) => {
                        tc.draft.reset(on);
                        tc.loading = Loading::Idle;
                    }
                    Err(message) => tc.loading = Loading::Failed(message),
                }
                match mesh_apis {
                    Ok(apis) => {
                        tc.mesh_apis.reset(apis.allowed);
                        tc.mesh_apis_editable = apis.can_edit;
                        tc.mesh_apis_loading = Loading::Idle;
                    }
                    Err(message) => tc.mesh_apis_loading = Loading::Failed(message),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn refresh_members(&mut self, cx: &mut Context<Self>) {
        let (Some(link), Some(web)) = (self.link.clone(), self.team_create.web.clone()) else {
            return;
        };
        let place_id = link.place_id;
        cx.spawn(async move |this, cx| {
            let reply = cx
                .background_spawn(async move { members(&web, place_id) })
                .await;
            let _ = this.update(cx, |this, cx| {
                // Dropped if the link moved or the editor signed out meanwhile.
                if this.team_create.web.is_some()
                    && this.link.as_ref().map(|l| l.place_id) == Some(place_id)
                {
                    this.team_create.members = Some(reply);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn set_team_create(&mut self, on: bool, cx: &mut Context<Self>) {
        self.team_create.draft.set(on);
        self.notice = None;
        cx.notify();
    }

    pub(super) fn set_mesh_apis(&mut self, on: bool, cx: &mut Context<Self>) {
        self.team_create.mesh_apis.set(on);
        self.notice = None;
        cx.notify();
    }
}

fn read_mesh_apis(web: &Web, universe_id: u64) -> Result<MeshImageApis, String> {
    match web {
        Web::Live(cookie) => {
            rbx_cloud::mesh_image_apis(cookie, universe_id).map_err(|e| describe(&e))
        }
        Web::Mock => Ok(MeshImageApis {
            allowed: false,
            can_edit: true,
        }),
    }
}

fn read(web: &Web, universe_id: u64) -> Result<bool, String> {
    match web {
        Web::Live(cookie) => {
            rbx_cloud::team_create_enabled(cookie, universe_id).map_err(|e| describe(&e))
        }
        Web::Mock => Ok(true),
    }
}

/// Sends each edited switch and returns what Roblox then holds (the sent
/// value when the re-read fails: the write itself was accepted).
pub(super) fn save(save: &WebSave) -> WebSaved {
    let (web, universe_id) = (&save.web, save.universe_id);
    WebSaved {
        team_create: save.team_create.map(|on| match web {
            Web::Live(cookie) => {
                rbx_cloud::set_team_create_enabled(cookie, universe_id, on)
                    .map_err(|e| describe(&e))?;
                Ok(rbx_cloud::team_create_enabled(cookie, universe_id).unwrap_or(on))
            }
            Web::Mock => Ok(on),
        }),
        mesh_apis: save.mesh_apis.map(|on| match web {
            Web::Live(cookie) => {
                rbx_cloud::set_mesh_image_apis_allowed(cookie, universe_id, on)
                    .map_err(|e| describe(&e))?;
                Ok(rbx_cloud::mesh_image_apis(cookie, universe_id).map_or(on, |a| a.allowed))
            }
            Web::Mock => Ok(on),
        }),
    }
}

fn members(web: &Web, place_id: u64) -> Result<Vec<SessionMember>, String> {
    match web {
        Web::Live(cookie) => {
            rbx_cloud::team_create_members(cookie, place_id).map_err(|e| describe(&e))
        }
        Web::Mock => Ok(vec![
            SessionMember {
                id: 1,
                name: "builderman".to_string(),
                display_name: "Builder".to_string(),
            },
            SessionMember {
                id: 2,
                name: "obby_architect".to_string(),
                display_name: "Mara".to_string(),
            },
        ]),
    }
}

/// Roblox's own message, as is. Not `fetch::describe`: its API-key wording
/// is wrong for the sign-in. A 401 means the stored sign-in expired, which
/// Roblox's "Authorization has been denied" doesn't say how to fix.
fn describe(err: &CloudError) -> String {
    match err {
        CloudError::Refused { status: 401, .. } => format!(
            "{err}. Your Roblox sign-in has expired: sign in again in Settings \u{203a} Account \u{203a} Roblox sign-in."
        ),
        _ => err.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{describe, save, CloudError, Link, TeamCreate, Web, WebSave, WebSaved};

    #[test]
    fn an_expired_sign_in_says_how_to_fix_it_and_other_refusals_stay_roblox_s() {
        let expired = describe(&CloudError::Refused {
            status: 401,
            message: "Authorization has been denied for this request.".to_string(),
        });
        assert!(
            expired.contains("Authorization has been denied") && expired.contains("sign in again")
        );
        let refused = describe(&CloudError::Refused {
            status: 403,
            message: "You are not authorized to configure this universe.".to_string(),
        });
        assert!(refused.ends_with("You are not authorized to configure this universe."));
    }

    #[test]
    fn a_save_needs_a_sign_in_a_link_and_an_edit() {
        let link = Link {
            universe_id: 9,
            place_id: 3,
            name: None,
        };
        let mut tc = TeamCreate {
            web: Some(Web::Mock),
            ..TeamCreate::new(None)
        };
        tc.draft.reset(false);
        tc.mesh_apis.reset(false);
        assert!(tc.pending(Some(&link)).is_none());
        tc.draft.set(true);
        assert!(matches!(
            tc.pending(Some(&link)),
            Some(WebSave {
                web: Web::Mock,
                universe_id: 9,
                team_create: Some(true),
                mesh_apis: None,
            })
        ));
        tc.mesh_apis.set(true);
        assert!(matches!(
            tc.pending(Some(&link)),
            Some(WebSave {
                team_create: Some(true),
                mesh_apis: Some(true),
                ..
            })
        ));
        assert!(tc.pending(None).is_none());
        tc.web = None;
        assert!(tc.pending(Some(&link)).is_none());
    }

    #[test]
    fn a_save_sends_only_the_edited_switches_and_a_refusal_keeps_its_edit() {
        let saved = save(&WebSave {
            web: Web::Mock,
            universe_id: 9,
            team_create: None,
            mesh_apis: Some(true),
        });
        assert!(saved.team_create.is_none());
        assert_eq!(saved.mesh_apis, Some(Ok(true)));

        let mut tc = TeamCreate::new(None);
        tc.draft.reset(false);
        tc.mesh_apis.reset(false);
        tc.draft.set(true);
        tc.mesh_apis.set(true);
        let mut refusals = Vec::new();
        tc.saved(
            WebSaved {
                team_create: Some(Err("no".to_string())),
                mesh_apis: Some(Ok(true)),
            },
            &mut refusals,
        );
        assert_eq!(refusals, ["no"]);
        assert_eq!(tc.draft.pending(), Some(true));
        assert_eq!(
            (tc.mesh_apis.value(), tc.mesh_apis.pending()),
            (Some(true), None)
        );
        assert!(tc.is_dirty());
        tc.revert();
        assert!(!tc.is_dirty());
    }
}
