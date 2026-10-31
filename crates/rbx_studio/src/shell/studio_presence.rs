//! Shell ↔ "In Studio" on the user's Roblox profile: the
//! `rbx_cloud::heartbeats::Pulser` started and stopped like Discord's
//! presence (`shell::discord`), with the session from `key_store`.

use gpui_kit::{Context, Task};
use rbx_cloud::heartbeats::{Pulser, PulserEvent, SessionCookie, StudioLocation};

use super::Shell;
use crate::command_bar::Feedback;

/// How often the open place's link is re-read to move the pulse along.
pub(super) const LOCATION_CHECK: std::time::Duration = std::time::Duration::from_secs(5);

pub(super) struct StudioPresence {
    /// The setting; persisted.
    pub(super) enabled: bool,
    /// The stored sign-in, once `restore_roblox_session` has read it.
    pub(super) session: Option<SessionCookie>,
    /// The Roblox user that session belongs to, when known (sign-in tells
    /// it; a restored session does not until a pulse succeeds).
    pub(super) user_id: Option<u64>,
    /// Running while enabled and signed in.
    pulser: Option<Pulser>,
    /// Drains the pulser's events and re-reads the location every
    /// `LOCATION_CHECK`; dropped with the pulser.
    watch: Option<Task<()>>,
    /// The location last handed to the pulser.
    last: StudioLocation,
}

impl StudioPresence {
    pub(super) fn new(enabled: bool) -> Self {
        StudioPresence {
            enabled,
            session: None,
            user_id: None,
            pulser: None,
            watch: None,
            last: StudioLocation { place: None },
        }
    }

    /// Signed in, enabled, and pulsing.
    pub(super) fn active(&self) -> bool {
        self.pulser.is_some()
    }
}

impl Shell {
    /// The editor's Roblox web sign-in, for windows that follow the shell.
    pub(crate) fn roblox_session(&self) -> Option<SessionCookie> {
        self.studio_presence.session.clone()
    }

    /// Startup: reads the session from the keyring on a task; when one is
    /// there, stores it and, if `enabled`, starts pulsing.
    pub(super) fn restore_roblox_session(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let Ok(Some(session)) = crate::key_store::read_session(cx).await else {
                return;
            };
            // A stored session carries no user id; team presence needs it
            // to say who this is, so ask Roblox whose session it is.
            let probe = session.clone();
            let user_id = cx
                .background_executor()
                .spawn(async move { rbx_cloud::heartbeats::authenticated_user_id(&probe) })
                .await
                .ok();
            let _ = this.update(cx, |shell, cx| {
                shell.studio_presence.session = Some(session);
                shell.studio_presence.user_id = user_id;
                if shell.studio_presence.enabled {
                    shell.start_studio_presence(cx);
                }
                shell.retry_presence_after_sign_in(cx);
                // Windows opened before the keyring answered follow the shell.
                cx.notify();
            });
        })
        .detach();
    }

    fn start_studio_presence(&mut self, cx: &mut Context<Self>) {
        self.stop_studio_presence();
        let Some(session) = self.studio_presence.session.clone() else {
            return;
        };
        let location = self.studio_location();
        let (pulser, events) = Pulser::start(session, location);
        self.studio_presence.pulser = Some(pulser);
        self.studio_presence.last = location;
        self.studio_presence.watch = Some(cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(LOCATION_CHECK).await;
            let finished = this.update(cx, |shell, cx| {
                let mut signed_out = false;
                while let Ok(event) = events.try_recv() {
                    if event == PulserEvent::SignedOut {
                        signed_out = true;
                    }
                }
                if signed_out {
                    shell.stop_studio_presence();
                    shell.studio_presence.session = None;
                    shell.studio_presence.user_id = None;
                    rbx_cloud::expiry::report_session_refused();
                    cx.notify();
                    return true;
                }
                let now = shell.studio_location();
                if now != shell.studio_presence.last {
                    if let Some(pulser) = &shell.studio_presence.pulser {
                        pulser.set_location(now);
                    }
                    shell.studio_presence.last = now;
                }
                false
            });
            if finished.unwrap_or(true) {
                return;
            }
        }));
        cx.notify();
    }

    pub(super) fn stop_studio_presence(&mut self) {
        self.studio_presence.pulser = None;
        self.studio_presence.watch = None;
    }

    /// The Settings toggle. Turning it on without a session opens the
    /// sign-in window instead (`open_roblox_login`); saves settings.
    pub(in crate::shell) fn set_studio_presence(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.studio_presence.enabled = enabled;
        if enabled {
            if self.studio_presence.session.is_some() {
                if !self.studio_presence.active() {
                    self.start_studio_presence(cx);
                }
            } else {
                self.open_roblox_login(cx);
            }
        } else {
            self.stop_studio_presence();
        }
        self.save_settings();
        cx.notify();
    }

    /// The sign-in window succeeded: store the session in the keyring,
    /// remember it, and start pulsing if enabled.
    pub(in crate::shell) fn roblox_signed_in(
        &mut self,
        session: SessionCookie,
        user_id: u64,
        cx: &mut Context<Self>,
    ) {
        let to_store = session.clone();
        cx.spawn(async move |this, cx| {
            if let Err(error) = crate::key_store::save_session(&to_store, cx).await {
                let _ = this.update(cx, |shell, cx| {
                    shell.output.push(
                        "Roblox",
                        Feedback::Error(format!("Could not store the Roblox sign-in: {error}")),
                    );
                    cx.notify();
                });
            }
        })
        .detach();
        self.studio_presence.session = Some(session);
        self.studio_presence.user_id = Some(user_id);
        if self.studio_presence.enabled {
            self.start_studio_presence(cx);
        }
        self.retry_presence_after_sign_in(cx);
        cx.notify();
    }

    /// Settings › Sign out: stop pulsing, forget the session in memory and
    /// in the keyring.
    pub(in crate::shell) fn roblox_sign_out(&mut self, cx: &mut Context<Self>) {
        self.stop_studio_presence();
        self.studio_presence.session = None;
        self.studio_presence.user_id = None;
        cx.spawn(async move |this, cx| {
            if let Err(error) = crate::key_store::forget_session(cx).await {
                let _ = this.update(cx, |shell, cx| {
                    shell.output.push(
                        "Roblox",
                        Feedback::Error(format!("Could not remove the Roblox sign-in: {error}")),
                    );
                    cx.notify();
                });
            }
        })
        .detach();
        cx.notify();
    }

    /// `StudioLocation` for the open file: its `home::link_of` place, or none.
    pub(super) fn studio_location(&self) -> StudioLocation {
        StudioLocation {
            place: crate::home::link_of(&self.path).map(|l| (l.place_id, l.universe_id)),
        }
    }
}
