//! What turning presence on puts into the place (the key and the relay,
//! with no save button), and keeping this editor on the key saved on
//! Roblox (see `rbx_presence::converge`): a place opened without a key
//! takes the saved one before making its own, and when the beacon says
//! another editor uses a different key the saved place is read again and
//! its key adopted.

use std::time::{Duration, Instant};

use gpui_kit::*;
use rbx_presence::converge::Next;
use rbx_presence::relay::{RelayConfig, RelayKind};

use super::Connection;
use crate::command_bar::Feedback;
use crate::shell::Shell;

/// The saved place is read again at most this often while a conflict
/// lasts; its version is checked first, so an unchanged place is not
/// downloaded again.
const RECHECK: Duration = Duration::from_secs(60);

/// Reading the key out of the place saved on Roblox.
#[derive(Default)]
pub(in crate::shell) struct KeyCheck {
    in_flight: bool,
    last_at: Option<Instant>,
    /// The saved version last read and the key it held.
    seen: Option<(u64, Option<String>)>,
    /// This connect already looked for a saved key before making one.
    pub(super) asked_on_connect: bool,
    /// What the last read said to do about a conflict, or why it failed.
    pub(super) next: Option<Result<Next, String>>,
}

/// The relay the owner setup names: `kind` with `url`, a blank URL being
/// Auto. `None` when `kind` needs a URL and none was typed.
pub(super) fn owner_relay(kind: RelayKind, url: &str) -> Option<RelayConfig> {
    let url = url.trim();
    match (url.is_empty(), kind) {
        (true, RelayKind::Hosted) => Some(RelayConfig::community()),
        (true, _) => None,
        (false, kind) => Some(RelayConfig {
            kind,
            url: url.to_owned(),
        }),
    }
}

/// The key in `place` as last saved on Roblox: `known` is the version and
/// key read before, returned as they are while the place has no newer
/// version. Blocking; run off the UI thread.
fn fetch_saved_key(
    place: u64,
    known: Option<(u64, Option<String>)>,
) -> Result<(Option<u64>, Option<String>), String> {
    let client = rbx_cloud::Client::new(rbx_cloud::ApiKey::from_env_or_config());
    // Version history needs a scope not every key has; without it the place
    // is just downloaded again.
    let newest = client
        .place_versions(place, None, None)
        .ok()
        .and_then(|page| page.versions.first().map(|v| v.version));
    if let (Some(version), Some((seen, key))) = (newest, &known) {
        if version == *seen {
            return Ok((newest, key.clone()));
        }
    }
    let bytes = client
        .download_place(place)
        .map_err(|err| err.to_string())?;
    let dom = rbx_viewer::parse_place(&bytes, std::path::Path::new("the saved place"))?;
    Ok((newest, rbx_presence::secret::read(&dom)))
}

/// Whether filling in the presence key is an edit to save. Copying in the
/// key the place already has on Roblox (`saved`, none here yet) only
/// catches this copy up: no undo step, and quitting asks nothing. A fresh
/// key or a new relay is the user's to save or publish.
fn is_edit(had: bool, saved: bool) -> bool {
    had || !saved
}

impl Shell {
    /// Makes sure the place holds the key and the relay, writing what is
    /// missing in one undoable edit: the key from the saved place when one
    /// was found (`saved`), else a fresh one, and the owner setup's relay.
    /// Returns the key, or `None` when the place has no `ServerStorage`.
    pub(super) fn presence_setup(
        &mut self,
        saved: Option<String>,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        let had = rbx_presence::secret::read(&self.dom);
        let relay = rbx_presence::secret::read_relay(&self.dom);
        if let (Some(secret), Some(_)) = (&had, &relay) {
            return Some(secret.clone());
        }
        let edit = is_edit(had.is_some(), saved.is_some());
        let unsaved = self.recovery.unsaved();
        if edit {
            self.push_history();
        } else {
            self.dom.take_changes();
        }
        let minted = had.is_none() && saved.is_none();
        let secret = had
            .clone()
            .or(saved)
            .unwrap_or_else(rbx_presence::secret::generate);
        let config = owner_relay(self.team_presence.owner_kind, &self.relay_url(cx))
            .unwrap_or_else(RelayConfig::community);
        let written = (had.is_some()
            || rbx_presence::secret::write(&mut self.dom, &secret).is_some())
            && (relay.is_some()
                || rbx_presence::secret::write_relay(&mut self.dom, &config).is_some());
        let changes = self.dom.take_changes();
        self.rebuild_explorer(cx);
        self.reflect_changes(&changes, cx);
        if edit {
            self.record_history_change(changes);
        } else {
            self.recovery.set_unsaved(unsaved);
        }
        if !written {
            self.output.push(
                "Presence",
                Feedback::Error(
                    "This place has no ServerStorage to keep the presence key in.".into(),
                ),
            );
            return None;
        }
        self.team_presence.minted = minted;
        let said = match (had.is_some(), minted) {
            (true, _) => {
                "Saved the presence relay into this place. Save or Publish so collaborators get it."
            }
            (false, true) => {
                "Put a new presence key into this place. Save or Publish so collaborators get it."
            }
            (false, false) => "Using the presence key saved in this place on Roblox.",
        };
        self.output.push("Presence", Feedback::Info(said.into()));
        Some(secret)
    }

    fn relay_url(&self, cx: &App) -> String {
        self.team_presence
            .relay_input
            .read(cx)
            .value()
            .trim()
            .to_string()
    }

