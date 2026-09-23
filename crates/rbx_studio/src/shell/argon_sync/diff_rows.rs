//! The review prompt's data: the pending batch read back out as a tree
//! the Diff window can draw. Every "before" comes from the DOM as it is
//! now — nothing in a pending review has been applied yet, so the DOM
//! *is* the before state — and the window rebuilds this per batch, never
//! keeping a copy the batch could drift from.

use std::collections::BTreeMap;

use gpui_kit::Context;
use rbx_dom::{Ref, Variant, WeakDom};

use crate::argon_client::{self, ArgonRef, Snapshot};
use crate::folder_colors::path_of;

use super::{Shell, DIFF_VARIABLE};

/// What one node of the batch is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum ChangeKind {
    Added,
    Updated,
    Removed,
}

/// One property of a change: what the DOM holds now and what the server
/// sends. `None` on either side is "no such value": an addition has no
/// before, a value this client can't decode has no after.
#[derive(Debug, Clone)]
pub(crate) struct PropertyChange {
    pub(crate) name: String,
    pub(crate) before: Option<Variant>,
    pub(crate) after: Option<Variant>,
}

/// A script's source on each side; a `None` side is a script that didn't
/// exist (an addition) or won't (a removal).
#[derive(Debug, Clone)]
pub(crate) struct SourceChange {
    pub(crate) old: Option<String>,
    pub(crate) new: Option<String>,
}

/// One change, with an addition's subtree under it.
#[derive(Debug, Clone)]
pub(crate) struct DiffNode {
    /// Stable within one batch: nodes are numbered in list order.
    pub(crate) id: usize,
    pub(crate) kind: ChangeKind,
    pub(crate) name: String,
    pub(crate) class: String,
    /// The parent's full name, `game` for a service.
    pub(crate) path: String,
    /// `Name` first when the change renames, then the rest by name.
    /// `Source` never appears here; it is [`DiffNode::source`].
    pub(crate) properties: Vec<PropertyChange>,
    pub(crate) source: Option<SourceChange>,
    /// An addition's own subtree, in the server's order.
    pub(crate) children: Vec<DiffNode>,
    /// Every descendant, however deep.
    pub(crate) nested: usize,
    /// Descendants by class, most first, then by name.
    pub(crate) contents: Vec<(String, usize)>,
}

impl DiffNode {
    pub(crate) fn full_path(&self) -> String {
        if self.path == "game" {
            self.name.clone()
        } else {
            format!("{}.{}", self.path, self.name)
        }
    }
}

/// A `LuaSourceContainer`: the classes whose `Source` is shown as code
/// rather than as a property.
fn is_script_class(class: &str) -> bool {
    matches!(class, "Script" | "LocalScript" | "ModuleScript")
}

/// Numbers nodes in list order as the tree is built.
struct Ids(usize);

impl Ids {
    fn next(&mut self) -> usize {
        let id = self.0;
        self.0 += 1;
        id
    }
}

impl Shell {
    /// The pending batch as a tree, or nothing once no review is pending
    /// — including right after Accept or Cancel, which is what tells the
    /// Diff window to close itself.
    pub(in crate::shell) fn argon_diff_nodes(&self) -> Vec<DiffNode> {
        let Some(pending) = &self.argon.pending else {
            return Vec::new();
        };
        let changes = pending.changes();
        let mut ids = Ids(0);
        let mut nodes = Vec::new();
        for addition in &changes.additions {
            let path = addition
                .parent
                .and_then(|parent| self.argon.ids.get(&parent))
                .and_then(|&parent| path_of(&self.dom, parent))
                .unwrap_or_else(|| "game".to_owned());
            nodes.push(added_node(addition, path, &mut ids));
        }
        for update in &changes.updates {
            let existing = self.argon.ids.get(&update.id).copied();
            nodes.push(self.updated_node(update, existing, &mut ids));
        }
        for id in &changes.removals {
            let existing = self.argon.ids.get(id).copied();
            nodes.push(self.removed_node(existing, &mut ids));
        }
        nodes
    }

