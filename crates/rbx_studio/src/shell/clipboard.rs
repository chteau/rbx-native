//! Copy/Paste/Duplicate (`Ctrl+C`/`V`/`D`): a deep, in-process copy of the
//! selection, pushed through the same take/put-back path `shell::keys`'
//! insert and `shell::group`'s wrap already use for Paste and Duplicate —
//! one `push_history`/`take_changes` pair per call, however many instances
//! it copies, so one Paste or Duplicate is one `Ctrl+Z`. Copy itself never
//! touches `self.dom`, so it pushes nothing onto the undo stack.
//!
//! The clipboard (`Shell::clipboard`) is this window's own, not the system
//! one: an instance has no text form worth putting on the OS clipboard the
//! way `InputState`'s own Ctrl+C/V already do for a text field (`rbx_binary`/
//! `rbx_xml` are file formats, not a clipboard payload, and Roblox's own
//! Studio clipboard is a private format this project has no reason to
//! reverse-engineer). It survives a selection change — copy something,
//! click elsewhere, Paste still pastes what was copied — but not a window
//! restart, which real Studio's own clipboard doesn't promise either.
//!
//! Real Studio's plain `Ctrl+V` always lands in `Workspace`, not wherever
//! the selection happens to be (`studio/explorer.md`: "Pastes the clipboard
//! contents into the top‑level Workspace branch"). `Ctrl+Shift+V`, "Paste
//! Into", is the separate shortcut real Studio offers for pasting into the
//! selection instead: "Using this action on multiple selected objects is a
//! convenient way to paste the same clipboard items into multiple parents",
//! so each selected instance receives its own independent copy of the
//! whole clipboard. `Ctrl+D` duplicates each selected instance into its own
//! existing parent — "the same branch" real Studio duplicates into — rather
//! than `Workspace`.

use std::collections::{BTreeMap, HashMap};

use gpui_kit::{Context, Keystroke, Modifiers};
use rbx_dom::{Content, Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::explorer::{self, insert};

use super::Shell;

const WORKSPACE_NAME: &str = "Workspace";

/// What one keystroke does at the window level, or `None` if it means
/// something else — see [`action_for`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Action {
    Copy,
    Cut,
    Paste,
    PasteInto,
    Duplicate,
}

/// Checked at the window level (see `Shell::handle_shell_key`) rather than
/// only while the Explorer has focus, the same reasoning `shell::group`'s
/// own `action_for` gives for Ctrl+G: copying, pasting or duplicating the
/// selection is just as sensible from a viewport click as from the tree.
/// Safe to run no matter what else has focus — `InputState`'s own
/// Ctrl+C/X/V bindings are scoped to its own `"Input"` key context and
/// resolve first, so a text field being edited (a Properties row, the
/// Command Bar, an open script) keeps its own copy/paste and this handler
/// never sees the keystroke.
pub(super) fn action_for(key: &str, modifiers: Modifiers) -> Option<Action> {
    // The whole modifier set decides, not just Ctrl plus whichever other key
    // one arm happens to check: Ctrl+Alt+V (AltGr on some layouts) is no
    // paste, and Ctrl+Shift+D is not a duplicate. Shift is the one modifier
    // that means something, and only on V.
    if !modifiers.control || modifiers.alt || modifiers.platform || modifiers.function {
        return None;
    }
    match (key, modifiers.shift) {
        ("c", false) => Some(Action::Copy),
        ("x", false) => Some(Action::Cut),
        ("v", false) => Some(Action::Paste),
        ("v", true) => Some(Action::PasteInto),
        ("d", false) => Some(Action::Duplicate),
        _ => None,
    }
}

/// One instance as copied to the clipboard, together with every
/// descendant — enough to build a wholly independent copy without reading
/// the DOM it came from again. `origin` is the referent it held at copy
/// time: meaningless once that DOM moves on, kept only so [`materialize`]
/// can tell which pasted copy a `Variant::Ref`/`Content::Object` elsewhere
/// in this same entry was pointing at (see [`remap`]).
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Clipped {
    origin: Ref,
    class: String,
    name: String,
    properties: BTreeMap<String, Variant>,
    children: Vec<Clipped>,
}

