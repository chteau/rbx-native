//! Home: New / Recent / My Games, the sidebar with its
//! key card, and the open flow — LocalCopy, Downloading, DownloadError —
//! that ends in the editor. State and flows live here; the pages are drawn
//! in `view`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;
use rbx_cloud::{ApiKey, Client, Experience, Experiences, Grant, KeyReport};

use super::{fixtures, Boot};
use crate::home::{self, RecentPlace};

mod open;
mod view;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Page {
    Home,
    Recent,
    MyGames,
}

/// What Home knows about the stored key.
pub(super) enum KeyState {
    Missing,
    Checking,
    Ready {
        owner: String,
        report: KeyReport,
    },
    /// Stored, but the check failed (refused, network) — Home still lists
    /// what it can.
    Unchecked,
}

pub(super) enum Games {
    Loading,
    Loaded(Experiences),
    Failed(String),
}

#[derive(Clone, PartialEq, Eq)]
pub(super) enum LinkState {
    Idle,
    Resolving,
    NotALink,
    NoPlace(u64),
    NoAccess,
    Unreachable,
}

pub(super) enum Dialog {
    LocalCopy {
        experience: Experience,
        path: PathBuf,
        replace: bool,
    },
    Downloading {
        experience: Experience,
    },
    Error {
        experience: Option<Experience>,
        title: String,
        status: Option<u16>,
        reason: String,
    },
}

pub(crate) struct HomeWindow {
    pub(super) boot: Boot,
    pub(super) page: Page,
    pub(super) key: KeyState,
    pub(super) games: Games,
    pub(super) icons: HashMap<u64, Arc<RenderImage>>,
    pub(super) recent: Vec<RecentPlace>,
    pub(super) search: Entity<InputState>,
    pub(super) link: Entity<InputState>,
    pub(super) link_state: LinkState,
    /// The "Add by link" field row, when the note that normally holds it is
    /// hidden (see [`HomeWindow::note_visible`]).
    pub(super) link_open: bool,
    /// Whether the link field has the caret: unfocused, a long link shows
    /// ellipsized instead of clipped (the kit's input can only scroll).
    pub(super) link_focused: bool,
    pub(super) dialog: Option<Dialog>,
    /// Bumped per open, so Cancel drops a download still in flight.
    pub(super) serial: u64,
    pub(super) handle: AnyWindowHandle,
    _subscriptions: Vec<Subscription>,
}

/// `RBX_STUDIO_LAUNCHER_PAGE=home|recent|games` and
/// `RBX_STUDIO_LAUNCHER_DIALOG=localcopy|downloading|error` (with
/// `RBX_STUDIO_LAUNCHER_LINK=<text>|<text>@resolving|@notlink|@noplace|@noaccess`):
/// Home's opening state for a capture.
const PAGE_VARIABLE: &str = "RBX_STUDIO_LAUNCHER_PAGE";
const DIALOG_VARIABLE: &str = "RBX_STUDIO_LAUNCHER_DIALOG";
const LINK_VARIABLE: &str = "RBX_STUDIO_LAUNCHER_LINK";

