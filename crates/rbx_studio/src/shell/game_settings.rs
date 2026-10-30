//! Home › Game Settings: the linked experience's settings and the place
//! file's own, on one set of pages shaped like Studio's Experience
//! Settings, each row saying where its value lives (see `catalog`). Edits
//! wait for Save: the cloud ones go to Roblox, the local ones land in the
//! place as one undo step. Team Create's switch and session list, and
//! Security's Allow Mesh / Image APIs, run as the editor's Roblox sign-in
//! instead (`team_create`).
//!
//! Like Version History, the window keeps no copy of the link: it reads it
//! from `links.json` when it opens and whenever `roblox.changes` moves.
//! `RBX_STUDIO_ROBLOX=game-settings` opens it with the editor; see `fetch` for
//! the capture mock.

use std::collections::BTreeMap;

use gpui_kit::component::input::InputState;
use gpui_kit::component::Root;
use gpui_kit::*;

use crate::home::{self, Link};
use crate::tokens;

use super::Shell;

mod catalog;
mod fetch;
mod inputs;
mod model;
mod save;
mod team_create;
mod view;

use catalog::{CloudField, Tab};
use model::{CloudDraft, CloudValue, LocalDraft};
use team_create::TeamCreate;

const WIDTH: f32 = 860.;
const HEIGHT: f32 = 640.;
const MIN_WIDTH: f32 = 640.;
const MIN_HEIGHT: f32 = 440.;

#[derive(Debug, Clone, PartialEq, Eq)]
enum Loading {
    /// No link, or loaded.
    Idle,
    Busy,
    Failed(String),
}

/// The line over the footer saying how the last save went.
struct Notice {
    ok: bool,
    text: String,
}

/// A text box's field: the cloud text and number fields, and the local
/// `Float` fields by their `LOCAL_FIELDS` index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum InputKey {
    Cloud(CloudField),
    Local(usize),
}