/// Deep-copies `reference` and its whole subtree out of `dom`, or `None` if
/// it no longer resolves. `Variant`'s own clone is already a full, owned
/// copy — nothing in it is a shared reference — so the result shares no
/// state with `dom` at all.
///
/// `reference` itself is snapshotted regardless of its own `Archivable`
/// property — real Studio's Copy/Duplicate "ignore its own `Archivable`"
/// the same way, unlike `Instance:Clone()` — but a *descendant* that is not
/// `Archivable` is skipped along with everything under it, matching
/// `Archivable`'s own docs ("determines if an instance **and its
/// descendants** can be cloned"). Checked once per child, before recursing,
/// so a skipped subtree is never walked at all rather than walked and
/// discarded.
fn snapshot(dom: &WeakDom, reference: Ref) -> Option<Clipped> {
    let instance = dom.get(reference)?;
    Some(Clipped {
        origin: reference,
        class: instance.class().to_owned(),
        name: instance.name().to_owned(),
        properties: instance.properties().clone(),
        children: instance
            .children()
            .iter()
            .filter(|&&child| dom.get(child).is_some_and(|i| archivable(i.properties())))
            .filter_map(|&child| snapshot(dom, child))
            .collect(),
    })
}

/// `Archivable`'s documented default is `true`; only an explicit `false`
/// excludes an instance from a copy.
fn archivable(properties: &BTreeMap<String, Variant>) -> bool {
    !matches!(properties.get("Archivable"), Some(Variant::Bool(false)))
}

/// Every entry of `selected` this editor will copy, duplicate or let
/// through to the clipboard at all: still resolving, and not a service —
/// Roblox creates exactly one of each service and parenting a second one
/// anywhere would be meaningless, the same rule `shell::group`'s own
/// `common_parent`/`ungroupable` already enforce for Group/Ungroup. Anything
/// else in a mixed selection still goes through —
/// `shell::group::ungroup_selected` sets the precedent in this codebase for
/// skipping just the part of a selection that cannot follow along rather
/// than refusing the whole call.
fn copyable(dom: &WeakDom, database: &ReflectionDatabase, selected: &[Ref]) -> Vec<Ref> {
    selected
        .iter()
        .copied()
        .filter(|&reference| {
            dom.get(reference)
                .is_some_and(|instance| !database.is_service(instance.class()))
        })
        .collect()
}

/// Whether Copy and Duplicate would do anything with the current selection.
///
/// The same question both handlers ask before returning early, asked once
/// here so the ribbon can grey them instead of offering a click that no-ops.
/// A selection of nothing but services is not "something selected" for this
/// purpose — [`copyable`] drops them, so both handlers would return early
/// on one.
pub(super) fn has_copyable(dom: &WeakDom, database: &ReflectionDatabase, selected: &[Ref]) -> bool {
    !copyable(dom, database, selected).is_empty()
}

/// Materializes one clipboard entry under `parent`: every instance in it,
/// freshly created, with its own class, name and properties. Two passes —
/// every instance is created first, then every property is written — so a
/// `Variant::Ref`/`Content::Object` from an earlier sibling to a later one
/// still resolves to that later one's fresh copy (see [`remap`]) however
/// the tree is shaped, not just for a parent pointing at a child.
///
/// The root's own `Archivable` is then forced to `true`, regardless of what
/// `node` carried — the other half of the same real-Studio rule [`snapshot`]
/// applies on the way in: the original's `Archivable` never gates whether
/// the *root* of a copy happens, but the copy itself is always `Archivable`.
/// Nothing below the root needs the same treatment: every descendant that
/// made it into `node` at all already passed [`archivable`], so it was
/// already `true` (or absent, which reads the same way).
fn materialize(dom: &mut WeakDom, node: &Clipped, parent: Option<Ref>) -> Ref {
    let mut map = HashMap::new();
    let root = create(dom, node, parent, &mut map);
    write_properties(dom, node, &map);
    let _ = dom.set_property(root, "Archivable", Variant::Bool(true));
    root
}

fn create(
    dom: &mut WeakDom,
    node: &Clipped,
    parent: Option<Ref>,
    map: &mut HashMap<Ref, Ref>,
) -> Ref {
    let reference = dom.new_instance(&node.class, &node.name, parent);
    map.insert(node.origin, reference);
    for child in &node.children {
        create(dom, child, Some(reference), map);
    }
    reference
}

