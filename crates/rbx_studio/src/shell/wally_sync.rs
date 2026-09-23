//! Wires `crate::wally_client` into `Shell`: the dock's search box and
//! result list, and installing a picked package's whole resolved
//! dependency graph into the DOM as one undo step — reusing the exact
//! `push_history`/mutate/`take_changes`/`rebuild_explorer`/
//! `reflect_changes`/`record_history_change`/`cx.notify()` skeleton every
//! other mutation in this editor already uses (`shell::command::
//! run_command` is the closest twin). If a live Argon session happens to
//! be connected, the result reaches it automatically through the
//! write-back hook `shell::argon_sync` already built — nothing here is
//! Argon-aware.
//!
//! **Install layout.** Matches `wally install`'s own real on-disk shape,
//! confirmed against its actual source this session, not a flattened
//! shortcut: each resolved package's real content lands at `<Root>/
//! _Index/<scope>_<name>@<version>/<name>`, one alias `ModuleScript` per
//! dependency edge sits beside it in that same slot
//! (`<Root>/_Index/.../<Alias>.lua`, containing
//! `return require(script.Parent.Parent["<dep>"]["<dep_name>"])`), and
//! only the package the user actually picked gets a top-level alias,
//! `<Root>/<name>.lua`. Reproducing this exactly is why a package's own
//! `require(script.Parent.X)` calls keep working once installed — a
//! transitively-pulled dependency has no top-level alias of its own,
//! matching what a real `wally install` would produce for the same graph.

use std::time::Duration;

use crate::wally_client::{self, SearchResult};
use gpui_kit::Context;

use super::Shell;

mod install;

const SEARCH_DEBOUNCE: Duration = Duration::from_millis(400);

/// `RBX_STUDIO_WALLY_INSTALL=<scope>/<name>` installs that package on the
/// editor's behalf at startup — the same screenshot-aid reason every other
/// `RBX_STUDIO_*` var in `shell.rs` exists (see `main`'s module doc
/// comment): a result row is a dynamically-populated click target, nothing
/// else can drive the dock's own search-then-click flow deterministically.
pub(crate) const INSTALL_VARIABLE: &str = "RBX_STUDIO_WALLY_INSTALL";

pub(super) enum InstallState {
    Idle,
    Installing { name: String },
    Installed { name: String, count: usize },
    Error(String),
}

pub(super) struct Search {
    pub(super) results: Vec<SearchResult>,
    pub(super) install: InstallState,
    generation: u64,
}

impl Default for Search {
    fn default() -> Self {
        Search {
            results: Vec::new(),
            install: InstallState::Idle,
            generation: 0,
        }
    }
}

impl Shell {
    pub(super) fn wally_results(&self) -> &[SearchResult] {
        &self.wally.results
    }

    pub(super) fn wally_install_state(&self) -> &InstallState {
        &self.wally.install
    }

    /// `RBX_STUDIO_WALLY_INSTALL`: documented at [`INSTALL_VARIABLE`].
    pub(super) fn apply_debug_wally_install(&mut self, cx: &mut Context<Self>) {
        let Ok(value) = std::env::var(INSTALL_VARIABLE) else {
            return;
        };
        let Some((scope, name)) = value.split_once('/') else {
            return;
        };
        let result = SearchResult {
            scope: scope.to_owned(),
            name: name.to_owned(),
            description: None,
            versions: Vec::new(),
        };
        self.wally_install(result, cx);
    }

    /// The dock's search field: debounced (~400ms, the same shape `shell::
    /// scripts`'s commit debounce and `shell::argon_sync`'s write debounce
    /// already use), a background-thread `package-search` call, results
    /// swapped in only if nothing newer has been typed since.
    pub(super) fn wally_query_changed(&mut self, cx: &mut Context<Self>) {
        self.wally.generation = self.wally.generation.wrapping_add(1);
        let generation = self.wally.generation;
        let query = self.wally_query.read(cx).value().to_string();
        if query.trim().is_empty() {
            self.wally.results.clear();
            cx.notify();
            return;
        }
        cx.spawn(async move |shell, cx| {
            cx.background_executor().timer(SEARCH_DEBOUNCE).await;
            if shell
                .read_with(cx, |shell, _| shell.wally.generation)
                .unwrap_or(generation)
                != generation
            {
                return;
            }
            let results = cx
                .background_executor()
                .spawn(async move { wally_client::search(&query) })
                .await;
            let _ = shell.update(cx, |shell, cx| {
                if shell.wally.generation != generation {
                    return;
                }
                shell.wally.results = results.unwrap_or_default();
                cx.notify();
            });
        })
        .detach();
    }

    /// A result row's click: resolves the whole dependency graph off the
    /// main thread, then installs it in one pass.
    pub(super) fn wally_install(&mut self, result: SearchResult, cx: &mut Context<Self>) {
        self.wally.install = InstallState::Installing {
            name: result.name.clone(),
        };
        cx.notify();
        let scope = result.scope;
        let name = result.name;
        cx.spawn(async move |shell, cx| {
            let resolved = cx
                .background_executor()
                .spawn(async move {
                    let version = wally_client::latest_version(&scope, &name)?;
                    wally_client::resolve(&scope, &name, version)
                })
                .await;
            let _ = shell.update(cx, |shell, cx| match resolved {
                Ok(graph) => shell.apply_wally_graph(graph, cx),
                Err(message) => {
                    shell.wally.install = InstallState::Error(message);
                    cx.notify();
                }
            });
        })
        .detach();
    }
}