    /// Owner setup saves as it changes (a kind picked, Enter or leaving the
    /// URL field): writes the relay into the place when it differs from what
    /// is there, and reconnects. Worker and ntfy wait for their URL.
    pub(super) fn save_presence_relay(&mut self, cx: &mut Context<Self>) {
        let url = self.relay_url(cx);
        // Kept as a convenience so the field survives a reload.
        self.team_presence.settings.relay_url = url.clone();
        self.save_settings();
        let Some(config) = owner_relay(self.team_presence.owner_kind, &url) else {
            self.output.push(
                "Presence",
                Feedback::Info(format!(
                    "Type the {} URL. It saves into the place when you press Enter or click away.",
                    self.team_presence.owner_kind.as_str()
                )),
            );
            cx.notify();
            return;
        };
        if rbx_presence::secret::read_relay(&self.dom).as_ref() == Some(&config) {
            return;
        }
        self.push_history();
        if rbx_presence::secret::write_relay(&mut self.dom, &config).is_none() {
            self.output.push(
                "Presence",
                Feedback::Error(
                    "This place has no ServerStorage to keep the relay setting in.".into(),
                ),
            );
            return;
        }
        let changes = self.dom.take_changes();
        self.rebuild_explorer(cx);
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);
        self.output.push(
            "Presence",
            Feedback::Info(
                "Saved the relay into this place. Save or Publish so collaborators get it.".into(),
            ),
        );
        if self.team_presence.settings.enabled && self.team_presence.connection != Connection::Mock
        {
            self.connect_presence(cx);
        }
        cx.notify();
    }

    /// Owner setup: the next transport, saved at once.
    pub(super) fn toggle_owner_kind(&mut self, cx: &mut Context<Self>) {
        self.team_presence.owner_kind = match self.team_presence.owner_kind {
            RelayKind::Hosted => RelayKind::Worker,
            RelayKind::Worker => RelayKind::Ntfy,
            RelayKind::Ntfy => RelayKind::Hosted,
        };
        self.save_presence_relay(cx);
    }

    /// Reads the key out of the saved place, off the UI thread. On connect
    /// (`on_connect`) it is the key to use when the open place has none; on a
    /// conflict it is adopted when it differs from ours. At most one read at
    /// a time, and on a conflict at most one per `RECHECK`.
    pub(super) fn check_saved_key(&mut self, place: u64, on_connect: bool, cx: &mut Context<Self>) {
        let check = &mut self.team_presence.key_check;
        if check.in_flight
            || (!on_connect && check.last_at.is_some_and(|at| at.elapsed() < RECHECK))
        {
            return;
        }
        check.in_flight = true;
        check.last_at = Some(Instant::now());
        let known = check.seen.clone();
        cx.spawn(async move |this, cx| {
            let fetched = cx
                .background_spawn(async move { fetch_saved_key(place, known) })
                .await;
            let _ = this.update(cx, |shell, cx| {
                shell.team_presence.key_check.in_flight = false;
                shell.saved_key_read(place, on_connect, fetched, cx);
            });
        })
        .detach();
    }

    fn saved_key_read(
        &mut self,
        place: u64,
        on_connect: bool,
        fetched: Result<(Option<u64>, Option<String>), String>,
        cx: &mut Context<Self>,
    ) {
        let presence = &self.team_presence;
        // Turned off, or moved to another place, while the read ran.
        if !presence.settings.enabled
            || crate::home::link_of(&self.path).map(|l| l.place_id) != Some(place)
        {
            return;
        }
        let saved = match fetched {
            Ok((version, key)) => {
                if let Some(version) = version {
                    self.team_presence.key_check.seen = Some((version, key.clone()));
                }
                Ok(key)
            }
            Err(why) => Err(why),
        };
        if on_connect {
            if let Ok(Some(key)) = &saved {
                if rbx_presence::secret::read(&self.dom).is_none() {
                    self.presence_setup(Some(key.clone()), cx);
                }
            }
            self.connect_presence(cx);
            return;
        }
        let Some(ours) = rbx_presence::secret::read(&self.dom) else {
            return;
        };
        let next = saved.map(|key| rbx_presence::converge::on_conflict(&ours, key.as_deref()));
        if let Ok(Next::Adopt(key)) = &next {
            self.adopt_saved_key(key, cx);
        }
        self.team_presence.key_check.next = Some(next);
        cx.notify();
    }

    /// Switches the place to the saved key (one undoable edit) and
    /// reconnects with it.
    fn adopt_saved_key(&mut self, key: &str, cx: &mut Context<Self>) {
        self.push_history();
        if rbx_presence::secret::write(&mut self.dom, key).is_none() {
            return;
        }
        let changes = self.dom.take_changes();
        self.rebuild_explorer(cx);
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);
        self.team_presence.minted = false;
        self.output.push(
            "Presence",
            Feedback::Info(
                "Another editor had this place open with a different presence key. Switched to the one saved on Roblox, so you all end up in the same room.".into(),
            ),
        );
        self.connect_presence(cx);
    }
}

#[cfg(test)]
mod tests {
    use super::{is_edit, owner_relay};
    use rbx_presence::relay::{RelayConfig, RelayKind};

    #[test]
    fn taking_the_key_saved_on_roblox_leaves_the_place_saved() {
        assert!(!is_edit(false, true));
        assert!(is_edit(false, false));
        assert!(is_edit(true, false));
    }

    #[test]
    fn a_blank_url_is_auto_and_only_auto_goes_without_one() {
        assert_eq!(
            owner_relay(RelayKind::Hosted, "  "),
            Some(RelayConfig::community())
        );
        assert_eq!(owner_relay(RelayKind::Worker, ""), None);
        assert_eq!(owner_relay(RelayKind::Ntfy, ""), None);
        assert_eq!(
            owner_relay(RelayKind::Worker, " https://relay.example "),
            Some(RelayConfig {
                kind: RelayKind::Worker,
                url: "https://relay.example".into(),
            })
        );
    }
}
