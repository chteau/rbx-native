//! File › Data Stores…: browse and edit an experience's data stores from
//! the editor, through Open Cloud (`rbx_cloud::datastores`), without a
//! plugin or the Creator Hub page.
//!
//! Three sections: standard data stores (stores, scopes, keys, the entry
//! with its users, attributes and version history, rollback), ordered data
//! stores (entries sorted by value), and the memory store (sorted maps and
//! queues). Open Cloud lists neither ordered stores nor memory structures,
//! so those are opened by name.
//!
//! Every write is asked first, naming the key and the experience. The
//! experience is the open place's link, else the Play sandbox, else one
//! picked from the game picker; anything but the sandbox is badged live, and
//! a write there also needs "I know this is live player data" ticked.
//!
//! `RBX_STUDIO_ROBLOX=datastores` opens it with the editor;
//! `RBX_STUDIO_DATASTORE_MOCK` serves a fake universe from a local mock Open
//! Cloud (see `fetch`), and `RBX_STUDIO_DATASTORE_OPEN` lands it on a given
//! state for captures (see `debug`).

use gpui_kit::component::input::{InputState, TextareaState};
use gpui_kit::component::Root;
use gpui_kit::*;

use crate::home;
use crate::tokens;

use super::Shell;

mod actions;
mod actions_more;
mod debug;
mod fetch;
mod inputs;
mod json_tree;
mod state;
#[cfg(test)]
mod tests;
mod view;

use state::{
    Dialog, EntryPane, ListId, Listing, MapPane, MemoryKind, OrderedPane, QueuePane, Section,
    Status, Universe, UniverseSource,
};

const WIDTH: f32 = 1180.;
const HEIGHT: f32 = 760.;
const MIN_WIDTH: f32 = 900.;
const MIN_HEIGHT: f32 = 560.;

/// Which text field changed, for `DataStoresWindow::on_input`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Field {
    StoreSearch,
    Scope,
    KeySearch,
    NewKey,
    Value,
    Users,
    Attributes,
    Increment,
    OrderedStore,
    OrderedScope,
    OrderedMin,
    OrderedMax,
    OrderedKey,
    OrderedValue,
    MapName,
    MapAfter,
    MapKey,
    MapValue,
    MapTtl,
    MapSortKey,
    QueueName,
    QueueCount,
    QueueInvisibility,
    QueueData,
    QueuePriority,
    QueueTtl,
}

/// Every text field the window draws. Multi-line ones hold JSON.
pub(super) struct Inputs {
    pub(super) store_search: Entity<InputState>,
    pub(super) scope: Entity<InputState>,
    pub(super) key_search: Entity<InputState>,
    pub(super) new_key: Entity<InputState>,
    pub(super) value: Entity<TextareaState>,
    pub(super) users: Entity<InputState>,
    pub(super) attributes: Entity<TextareaState>,
    pub(super) increment: Entity<InputState>,
    pub(super) ordered_store: Entity<InputState>,
    pub(super) ordered_scope: Entity<InputState>,
    pub(super) ordered_min: Entity<InputState>,
    pub(super) ordered_max: Entity<InputState>,
    pub(super) ordered_key: Entity<InputState>,
    pub(super) ordered_value: Entity<InputState>,
    pub(super) map_name: Entity<InputState>,
    pub(super) map_after: Entity<InputState>,
    pub(super) map_key: Entity<InputState>,
    pub(super) map_value: Entity<TextareaState>,
    pub(super) map_ttl: Entity<InputState>,
    pub(super) map_sort_key: Entity<InputState>,
    pub(super) queue_name: Entity<InputState>,
    pub(super) queue_count: Entity<InputState>,
    pub(super) queue_invisibility: Entity<InputState>,
    pub(super) queue_data: Entity<TextareaState>,
    pub(super) queue_priority: Entity<InputState>,
    pub(super) queue_ttl: Entity<InputState>,
}

pub(crate) struct DataStoresWindow {
    shell: Entity<Shell>,
    /// `None` until an experience is known (no link, no sandbox, nothing
    /// picked yet).
    universe: Option<Universe>,
    section: Section,
    memory_kind: MemoryKind,
    stores: Listing<rbx_cloud::DataStoreInfo>,
    show_deleted: bool,
    /// The selected standard store's name.
    store: Option<String>,
    keys: Listing<rbx_cloud::EntryRef>,
    pane: EntryPane,
    ordered: OrderedPane,
    map: MapPane,
    queue: QueuePane,
    dialog: Option<Dialog>,
    status: Option<Status>,
    /// Writes in flight; the footer shows a spinner while any are.
    writing: usize,
    /// Requests of any kind in flight, for the rate-limit watch.
    in_flight: usize,
    /// `rbx_cloud::rate_limit_waits()` when the oldest request in flight
    /// started; a bigger count means Roblox is throttling us right now.
    waits_seen: u64,
    rate_limited: bool,
    inputs: Inputs,
    /// Text to put in fields at the next render, the one place a `Window`
    /// is at hand.
    reseed: Vec<(Field, String)>,
    /// The list the arrow keys move in.
    active_list: ListId,
    /// What `RBX_STUDIO_DATASTORE_OPEN` still has to do once loads land.
    debug: Option<debug::DebugOpen>,
    /// The game picker, while open.
    picker: Option<WindowHandle<Root>>,
    /// The shell's `roblox.changes` this window last caught up with.
    seen: u64,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
    /// Polls the rate-limit counter while requests are out.
    _poll: Option<Task<()>>,
}