    fn updated_node(
        &self,
        update: &argon_client::UpdatedSnapshot,
        existing: Option<Ref>,
        ids: &mut Ids,
    ) -> DiffNode {
        let instance = existing.and_then(|r| self.dom.get(r));
        let name = instance.map(|i| i.name().to_owned()).unwrap_or_default();
        let class = update
            .class
            .clone()
            .or_else(|| instance.map(|i| i.class().to_owned()))
            .unwrap_or_default();
        let path = existing
            .and_then(|r| self.dom.parent(r))
            .and_then(|parent| path_of(&self.dom, parent))
            .unwrap_or_else(|| "game".to_owned());
        let mut properties = Vec::new();
        let mut source = None;
        if let Some(new_name) = &update.name {
            properties.push(PropertyChange {
                name: "Name".to_owned(),
                before: Some(Variant::String(name.clone())),
                after: Some(Variant::String(new_name.clone())),
            });
        }
        let mut rest = Vec::new();
        for (property, encoded) in update.properties.iter().flatten() {
            let after = argon_client::decode_value(encoded);
            let before = instance.and_then(|i| i.properties().get(property).cloned());
            if property == "Source" && is_script_class(&class) {
                source = Some(SourceChange {
                    old: string_of(before.as_ref()),
                    new: string_of(after.as_ref()),
                });
                continue;
            }
            rest.push(PropertyChange {
                name: property.clone(),
                before,
                after,
            });
        }
        rest.sort_by(|a, b| a.name.cmp(&b.name));
        properties.extend(rest);
        DiffNode {
            id: ids.next(),
            kind: ChangeKind::Updated,
            name,
            class,
            path,
            properties,
            source,
            children: Vec::new(),
            nested: 0,
            contents: Vec::new(),
        }
    }

    fn removed_node(&self, existing: Option<Ref>, ids: &mut Ids) -> DiffNode {
        let instance = existing.and_then(|r| self.dom.get(r));
        let (name, class) = instance
            .map(|i| (i.name().to_owned(), i.class().to_owned()))
            .unwrap_or_default();
        let path = existing
            .and_then(|r| self.dom.parent(r))
            .and_then(|parent| path_of(&self.dom, parent))
            .unwrap_or_else(|| "game".to_owned());
        let source = (is_script_class(&class)).then(|| SourceChange {
            old: existing.and_then(|r| crate::script_editor::source::read(&self.dom, r)),
            new: None,
        });
        let mut counts = BTreeMap::new();
        let nested = existing.map_or(0, |r| count_dom_descendants(&self.dom, r, &mut counts));
        DiffNode {
            id: ids.next(),
            kind: ChangeKind::Removed,
            name,
            class,
            path,
            properties: Vec::new(),
            source,
            children: Vec::new(),
            nested,
            contents: sorted_contents(counts),
        }
    }

    /// `RBX_STUDIO_ARGON_DIFF=1` opens the Diff window on the batch
    /// [`Shell::seed_debug_diff_pending`] builds — a real one only exists
    /// after a live `argon serve` session pushes enough changes at once,
    /// which nothing else can arrange on the editor's behalf, the same
    /// reason every other `RBX_STUDIO_*` var exists.
    pub(in crate::shell) fn apply_debug_argon_diff(&mut self, cx: &mut Context<Self>) {
        if std::env::var(DIFF_VARIABLE).as_deref() != Ok("1") {
            return;
        }
        self.seed_debug_diff_pending();
        self.open_argon_diff(cx);
    }