pub(crate) struct GameSettingsWindow {
    shell: Entity<Shell>,
    link: Option<Link>,
    tab: Tab,
    cloud: CloudDraft,
    local: LocalDraft,
    loading: Loading,
    saving: bool,
    notice: Option<Notice>,
    /// Bumped by every load and save, so a reply that lost the race is dropped.
    generation: u64,
    team_create: TeamCreate,
    inputs: BTreeMap<InputKey, Entity<InputState>>,
    /// Inputs whose text doesn't parse, with why; Save waits for none.
    invalid: BTreeMap<InputKey, String>,
    /// The inputs need refilling from the drafts at the next render, the
    /// one place a `Window` is at hand.
    reseed: bool,
    /// The shell's `roblox.changes` this window last caught up with.
    seen: u64,
    focus: FocusHandle,
    scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl Shell {
    /// Brings Game Settings forward, opening it if it isn't.
    pub(crate) fn open_game_settings(&mut self, cx: &mut Context<Self>) {
        self.show_game_settings(Tab::BasicInfo, cx);
    }

    /// Game Settings on the experience's page or the place's.
    pub(crate) fn configure_game(&mut self, page: crate::cli::Configure, cx: &mut Context<Self>) {
        let tab = match page {
            crate::cli::Configure::Experience => Tab::BasicInfo,
            crate::cli::Configure::Place => Tab::Places,
        };
        self.show_game_settings(tab, cx);
    }

    /// `tab` is where a newly opened window starts; one already open keeps its own.
    fn show_game_settings(&mut self, tab: Tab, cx: &mut Context<Self>) {
        if let Some(existing) = &self.game_settings {
            if existing
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
            {
                return;
            }
        }
        // Deferred: the window's first render reads this `Shell`, which is
        // still being updated here.
        let shell = cx.entity();
        cx.defer(move |cx| {
            let opened = GameSettingsWindow::open(shell.clone(), tab, cx);
            shell.update(cx, |shell, _| shell.game_settings = opened);
        });
    }
}

impl GameSettingsWindow {
    fn open(shell: Entity<Shell>, tab: Tab, cx: &mut App) -> Option<WindowHandle<Root>> {
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(tokens::scaled_width(WIDTH), tokens::scaled_width(HEIGHT)),
                cx,
            ))),
            is_resizable: true,
            window_min_size: Some(size(px(MIN_WIDTH), px(MIN_HEIGHT))),
            app_owns_titlebar_drag: true,
            titlebar: Some(TitlebarOptions {
                title: Some("Game Settings".into()),
                appears_transparent: true,
                ..Default::default()
            }),
            window_decorations: Some(WindowDecorations::Client),
            window_background: crate::theme::active().effects.window,
            ..Default::default()
        };
        cx.open_window(options, move |window, cx| {
            let view = cx.new(|cx| GameSettingsWindow::new(shell, tab, window, cx));
            cx.new(|cx| Root::new(view, window, cx))
        })
        .ok()
    }

    fn new(shell: Entity<Shell>, tab: Tab, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut subscriptions = vec![cx.observe(&shell, |this, shell, cx| {
            let (changes, path, fresh, session) = {
                let s = shell.read(cx);
                (
                    s.roblox.changes,
                    s.path.clone(),
                    LocalDraft::read(&s.dom, &s.database),
                    s.studio_presence.session.clone(),
                )
            };
            // A publish bumps the counter without moving the link; only a
            // different link is worth dropping the edits for.
            if changes != this.seen {
                this.seen = changes;
                if home::link_of(&path) != this.link {
                    this.relink(cx);
                    return;
                }
            }
            this.follow_sign_in(session, cx);
            // Undo, redo and the Properties panel change the place under us.
            if this.local.rebase(fresh) {
                this.reseed = true;
                cx.notify();
            }
        })];
        let inputs = Self::make_inputs(window, &mut subscriptions, cx);
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let (path, seen, local, session) = {
            let s = shell.read(cx);
            (
                s.path.clone(),
                s.roblox.changes,
                LocalDraft::read(&s.dom, &s.database),
                s.studio_presence.session.clone(),
            )
        };
        Self::poll_members(cx);
        let mut this = GameSettingsWindow {
            link: home::link_of(&path),
            shell,
            tab,
            cloud: CloudDraft::default(),
            local,
            loading: Loading::Idle,
            saving: false,
            notice: None,
            generation: 0,
            team_create: TeamCreate::new(session),
            inputs,
            invalid: BTreeMap::new(),
            reseed: true,
            seen,
            focus,
            scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
        };
        this.load(cx);
        this
    }

    /// Re-reads the link and the place file, dropping every edit, and loads.
    fn relink(&mut self, cx: &mut Context<Self>) {
        let (path, local) = {
            let s = self.shell.read(cx);
            (s.path.clone(), LocalDraft::read(&s.dom, &s.database))
        };
        self.link = home::link_of(&path);
        self.cloud = CloudDraft::default();
        self.local = local;
        self.invalid.clear();
        self.notice = None;
        self.saving = false;
        self.reseed = true;
        self.load(cx);
    }

    /// Fetches the remote settings for the current link (nothing unlinked).
    fn load(&mut self, cx: &mut Context<Self>) {
        self.load_team_create(cx);
        self.generation += 1;
        let generation = self.generation;
        let Some(link) = self.link.clone() else {
            self.loading = Loading::Idle;
            cx.notify();
            return;
        };
        self.loading = Loading::Busy;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let reply = cx.background_spawn(async move { fetch::load(&link) }).await;
            let _ = this.update(cx, |this, cx| {
                if generation != this.generation {
                    return;
                }
                match reply {
                    Ok(remote) => {
                        this.cloud.reset(remote);
                        this.loading = Loading::Idle;
                        this.reseed = true;
                    }
                    Err(message) => this.loading = Loading::Failed(message),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn link_place(&mut self, cx: &mut Context<Self>) {
        self.shell
            .update(cx, |shell, cx| shell.open_roblox_link(None, cx));
    }

    fn set_cloud(&mut self, field: CloudField, value: CloudValue, cx: &mut Context<Self>) {
        self.cloud.set(field, value);
        self.notice = None;
        cx.notify();
    }

    fn set_local(&mut self, index: usize, value: rbx_dom::Variant, cx: &mut Context<Self>) {
        self.local.set(index, value);
        self.notice = None;
        cx.notify();
    }

    fn is_dirty(&self) -> bool {
        self.cloud.is_dirty() || self.local.is_dirty() || self.team_create.is_dirty()
    }
}