impl Shell {
    /// Brings Data Stores forward, opening it if it isn't.
    pub(crate) fn open_data_stores(&mut self, cx: &mut Context<Self>) {
        if let Some(existing) = &self.data_stores {
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
            let opened = DataStoresWindow::open(shell.clone(), cx);
            shell.update(cx, |shell, _| shell.data_stores = opened);
        });
    }
}

impl DataStoresWindow {
    fn open(shell: Entity<Shell>, cx: &mut App) -> Option<WindowHandle<Root>> {
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
                title: Some("Data Stores".into()),
                appears_transparent: true,
                ..Default::default()
            }),
            window_decorations: Some(WindowDecorations::Client),
            window_background: crate::theme::active().effects.window,
            ..Default::default()
        };
        cx.open_window(options, move |window, cx| {
            let view = cx.new(|cx| DataStoresWindow::new(shell, window, cx));
            cx.new(|cx| Root::new(view, window, cx))
        })
        .ok()
    }

    fn new(shell: Entity<Shell>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut subscriptions = vec![cx.observe(&shell, |this, shell, cx| {
            let changes = shell.read(cx).roblox.changes;
            if changes != this.seen {
                this.seen = changes;
                // Only an experience that came from the link follows it.
                if this
                    .universe
                    .as_ref()
                    .is_none_or(|u| u.source == UniverseSource::Linked)
                {
                    let next = this.resolve_universe(cx);
                    if next.as_ref().map(|u| u.id) != this.universe.as_ref().map(|u| u.id) {
                        this.set_universe(next, cx);
                    }
                }
            }
        })];
        let inputs = Inputs::new(window, &mut subscriptions, cx);
        let focus = cx.focus_handle();
        focus.focus(window, cx);
        let seen = shell.read(cx).roblox.changes;
        let mut this = DataStoresWindow {
            shell,
            universe: None,
            section: Section::Standard,
            memory_kind: MemoryKind::SortedMap,
            stores: Listing::default(),
            show_deleted: false,
            store: None,
            keys: Listing::default(),
            pane: EntryPane::default(),
            ordered: OrderedPane::default(),
            map: MapPane::default(),
            queue: QueuePane::default(),
            dialog: None,
            status: None,
            writing: 0,
            in_flight: 0,
            waits_seen: 0,
            rate_limited: false,
            inputs,
            reseed: Vec::new(),
            active_list: ListId::Stores,
            debug: debug::DebugOpen::from_env(),
            picker: None,
            seen,
            focus,
            _subscriptions: subscriptions,
            _poll: None,
        };
        let universe = this.resolve_universe(cx);
        this.set_universe(universe, cx);
        this
    }

    /// The experience to open with: the mock's, the open place's link, or
    /// the Play sandbox.
    fn resolve_universe(&self, cx: &App) -> Option<Universe> {
        if let Some(mock) = fetch::mock_universe() {
            return Some(mock);
        }
        let shell = self.shell.read(cx);
        let sandbox = shell.play.sandbox.map(|s| s.universe_id);
        if let Some(link) = home::link_of(&shell.path) {
            return Some(Universe {
                id: link.universe_id,
                name: link.name,
                source: UniverseSource::Linked,
                live: sandbox != Some(link.universe_id),
            });
        }
        sandbox.map(|id| Universe {
            id,
            name: None,
            source: UniverseSource::Sandbox,
            live: false,
        })
    }

    /// Whether `universe_id` is the user's Play sandbox, for a picked one.
    fn is_sandbox(&self, universe_id: u64, cx: &App) -> bool {
        self.shell
            .read(cx)
            .play
            .sandbox
            .is_some_and(|s| s.universe_id == universe_id)
    }

    /// Switches experience: every list and pane starts over.
    fn set_universe(&mut self, universe: Option<Universe>, cx: &mut Context<Self>) {
        self.universe = universe;
        self.stores = Listing {
            generation: self.stores.generation + 1,
            ..Listing::default()
        };
        self.keys = Listing {
            generation: self.keys.generation + 1,
            ..Listing::default()
        };
        self.store = None;
        self.pane = EntryPane {
            generation: self.pane.generation + 1,
            ..EntryPane::default()
        };
        self.ordered.at = None;
        self.ordered.list = Listing::default();
        self.map.name = None;
        self.map.list = Listing::default();
        self.queue.name = None;
        self.queue.read = None;
        self.dialog = None;
        self.status = None;
        if self.universe.is_some() {
            self.load_stores(cx);
        }
        cx.notify();
    }
}

impl DataStoresWindow {
    /// Fills the fields `reseed` holds; called first thing in render.
    fn reseed_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for (field, text) in std::mem::take(&mut self.reseed) {
            self.inputs.set(field, text, window, cx);
        }
    }

    /// Queues `text` for `field` (see `reseed`).
    fn fill(&mut self, field: Field, text: impl Into<String>) {
        self.reseed.push((field, text.into()));
    }
}
