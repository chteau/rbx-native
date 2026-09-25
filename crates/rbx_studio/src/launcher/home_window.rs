//! Home: New / Recent / My Games, the sidebar with its
//! key card, and the open flow — LocalCopy, Downloading, DownloadError —
//! that ends in the editor. State and flows live here; the pages are drawn
//! in `view`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::select::{SearchableVec, SelectEvent, SelectState};
use gpui_kit::component::IndexPath;
use gpui_kit::*;
use rbx_cloud::{Experience, Experiences, Grant, KeyReport};

use super::cache::GamesCache;
use super::{fixtures, Boot};
use crate::home::{self, RecentPlace};

mod games;
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
    /// The key's account, once checked: whose cache file is in use.
    pub(super) user_id: Option<u64>,
    pub(super) cache: GamesCache,
    /// A cached listing is showing while the fresh one loads.
    pub(super) refreshing: bool,
    /// My Games' owner filter: `None` is the key's own account, `Some` a
    /// group id.
    pub(super) owner: Option<u64>,
    /// The group whose games are being fetched.
    pub(super) group_loading: Option<u64>,
    pub(super) owner_select: Entity<SelectState<SearchableVec<SharedString>>>,
    /// The dropdown's rows, in order: `None` is "You".
    pub(super) owner_options: Vec<(Option<u64>, SharedString)>,
    /// The rows changed; handed to the dropdown on the next render, which
    /// is where a window to do it with is at hand.
    pub(super) owner_options_dirty: bool,
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
        let owner_select = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(vec![SharedString::from("You")]),
                Some(IndexPath::new(0)),
                window,
                cx,
            )
        });
        let subscriptions = vec![
            cx.subscribe(&owner_select, |this, _, event: &SelectEvent<_>, cx| {
                let SelectEvent::Confirm(Some(label)) = event else {
                    return;
                };
                if let Some(&(owner, _)) = this.owner_options.iter().find(|(_, l)| l == label) {
                    this.pick_owner(owner, cx);
                }
            }),
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
            user_id: None,
            cache: GamesCache::default(),
            refreshing: false,
            owner: None,
            group_loading: None,
            owner_select,
            owner_options: vec![(None, "You".into())],
            owner_options_dirty: true,
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

    /// The groups the owner dropdown lists after "You", by name.
    pub(super) fn set_groups(&mut self, mut groups: Vec<rbx_cloud::Group>) {
        groups.sort_by_key(|g| g.name.to_lowercase());
        let options: Vec<(Option<u64>, SharedString)> = std::iter::once((None, "You".into()))
            .chain(groups.into_iter().map(|g| (Some(g.id), g.name.into())))
            .collect();
        if options != self.owner_options {
            self.owner_options = options;
            self.owner_options_dirty = true;
        }
        if self
            .owner
            .is_some_and(|id| !self.owner_options.iter().any(|(o, _)| *o == Some(id)))
        {
            self.owner = None;
        }
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
        // Nothing to say until the key has been checked.
        let Some(report) = self.report() else {
            return false;
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