    /// The review prompt's Diff button: opens the detail window
    /// (`shell::argon_diff_window`), or raises it if one is already open —
    /// there is only ever one review pending at a time, so a second window
    /// would just be the same rows twice.
    pub(in crate::shell) fn open_argon_diff(&mut self, cx: &mut Context<Self>) {
        if let Some(existing) = &self.argon_diff {
            let _ = existing.update(cx, |_, window, _| window.activate_window());
            return;
        }
        let shell = cx.entity();
        // Deferred for the same reason `open_sequence_editor` defers: this
        // runs inside the click handler's own `Shell` update, and opening a
        // window renders it immediately — reading the entity that update is
        // still holding is a panic, not something the compiler catches.
        cx.defer(move |cx| {
            let opened = crate::shell::argon_diff_window::ArgonDiffWindow::open(shell.clone(), cx);
            shell.update(cx, |shell, _| shell.argon_diff = opened);
        });
    }
}

/// An addition and its subtree, numbered depth-first in the server's
/// order — the order the list shows.
fn added_node(snapshot: &Snapshot, path: String, ids: &mut Ids) -> DiffNode {
    let id = ids.next();
    let is_script = is_script_class(&snapshot.class);
    let mut properties = Vec::new();
    let mut source = None;
    for (property, encoded) in &snapshot.properties {
        let after = argon_client::decode_value(encoded);
        if property == "Source" && is_script {
            source = Some(SourceChange {
                old: None,
                new: string_of(after.as_ref()),
            });
            continue;
        }
        properties.push(PropertyChange {
            name: property.clone(),
            before: None,
            after,
        });
    }
    properties.sort_by(|a, b| a.name.cmp(&b.name));
    let full_path = if path == "game" {
        snapshot.name.clone()
    } else {
        format!("{path}.{}", snapshot.name)
    };
    let children: Vec<DiffNode> = snapshot
        .children
        .iter()
        .map(|child| added_node(child, full_path.clone(), ids))
        .collect();
    let mut counts = BTreeMap::new();
    count_snapshot_descendants(&snapshot.children, &mut counts);
    DiffNode {
        id,
        kind: ChangeKind::Added,
        name: snapshot.name.clone(),
        class: snapshot.class.clone(),
        path,
        properties,
        source,
        children,
        nested: count_descendants(&snapshot.children),
        contents: sorted_contents(counts),
    }
}

fn string_of(value: Option<&Variant>) -> Option<String> {
    match value {
        Some(Variant::String(text)) => Some(text.clone()),
        _ => None,
    }
}

/// One addition's own subtree, minus its own root — a row's "+N nested".
pub(super) fn count_descendants(children: &[Snapshot]) -> usize {
    children
        .iter()
        .map(|child| 1 + count_descendants(&child.children))
        .sum()
}

fn count_snapshot_descendants(children: &[Snapshot], counts: &mut BTreeMap<String, usize>) {
    for child in children {
        *counts.entry(child.class.clone()).or_default() += 1;
        count_snapshot_descendants(&child.children, counts);
    }
}

fn count_dom_descendants(dom: &WeakDom, root: Ref, counts: &mut BTreeMap<String, usize>) -> usize {
    let mut total = 0;
    let mut stack: Vec<Ref> = dom
        .get(root)
        .map(|i| i.children().to_vec())
        .unwrap_or_default();
    while let Some(referent) = stack.pop() {
        let Some(instance) = dom.get(referent) else {
            continue;
        };
        total += 1;
        *counts.entry(instance.class().to_owned()).or_default() += 1;
        stack.extend(instance.children().iter().copied());
    }
    total
}

/// Most numerous first, then by name.
fn sorted_contents(counts: BTreeMap<String, usize>) -> Vec<(String, usize)> {
    let mut contents: Vec<(String, usize)> = counts.into_iter().collect();
    contents.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    contents
}

/// The ArgonRef a fixture or a test gives a DOM instance.
pub(super) fn bind(shell: &mut Shell, referent: Ref) -> ArgonRef {
    if let Some(&id) = shell.argon.ids_rev.get(&referent) {
        return id;
    }
    let id = ArgonRef::generate();
    shell.argon.ids.insert(id, referent);
    shell.argon.ids_rev.insert(referent, id);
    id
}