impl HomeWindow {
    pub(super) fn new(boot: Boot, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        let link = cx
            .new(|cx| InputState::new(window, cx).placeholder("Place ID or roblox.com/games link"));
        let subscriptions = vec![
            cx.subscribe(&search, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
            cx.subscribe_in(
                &link,
                window,
                |this, _, event: &InputEvent, window, cx| match event {
                    InputEvent::Change => {
                        if this.link_state != LinkState::Resolving {
                            this.link_state = LinkState::Idle;
                        }
                        cx.notify();
                    }
                    InputEvent::PressEnter { .. } => this.add_link(window, cx),
                    InputEvent::Focus => {
                        this.link_focused = true;
                        cx.notify();
                    }
                    InputEvent::Blur => {
                        this.link_focused = false;
                        if this.link.read(cx).value().is_empty() {
                            this.link_open = false;
                        }
                        cx.notify();
                    }
                },
            ),
        ];
        let mut this = HomeWindow {
            boot,
            page: match std::env::var(PAGE_VARIABLE).as_deref() {
                Ok("recent") => Page::Recent,
                Ok("games") => Page::MyGames,
                _ => Page::Home,
            },
            key: KeyState::Missing,
            games: Games::Loading,
            icons: HashMap::new(),
            recent: home::recent(),
            search,
            link,
            link_state: LinkState::Idle,
            link_open: false,
            link_focused: false,
            dialog: None,
            serial: 0,
            handle: window.window_handle(),
            _subscriptions: subscriptions,
        };
        this.reload(cx);
        this.apply_capture_state(window, cx);
        this
    }

    fn apply_capture_state(&mut self, window: &mut Window, cx: &mut Context<Self>) {
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
    }

    /// (Re)reads the key and My Games — at open, and after the key was
    /// replaced or removed in Roblox publishing.
    pub(super) fn reload(&mut self, cx: &mut Context<Self>) {
        self.recent = home::recent();
        if let Ok(which) = std::env::var(fixtures::GAMES_VARIABLE) {
            let key = std::env::var(super::key_check::FIXTURE_VARIABLE)
                .unwrap_or_else(|_| "ready".to_string());
            if let super::key_check::Status::Done(checked) = fixtures::key_status(&key) {
                self.key = KeyState::Ready {
                    owner: checked.owner,
                    report: checked.report,
                };
            }
            if which == "nokey" {
                self.key = KeyState::Missing;
            }
            self.games = match fixtures::games(&which) {
                Some(games) => Games::Loaded(games),
                None => Games::Loading,
            };
            return;
        }
        let Some(key) = ApiKey::from_env_or_config() else {
            self.key = KeyState::Missing;
            self.games = Games::Loaded(Experiences::default());
            cx.notify();
            return;
        };
        self.key = KeyState::Checking;
        self.games = Games::Loading;
        self.icons.clear();
        cx.notify();
        let client = Client::new(Some(key));
        cx.spawn(async move |this, cx| {
            let checked = cx
                .background_spawn({
                    let client = client.clone();
                    async move {
                        let info = client.introspect().ok()?;
                        let owner = client
                            .user_display_name(info.authorized_user_id)
                            .unwrap_or_else(|_| format!("User {}", info.authorized_user_id));
                        Some((owner, rbx_cloud::check_scopes(&info)))
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.key = match checked {
                    Some((owner, report)) => KeyState::Ready { owner, report },
                    None => KeyState::Unchecked,
                };
                cx.notify();
            });
            let games = cx
                .background_spawn({
                    let client = client.clone();
                    async move { client.list_experiences() }
                })
                .await;
            let ids: Vec<u64> = match &games {
                Ok(list) => list.experiences.iter().map(|e| e.universe_id).collect(),
                Err(_) => Vec::new(),
            };
            let _ = this.update(cx, |this, cx| {
                this.games = match games {
                    Ok(list) => Games::Loaded(list),
                    Err(err) => Games::Failed(err.to_string()),
                };
                cx.notify();
            });
            let urls = cx
                .background_spawn({
                    let client = client.clone();
                    async move { client.game_icon_urls(&ids).unwrap_or_default() }
                })
                .await;
            for (universe_id, url) in urls {
                let client = client.clone();
                let image = cx
                    .background_spawn(
                        async move { decode_icon(&client.download_image(&url).ok()?) },
                    )
                    .await;
                if let Some(image) = image {
                    let _ = this.update(cx, |this, cx| {
                        this.icons.insert(universe_id, image);
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }

    /// The report's grants, when the key was checked.
    fn report(&self) -> Option<&KeyReport> {
        match &self.key {
            KeyState::Ready { report, .. } => Some(report),
            _ => None,
        }
    }

    fn granted(&self, scope: &str) -> bool {
        self.report().is_some_and(|report| {
            report
                .checks
                .iter()
                .any(|c| c.permission.scope == scope && c.grant.granted())
        })
    }

    /// The partial-listing note shows unless My Games is already complete:
    /// the key lists private experiences through the Inventory API, or
    /// every scope it holds is restricted to named universes.
    pub(super) fn note_visible(&self) -> bool {
        let Some(report) = self.report() else {
            return true;
        };
        let all_restricted = report
            .checks
            .iter()
            .filter(|c| c.grant.granted())
            .all(|c| matches!(c.grant, Grant::Universes(_)));
        !self.granted("user.inventory-item:read") && !all_restricted
    }

    pub(super) fn groups_off(&self) -> bool {
        self.report().is_some() && !self.granted("legacy-group:manage")
    }

    pub(super) fn has_key(&self) -> bool {
        !matches!(self.key, KeyState::Missing)
    }
}

pub(super) fn decode_icon(bytes: &[u8]) -> Option<Arc<RenderImage>> {
    let rgba = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (w, h) = rgba.dimensions();
    crate::render_image::to_render_image(rgba.into_raw(), w, h)
}

pub(super) fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

impl Render for HomeWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_view(window, cx)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_grid_follows_the_column_rule() {
        // 1440 wide: 1440 - 232 - 64 = 1144 of content, 6 columns.
        assert_eq!(super::view::columns(1144.), 6);
        assert_eq!(super::view::columns(500.), 3);
        assert_eq!(super::view::columns(3000.), 8);
    }

    #[test]
    fn a_path_under_home_reads_with_a_tilde() {
        let Ok(home) = std::env::var("HOME") else {
            return;
        };
        let path = std::path::PathBuf::from(format!("{home}/RbxNative/a.rbxl"));
        assert_eq!(super::view::display_path(&path), "~/RbxNative/a.rbxl");
    }
}
