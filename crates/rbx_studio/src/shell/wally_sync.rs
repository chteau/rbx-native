//! Wires `crate::wally_client` into `Shell`: the dock's pages (Home,
//! Installed, Updates), its search, what the registry sent back, the
//! realm and version picked on a result card, and installing a picked
//! package's whole resolved dependency graph into the DOM as one undo
//! step — reusing the exact `push_history`/mutate/`take_changes`/
//! `rebuild_explorer`/`reflect_changes`/`record_history_change`/
//! `cx.notify()` skeleton every other mutation in this editor already uses
//! (`shell::command::run_command` is the closest twin). If a live Argon
//! session happens to be connected, the result reaches it automatically
//! through the write-back hook `shell::argon_sync` already built — nothing
//! here is Argon-aware.
//!
//! **Install layout.** Matches `wally install`'s own real on-disk shape,
//! confirmed against its actual source, not a flattened shortcut: each
//! resolved package's real content lands at `<Root>/_Index/<scope>_<name>@
//! <version>/<name>`, one alias `ModuleScript` per dependency edge sits
//! beside it in that same slot (`<Root>/_Index/.../<Alias>.lua`,
//! containing `return require(script.Parent.Parent["<dep>"]["<dep_name>"])`),
//! and only the package the user actually picked gets a top-level alias,
//! `<Root>/<name>.lua`. Reproducing this exactly is why a package's own
//! `require(script.Parent.X)` calls keep working once installed — a
//! transitively-pulled dependency has no top-level alias of its own,
//! matching what a real `wally install` would produce for the same graph.
//! The same `_Index` slots are what the Installed page reads back
//! ([`installed_packages`]), so nothing about installs is stored twice.

use std::collections::HashMap;
use std::time::Duration;

use gpui_kit::Context;

use crate::wally_client::{Listing, Realm, SearchResult};

use super::Shell;

mod install;
mod installed;
mod remote;

pub(super) use installed::{installed_packages, updates, Installed, Update};

const SEARCH_DEBOUNCE: Duration = Duration::from_millis(400);

/// How many search results get a card, and a metadata fetch each.
pub(super) const RESULT_LIMIT: usize = 12;

/// `RBX_STUDIO_WALLY_INSTALL=<scope>/<name>` installs that package on the
/// editor's behalf at startup — the same screenshot-aid reason every other
/// `RBX_STUDIO_*` var in `shell.rs` exists (see `main`'s module doc
/// comment): a result row is a dynamically-populated click target, nothing
/// else can drive the dock's own search-then-click flow deterministically.
pub(crate) const INSTALL_VARIABLE: &str = "RBX_STUDIO_WALLY_INSTALL";

/// The Output panel's source column for everything the dock reports:
/// what an install did, and a registry it couldn't reach.
pub(super) const LOG_SOURCE: &str = "wally";

/// The rail's three pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Page {
    Home,
    Installed,
    Updates,
}

/// Something asked of the registry, at one of its moments. A failure's
/// reason goes to Output when it happens; the page only shows that it
/// failed.
pub(super) enum Remote<T> {
    Idle,
    Loading,
    Ready(T),
    Failed,
}

impl<T> Remote<T> {
    pub(super) fn ready(&self) -> Option<&T> {
        match self {
            Remote::Ready(value) => Some(value),
            _ => None,
        }
    }
}

/// `(scope, name)`.
pub(super) type PackageId = (String, String);

pub(super) fn package_id(scope: &str, name: &str) -> PackageId {
    (scope.to_owned(), name.to_owned())
}

/// What the user picked on one result card. `None` is the default: the
/// package's own realm, its newest version.
#[derive(Debug, Clone, Default)]
pub(super) struct Pick {
    pub(super) realm: Option<Realm>,
    pub(super) version: Option<semver::Version>,
}

pub(super) struct Search {
    pub(super) page: Page,
    /// The Featured cards, fetched once the dock first shows Home.
    pub(super) featured: Remote<Vec<Listing>>,
    /// The current search's results; `query` is the text they answer.
    pub(super) results: Remote<Vec<SearchResult>>,
    pub(super) query: String,
    pub(super) picks: HashMap<PackageId, Pick>,
    /// Per package: its realm, versions and description, for the result
    /// cards' defaults, the Installed cards and the Updates check.
    pub(super) metadata: HashMap<PackageId, Remote<Listing>>,
    generation: u64,
}

impl Default for Search {
    fn default() -> Self {
        Search {
            page: Page::Home,
            featured: Remote::Idle,
            results: Remote::Idle,
            query: String::new(),
            picks: HashMap::new(),
            metadata: HashMap::new(),
            generation: 0,
        }
    }
}

impl Shell {
    pub(super) fn wally_page(&self) -> Page {
        self.wally.page
    }

    pub(super) fn wally_set_page(&mut self, page: Page, cx: &mut Context<Self>) {
        self.wally.page = page;
        cx.notify();
    }

    /// The metadata the registry sent for one package, if it has.
    pub(super) fn wally_listing(&self, id: &PackageId) -> Option<&Listing> {
        self.wally.metadata.get(id).and_then(Remote::ready)
    }

    pub(super) fn wally_pick(&self, id: &PackageId) -> Pick {
        self.wally.picks.get(id).cloned().unwrap_or_default()
    }

    pub(super) fn wally_pick_realm(&mut self, id: PackageId, realm: Realm, cx: &mut Context<Self>) {
        self.wally.picks.entry(id).or_default().realm = Some(realm);
        cx.notify();
    }

    pub(super) fn wally_pick_version(
        &mut self,
        id: PackageId,
        version: Option<semver::Version>,
        cx: &mut Context<Self>,
    ) {
        self.wally.picks.entry(id).or_default().version = version;
        cx.notify();
    }

    /// The Installed count and the Updates pill need every installed
    /// package's metadata; ask for what isn't known yet.
    pub(super) fn wally_installed(&mut self, cx: &mut Context<Self>) -> Vec<Installed> {
        let installed = installed_packages(&self.dom);
        let ids: Vec<PackageId> = installed
            .iter()
            .map(|package| package_id(&package.scope, &package.name))
            .collect();
        self.wally_fetch_metadata(ids, cx);
        installed
    }

    /// `RBX_STUDIO_WALLY_INSTALL`: documented at [`INSTALL_VARIABLE`].
    pub(super) fn apply_debug_wally_install(&mut self, cx: &mut Context<Self>) {
        let Ok(value) = std::env::var(INSTALL_VARIABLE) else {
            return;
        };
        let Some((scope, name)) = value.split_once('/') else {
            return;
        };
        self.wally_install(scope.to_owned(), name.to_owned(), None, Realm::Shared, cx);
    }
}
