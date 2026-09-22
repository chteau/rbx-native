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

use gpui_kit::Context;
use rbx_dom::{Ref, WeakDom};

use crate::script_editor::source;
use crate::wally_client::{self, PackageNode, Realm, ResolvedGraph, SearchResult};

use super::Shell;

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

    fn apply_wally_graph(&mut self, graph: ResolvedGraph, cx: &mut Context<Self>) {
        if graph.packages.is_empty() {
            return;
        }
        self.push_history();
        let mut dom = std::mem::replace(&mut self.dom, WeakDom::new());

        // Pass 1: realize every resolved package's own content under its
        // `_Index` slot, keeping each slot's referent for pass 2's alias
        // wiring (a diamond dependency's slot has to be findable no matter
        // which package's edge is being wired).
        let mut slots: std::collections::HashMap<PackageKey, Ref> =
            std::collections::HashMap::new();
        for package in &graph.packages {
            let root = find_or_create_root(&mut dom, package.package.manifest.realm);
            let index_folder = find_or_create_child(&mut dom, root, "_Index");
            let slot = find_or_create_child(&mut dom, index_folder, &slot_name(package));
            realize_node(&mut dom, &package.package.tree, slot);
            slots.insert(package_key(package), slot);
        }

        // Pass 2: one alias `ModuleScript` per dependency edge, inside the
        // depending package's own slot.
        for package in &graph.packages {
            let Some(&slot) = slots.get(&package_key(package)) else {
                continue;
            };
            for (alias, target) in &package.dependency_edges {
                // The target's own slot must have been realized in pass 1
                // for this edge to mean anything — `resolve` only ever
                // produces edges pointing at packages it also resolved.
                if !slots.contains_key(target) {
                    continue;
                }
                let alias_ref = dom.new_instance("ModuleScript", alias, Some(slot));
                source::write(
                    &mut dom,
                    alias_ref,
                    &require_sibling(&slot_name_from_key(target), &target.1),
                );
            }
        }

        // The root pick alone gets a top-level alias — a transitively
        // pulled dependency is reachable only through the alias chain
        // above, exactly like a real `wally install`.
        let root_package = &graph.packages[0];
        let root = find_or_create_root(&mut dom, root_package.package.manifest.realm);
        let top_alias = dom.new_instance("ModuleScript", &root_package.name, Some(root));
        source::write(
            &mut dom,
            top_alias,
            &require_index(&slot_name(root_package), &root_package.name),
        );

        self.dom = dom;
        let changes = self.dom.take_changes();
        self.rebuild_explorer(cx);
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);
        self.wally.install = InstallState::Installed {
            name: root_package.name.clone(),
            count: graph.packages.len(),
        };
        cx.notify();
    }
}

type PackageKey = (String, String, semver::Version);

fn package_key(package: &wally_client::ResolvedPackage) -> PackageKey {
    (
        package.scope.clone(),
        package.name.clone(),
        package.version.clone(),
    )
}

fn slot_name(package: &wally_client::ResolvedPackage) -> String {
    format!("{}_{}@{}", package.scope, package.name, package.version)
}

fn slot_name_from_key(key: &PackageKey) -> String {
    format!("{}_{}@{}", key.0, key.1, key.2)
}

/// `return require(script.Parent.Parent["<slot>"]["<name>"])` — one
/// package's own alias file, reaching a sibling `_Index` slot.
fn require_sibling(slot: &str, name: &str) -> String {
    format!(
        "return require(script.Parent.Parent[{}][{}])",
        lua_string(slot),
        lua_string(name)
    )
}

/// `return require(script.Parent._Index["<slot>"]["<name>"])` — the
/// top-level alias a package's direct consumer actually requires.
fn require_index(slot: &str, name: &str) -> String {
    format!(
        "return require(script.Parent._Index[{}][{}])",
        lua_string(slot),
        lua_string(name)
    )
}

/// A Luau double-quoted string literal — package/scope names never carry
/// a quote or backslash in practice, but escaping them costs nothing and
/// means a hostile registry entry can't break out of the literal.
fn lua_string(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The root-level `Packages`/`ServerPackages`/`DevPackages` service for one
/// realm, found by class among this DOM's existing roots (never
/// duplicated) or created fresh — the same "match an existing root by
/// class, else make one" pattern `shell::argon_sync::apply_snapshot_node`
/// already uses for Argon's own root services.
fn find_or_create_root(dom: &mut WeakDom, realm: Realm) -> Ref {
    let class = match realm {
        Realm::Shared => "Packages",
        Realm::Server => "ServerPackages",
        Realm::Dev => "DevPackages",
    };
    dom.root_refs()
        .iter()
        .copied()
        .find(|&r| dom.get(r).is_some_and(|i| i.class() == class))
        .unwrap_or_else(|| dom.new_instance(class, class, None))
}

/// A named child of `parent`, found by exact name (not class — `_Index`
/// and a package's own slot are both plain `Folder`s distinguished only by
/// name) or created as a fresh `Folder`.
fn find_or_create_child(dom: &mut WeakDom, parent: Ref, name: &str) -> Ref {
    let existing = dom.get(parent).and_then(|instance| {
        instance
            .children()
            .iter()
            .copied()
            .find(|&child| dom.get(child).is_some_and(|c| c.name() == name))
    });
    existing.unwrap_or_else(|| dom.new_instance("Folder", name, Some(parent)))
}

/// One `PackageNode`, recursively: a `ModuleScript` where the node carries
/// a `Source` (a `.lua`/`.luau` file, or a directory whose own `init.lua`/
/// `init.luau` supplied it), a plain `Folder` otherwise.
fn realize_node(dom: &mut WeakDom, node: &PackageNode, parent: Ref) -> Ref {
    let class = if node.source.is_some() {
        "ModuleScript"
    } else {
        "Folder"
    };
    let referent = dom.new_instance(class, &node.name, Some(parent));
    if let Some(text) = &node.source {
        source::write(dom, referent, text);
    }
    for child in &node.children {
        realize_node(dom, child, referent);
    }
    referent
}

#[cfg(test)]
mod tests;
