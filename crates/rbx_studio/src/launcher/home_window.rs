//! Home (boards `Home-*`): New / Recent / My Games, the sidebar with its
//! key card, and the open flow — LocalCopy, Downloading, DownloadError —
//! that ends in the editor. State and flows live here; the pages are drawn
//! in `view`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::*;
use rbx_cloud::{ApiKey, Client, CloudError, Experience, Experiences, Grant, KeyReport};

use super::{fixtures, Boot};
use crate::home::{self, OpenError, Opened, RecentPlace, Template};

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
    boot: Boot,
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
    pub(super) dialog: Option<Dialog>,
    /// Bumped per open, so Cancel drops a download still in flight.
    serial: u64,
    handle: AnyWindowHandle,
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
                    InputEvent::Blur if this.link.read(cx).value().is_empty() && this.link_open => {
                        this.link_open = false;
                        cx.notify();
                    }
                    _ => {}
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
                path: PathBuf::from(format!(
                    "~/.config/rbx-native/places/{}-{}.rbxl",
                    experience.universe_id, experience.root_place_id
                )),
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
    /// every scope it holds is restricted to named universes (#0077/#0079).
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

    // ------------------------------------------------------------ flows

    /// A card, a Recent linked entry, or a resolved link: the LocalCopy
    /// question when a copy exists, else straight to the download.
    pub(super) fn open_experience(&mut self, experience: Experience, cx: &mut Context<Self>) {
        match home::local_copy(&experience) {
            Some(path) => {
                self.dialog = Some(Dialog::LocalCopy {
                    experience,
                    path,
                    replace: false,
                })
            }
            None => self.download(experience, true, cx),
        }
        cx.notify();
    }

    pub(super) fn download(
        &mut self,
        experience: Experience,
        replace: bool,
        cx: &mut Context<Self>,
    ) {
        self.serial += 1;
        let serial = self.serial;
        self.dialog = Some(Dialog::Downloading {
            experience: experience.clone(),
        });
        cx.notify();
        let Some(key) = ApiKey::from_env_or_config() else {
            return;
        };
        let client = Client::new(Some(key));
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn({
                    let experience = experience.clone();
                    async move { home::open_experience(&client, &experience, replace) }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.serial != serial {
                    return;
                }
                match result {
                    Ok(Opened::Downloaded(path) | Opened::LocalCopy(path)) => {
                        this.dialog = None;
                        this.open_path_later(path, cx);
                    }
                    Err(OpenError { status, message }) => {
                        this.dialog = Some(Dialog::Error {
                            title: format!("Couldn\u{2019}t download {}", experience.name),
                            experience: Some(experience),
                            status,
                            reason: download_reason(status, &message),
                        })
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn cancel_dialog(&mut self, cx: &mut Context<Self>) {
        self.serial += 1;
        self.dialog = None;
        cx.notify();
    }

    /// Opens `path` in the editor and closes Home — deferred a frame, so the
    /// dialog closing paints first and the blocking load doesn't run inside
    /// a click handler.
    pub(super) fn open_path_later(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(16))
                .await;
            let _ = this.update(cx, |this, cx| this.open_path(&path, cx));
        })
        .detach();
    }

    fn open_path(&mut self, path: &Path, cx: &mut Context<Self>) {
        let Some(boot) = self.boot.borrow_mut().take() else {
            return;
        };
        match crate::open_editor(path, boot, cx) {
            Ok(()) => {
                let _ = self
                    .handle
                    .update(cx, |_, window, _| window.remove_window());
            }
            Err(failed) => {
                let (message, boot) = *failed;
                *self.boot.borrow_mut() = Some(boot);
                self.dialog = Some(Dialog::Error {
                    experience: None,
                    title: format!("Couldn\u{2019}t open {}", file_name(path)),
                    status: None,
                    reason: message,
                });
                cx.notify();
            }
        }
    }

    pub(super) fn open_recent(&mut self, place: &RecentPlace, cx: &mut Context<Self>) {
        self.open_path_later(place.path.clone(), cx);
    }

    pub(super) fn new_place(&mut self, cx: &mut Context<Self>) {
        let template = Template::Baseplate;
        let created = home::new_place_path(template)
            .ok_or_else(|| "no config directory to create the place in".to_string())
            .and_then(|path| template.create(&path).map(|()| path));
        match created {
            Ok(path) => self.open_path_later(path, cx),
            Err(reason) => {
                self.dialog = Some(Dialog::Error {
                    experience: None,
                    title: "Couldn\u{2019}t create the place".to_string(),
                    status: None,
                    reason,
                });
                cx.notify();
            }
        }
    }

    pub(super) fn open_file(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Open".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = picked.await {
                if let Some(path) = paths.into_iter().next() {
                    let _ = this.update(cx, |this, cx| this.open_path_later(path, cx));
                }
            }
        })
        .detach();
    }

    /// Add by place ID or URL: parse locally, resolve the universe, then
    /// the same open flow as a card.
    pub(super) fn add_link(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.link_state == LinkState::Resolving {
            return;
        }
        let text = self.link.read(cx).value().to_string();
        if text.trim().is_empty() {
            return;
        }
        let Some(place_id) = rbx_cloud::place_id_from_link(&text) else {
            self.link_state = LinkState::NotALink;
            cx.notify();
            return;
        };
        let Some(key) = ApiKey::from_env_or_config() else {
            self.link_state = LinkState::NoAccess;
            cx.notify();
            return;
        };
        self.link_state = LinkState::Resolving;
        cx.notify();
        let client = Client::new(Some(key));
        let _ = window;
        cx.spawn_in(window, async move |this, cx| {
            let found = cx
                .background_spawn(async move { client.experience_of_place(place_id) })
                .await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.link_state = match found {
                    Ok(Some(experience)) => {
                        this.link
                            .update(cx, |input, cx| input.set_value("", window, cx));
                        this.link_open = false;
                        this.open_experience(experience, cx);
                        LinkState::Idle
                    }
                    Ok(None) => LinkState::NoPlace(place_id),
                    Err(CloudError::Http {
                        status: 401 | 403 | 404,
                        ..
                    }) => LinkState::NoAccess,
                    Err(_) => LinkState::Unreachable,
                };
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn manage_key(&mut self, cx: &mut Context<Self>) {
        let this = cx.entity().downgrade();
        super::open_publishing(
            move |cx| {
                let _ = this.update(cx, |this, cx| this.reload(cx));
            },
            cx,
        );
    }

    /// Home without a key: back to the wizard, which comes back here.
    pub(super) fn set_up_key(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        super::open_wizard(self.boot.clone(), cx);
        window.remove_window();
    }
}

/// The DownloadError line after the status.
fn download_reason(status: Option<u16>, message: &str) -> String {
    match status {
        Some(401 | 403) => "legacy-asset:manage is not granted for this experience".to_string(),
        Some(404) => "The place doesn\u{2019}t exist any more".to_string(),
        _ => message.to_string(),
    }
}

fn decode_icon(bytes: &[u8]) -> Option<Arc<RenderImage>> {
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
    fn the_grid_follows_the_boards_column_rule() {
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
