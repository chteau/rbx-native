//! The window's state, apart from the window itself: what each list holds
//! and how far it got, the entry being looked at, and the writes waiting
//! for a confirmation. Pure data, so the rules live where tests reach them.

use std::collections::BTreeSet;

use rbx_cloud::{
    Entry, EntryWrite, MapItem, MapItemWrite, OrderedEntry, OrderedQuery, QueueRead, Revision,
    StoreRef,
};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Section {
    Standard,
    Ordered,
    Memory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MemoryKind {
    SortedMap,
    Queue,
}

/// Why something could not be read or written, sorted by what the user
/// can do about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Problem {
    /// No Open Cloud key is stored at all.
    NoKey,
    /// The key lacks this permission (`name:operation`), or isn't granted
    /// this experience.
    MissingScope {
        scope: String,
    },
    /// Roblox kept answering 429 after the retries.
    RateLimited,
    /// The entry changed since it was read (HTTP 409 on an etag).
    Conflict(String),
    NotFound(String),
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) enum Load {
    #[default]
    Idle,
    Busy,
    Failed(Problem),
}

/// One paged list: what has arrived, the cursor for the next page, and the
/// keyboard selection.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Listing<T> {
    pub(super) items: Vec<T>,
    pub(super) next: Option<String>,
    pub(super) load: Load,
    pub(super) selected: Option<usize>,
    /// Bumped by every reload, so a page that lost the race is dropped.
    pub(super) generation: u64,
}

impl<T> Default for Listing<T> {
    fn default() -> Self {
        Listing {
            items: Vec::new(),
            next: None,
            load: Load::Idle,
            selected: None,
            generation: 0,
        }
    }
}

/// Where the open experience came from, which decides the badge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum UniverseSource {
    /// The open place's own link.
    Linked,
    /// Settings › Files & Account › Sandbox place.
    Sandbox,
    /// Chosen from the game picker.
    Picked,
    /// `RBX_STUDIO_DATASTORE_MOCK`'s fake universe.
    Mock,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Universe {
    pub(super) id: u64,
    pub(super) name: Option<String>,
    pub(super) source: UniverseSource,
    /// Anything but the user's own sandbox: real players' data.
    pub(super) live: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum ValueView {
    #[default]
    Tree,
    Raw,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum EntryTab {
    #[default]
    Value,
    Versions,
}

/// The entry pane of the standard section.
#[derive(Debug, Clone, PartialEq, Default)]
pub(super) struct EntryPane {
    pub(super) entry: Option<Entry>,
    pub(super) load: Load,
    pub(super) generation: u64,
    pub(super) tab: EntryTab,
    pub(super) view: ValueView,
    /// Tree paths (see `json_tree`) the user folded.
    pub(super) collapsed: BTreeSet<String>,
    /// An older revision shown read-only in place of the current value.
    pub(super) revision: Option<Entry>,
    pub(super) revisions: Listing<Revision>,
    /// Some while the value/users/attributes editors are open.
    pub(super) editing: Option<EntryDraft>,
}

/// What the editor is writing; the text itself lives in the inputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct EntryDraft {
    /// A new entry (create, its key typed in `Inputs::new_key`) rather than
    /// an edit of `EntryPane::entry`.
    pub(super) creating: bool,
    /// Why the draft can't be sent yet, shown under the editor.
    pub(super) error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(super) struct OrderedPane {
    /// The store and scope opened; Open Cloud cannot list ordered stores,
    /// so they are typed.
    pub(super) at: Option<StoreRef>,
    pub(super) query: OrderedQuery,
    pub(super) list: Listing<OrderedEntry>,
    /// Ordered store names opened this session, newest first.
    pub(super) recent: Vec<String>,
    pub(super) error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(super) struct MapPane {
    pub(super) name: Option<String>,
    pub(super) descending: bool,
    pub(super) list: Listing<MapItem>,
    /// Some while the item editor is open: `true` for a new item.
    pub(super) creating: Option<bool>,
    pub(super) collapsed: BTreeSet<String>,
    pub(super) recent: Vec<String>,
    pub(super) error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub(super) struct QueuePane {
    pub(super) name: Option<String>,
    /// The last read, whose items stay invisible to game servers until the
    /// window passes or they are discarded.
    pub(super) read: Option<QueueRead>,
    pub(super) load: Load,
    pub(super) generation: u64,
    pub(super) adding: bool,
    pub(super) recent: Vec<String>,
    pub(super) error: Option<String>,
}

/// A write waiting for the confirm dialog (or for a retry after a conflict).
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Pending {
    SaveEntry {
        at: StoreRef,
        key: String,
        write: EntryWrite,
        creating: bool,
    },
    DeleteEntry {
        at: StoreRef,
        key: String,
    },
    IncrementEntry {
        at: StoreRef,
        key: String,
        amount: i64,
    },
    Rollback {
        at: StoreRef,
        key: String,
        revision: Revision,
        current_etag: String,
    },
    SaveOrdered {
        at: StoreRef,
        key: String,
        value: i64,
        creating: bool,
    },
    IncrementOrdered {
        at: StoreRef,
        key: String,
        amount: i64,
    },
    DeleteOrdered {
        at: StoreRef,
        key: String,
    },
    SaveMapItem {
        map: String,
        key: String,
        write: MapItemWrite,
        creating: bool,
    },
    DeleteMapItem {
        map: String,
        key: String,
    },
    AddQueueItem {
        queue: String,
        data: Value,
        priority: f64,
        ttl_seconds: u64,
    },
    /// Reading hides the items from game servers for the window, so on a
    /// live experience it is asked like a write.
    ReadQueue {
        queue: String,
        count: u32,
        invisibility_seconds: u32,
    },
    DiscardQueue {
        queue: String,
        read_id: String,
        count: usize,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub(super) enum Dialog {
    /// Every write is asked first; a live experience also needs the box
    /// ticked.
    Confirm { pending: Pending, live_ack: bool },
    /// The entry changed under an edit: shows Roblox's newer copy and
    /// offers to write ours over it (with the newer etag) or keep theirs.
    Conflict { pending: Pending, newer: Box<Entry> },
}

/// The line at the bottom of the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Status {
    pub(super) ok: bool,
    pub(super) text: String,
}

/// Which value tree a fold/unfold click belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TreeTarget {
    Entry,
    MapItem,
}

/// Every list the window shows, for keyboard navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ListId {
    Stores,
    Keys,
    Revisions,
    Ordered,
    MapItems,
    QueueItems,
}

/// What an `EntryRef` row and a selected store mean together.
pub(super) fn store_ref(universe_id: u64, store: &str, scope: &str) -> StoreRef {
    StoreRef {
        universe_id,
        store: store.to_string(),
        scope: scope.to_string(),
    }
}