fn write_properties(dom: &mut WeakDom, node: &Clipped, map: &HashMap<Ref, Ref>) {
    let Some(&reference) = map.get(&node.origin) else {
        return;
    };
    for (name, value) in &node.properties {
        let _ = dom.set_property(reference, name, remap(value, map));
    }
    for child in &node.children {
        write_properties(dom, child, map);
    }
}

/// Rewrites a `Ref`-carrying property through `map` — built fresh per
/// [`materialize`] call from this one clipboard entry's own referents — so
/// a reference to something that was copied *along with* it now points at
/// the copy, exactly as `Class.Instance:Clone()`'s own docs describe: "If a
/// reference property refers to an instance that was also cloned, the copy
/// will refer to the copy... If [it] refers to an instance that was not
/// cloned, the same value is maintained." A referent absent from `map`
/// (something outside this entry, or the null referent) is left as-is.
fn remap(value: &Variant, map: &HashMap<Ref, Ref>) -> Variant {
    match value {
        Variant::Ref(r) => Variant::Ref(map.get(r).copied().unwrap_or(*r)),
        Variant::Content(Content::Object(r)) => {
            Variant::Content(Content::Object(map.get(r).copied().unwrap_or(*r)))
        }
        other => other.clone(),
    }
}

impl Shell {
    /// The window-level `on_key_down` handler's clipboard half; called from
    /// `Shell::handle_shell_key` alongside `shell::history`/`shell::group`'s
    /// own checks — see [`action_for`] for why this runs no matter what
    /// currently has focus.
    pub(super) fn handle_clipboard_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) {
        match action_for(&keystroke.key, keystroke.modifiers) {
            Some(Action::Copy) => self.copy_selected(cx),
            Some(Action::Cut) => self.cut_selected(cx),
            Some(Action::Paste) => self.paste_clipboard(cx),
            Some(Action::PasteInto) => self.paste_into_selected(cx),
            Some(Action::Duplicate) => self.duplicate_selected(cx),
            None => {}
        }
    }

    /// Ctrl+C: snapshots every copyable entry of the selection (see
    /// [`copyable`]) into `self.clipboard`, replacing whatever it held
    /// before. A no-op — the previous clipboard survives untouched — if
    /// nothing in the selection can be copied. Never touches `self.dom`, so
    /// there is nothing here for `Ctrl+Z` to undo. `pub(crate)`: also
    /// `menu_bar`'s Copy item's entry point, so a menu click runs the exact
    /// same path `Ctrl+C` does.
    pub(crate) fn copy_selected(&mut self, cx: &mut Context<Self>) {
        let selected = copyable(&self.dom, &self.database, self.selected_all());
        if selected.is_empty() {
            return;
        }
        self.clipboard = selected
            .into_iter()
            .filter_map(|reference| snapshot(&self.dom, reference))
            .collect();
        cx.notify();
    }

    /// Ctrl+X: Copy, then remove what was copied. Deliberately built out of
    /// the two existing halves rather than as a third DOM path — a cut whose
    /// clipboard and whose deletion could disagree about what
    /// `Archivable`/service filtering left out is exactly the bug this
    /// avoids: what lands on the clipboard is what leaves the tree.
    ///
    /// Two undo steps, not one: `copy_selected` pushes none at all, so the
    /// removal below is the only thing `Ctrl+Z` has to take back, and taking
    /// it back restores the instances *and* leaves them on the clipboard.
    /// `pub(crate)`: also `menu_bar`'s and the ribbon's Cut entry point.
    pub(crate) fn cut_selected(&mut self, cx: &mut Context<Self>) {
        let cut = copyable(&self.dom, &self.database, self.selected_all());
        if cut.is_empty() {
            return;
        }
        self.copy_selected(cx);
        self.remove_instances(&cut, cx);
    }

    /// Whether Paste/Paste Into would have anything to paste — the guard
    /// their own handlers return early on, asked by the ribbon and the
    /// Explorer's context menu so neither offers a row that does nothing.
    pub(super) fn clipboard_is_empty(&self) -> bool {
        self.clipboard.is_empty()
    }

    /// Ctrl+V: pastes every clipboard entry into `Workspace` — real
    /// Studio's own target for a plain paste, never the current selection
    /// (see this module's doc comment) — each an independent deep copy, and
    /// selects the pasted copies. A no-op with nothing on the clipboard.
    /// `pub(crate)`: also `menu_bar`'s Paste item's entry point.
    pub(crate) fn paste_clipboard(&mut self, cx: &mut Context<Self>) {
        if self.clipboard.is_empty() {
            return;
        }
        let parent = explorer::find_by_name(&self.dom, WORKSPACE_NAME);
        self.paste_under(&[parent], cx);
    }

    /// Ctrl+Shift+V: pastes the whole clipboard into *each* selected
    /// instance — a service included, since `Workspace` and its siblings are
    /// ordinary places to put a script or a folder — as an independent deep
    /// copy per parent, then selects everything that was pasted. A no-op
    /// with nothing on the clipboard or nothing selected. `pub(crate)`: also
    /// `menu_bar`'s Paste Into item's entry point.
    pub(crate) fn paste_into_selected(&mut self, cx: &mut Context<Self>) {
        let parents: Vec<Option<Ref>> = self
            .selected_all()
            .iter()
            .copied()
            .filter(|&reference| self.dom.get(reference).is_some())
            .map(Some)
            .collect();
        if self.clipboard.is_empty() || parents.is_empty() {
            return;
        }
        self.paste_under(&parents, cx);
    }

    /// Materializes the clipboard once under every entry of `parents`,
    /// selects the copies and records the whole batch as one undo step —
    /// the single path Paste and Paste Into share, so neither can drift from
    /// the other in how it reaches the history or the viewport.
    fn paste_under(&mut self, parents: &[Option<Ref>], cx: &mut Context<Self>) {
        let increment = self.increment_names();
        // See `shell::history`: snapshotted before the inserts below, so one
        // Paste is one undo step however many entries the clipboard holds.
        self.push_history();
        let mut dom = std::mem::replace(&mut self.dom, WeakDom::new());
        let mut pasted: Vec<Ref> = Vec::new();
        for &parent in parents {
            for node in &self.clipboard {
                // Resolved before the copy exists, so it cannot collide with
                // itself; with the preference off the copy simply keeps the
                // original's name, sibling or not.
                let name = increment.then(|| insert::incremented_name(&dom, parent, &node.name));
                let copy = materialize(&mut dom, node, parent);
                if let Some(name) = name {
                    let _ = dom.set_name(copy, &name);
                }
                pasted.push(copy);
            }
        }
        self.dom = dom;
        let changes = self.dom.take_changes();

        self.rebuild_explorer(cx);
        self.reselect(pasted, cx);
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);
        cx.notify();
    }

    /// Ctrl+D: duplicates every copyable entry of the selection (see
    /// [`copyable`]) into its own existing parent — "the same branch" real
    /// Studio duplicates into, unlike Paste's fixed `Workspace` target — and
    /// selects the duplicates. A no-op if nothing in the selection can be
    /// duplicated. `pub(crate)`: also `menu_bar`'s Duplicate item's entry
    /// point.
    pub(crate) fn duplicate_selected(&mut self, cx: &mut Context<Self>) {
        let selected = copyable(&self.dom, &self.database, self.selected_all());
        if selected.is_empty() {
            return;
        }

        let increment = self.increment_names();
        // See `shell::history`: snapshotted before the inserts below, so one
        // Duplicate is one undo step however many instances it duplicates.
        self.push_history();
        let mut dom = std::mem::replace(&mut self.dom, WeakDom::new());
        let duplicated: Vec<Ref> = selected
            .into_iter()
            .filter_map(|reference| {
                let parent = dom.parent(reference);
                let node = snapshot(&dom, reference)?;
                let name = increment.then(|| insert::incremented_name(&dom, parent, &node.name));
                let copy = materialize(&mut dom, &node, parent);
                if let Some(name) = name {
                    let _ = dom.set_name(copy, &name);
                }
                Some(copy)
            })
            .collect();
        self.dom = dom;
        let changes = self.dom.take_changes();

        self.rebuild_explorer(cx);
        self.reselect(duplicated, cx);
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);
        cx.notify();
    }
}

#[cfg(test)]
#[path = "clipboard/tests.rs"]
mod tests;
