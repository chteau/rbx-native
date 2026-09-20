//! Group/ungroup: wrap the selection in one new `Model`, or unwind a `Model`
//! back into its own parent. Both push through the same take/put-back path
//! `shell::keys`' insert and delete, and `shell::reparent`'s drag, already
//! use — one `push_history`/`take_changes` pair per call, however many
//! instances move, so one Group or Ungroup is one `Ctrl+Z`.

use gpui_kit::{Context, Keystroke, Modifiers};
use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::Shell;

const MODEL_CLASS: &str = "Model";

/// Read once at startup by `Shell::new`; documented in `main`'s module doc
/// comment.
pub(crate) const GROUP_VARIABLE: &str = "RBX_STUDIO_GROUP";
pub(crate) const UNGROUP_VARIABLE: &str = "RBX_STUDIO_UNGROUP";

/// What one keystroke does at the window level, or `None` if it means
/// something else — see [`action_for`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Action {
    Group,
    Ungroup,
}

/// Real Studio's own bindings for both. Checked at the window level (see
/// `Shell::handle_shell_key`) rather than only while the Explorer holds
/// focus the way `shell::keys`' quick-inserts are: grouping the selection is
/// just as sensible from a viewport click as from the tree.
pub(super) fn action_for(key: &str, modifiers: Modifiers) -> Option<Action> {
    match key {
        "g" if modifiers.control && modifiers.shift => Some(Action::Ungroup),
        "g" if modifiers.control => Some(Action::Group),
        _ => None,
    }
}

/// The one parent every entry of `selected` could move under, or `None` to
/// refuse the whole thing: nothing selected, a referent that no longer
/// resolves, a service among them (services cannot be reparented at all —
/// the same rule `explorer::reparent`'s drag-and-drop already enforces), or
/// a selection that does not share one common parent (real Studio requires
/// a single common parent too).
fn common_parent(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    selected: &[Ref],
) -> Option<Option<Ref>> {
    let mut parents = Vec::with_capacity(selected.len());
    for &reference in selected {
        let instance = dom.get(reference)?;
        if database.is_service(instance.class()) {
            return None;
        }
        parents.push(dom.parent(reference));
    }
    let first = *parents.first()?;
    parents
        .iter()
        .all(|&parent| parent == first)
        .then_some(first)
}

/// What ungrouping `reference` would do — its own parent, to hand its
/// children to, and the children themselves — or `None` to refuse it: not a
/// `Model` or a subclass of one, a service (`Workspace` is a `Model`
/// subclass in Roblox's own class hierarchy, so this is what rules it back
/// out — the same reasoning `common_parent` above applies), or a `Model`
/// with nothing inside it to unwrap.
fn ungroupable(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    reference: Ref,
) -> Option<(Option<Ref>, Vec<Ref>)> {
    let instance = dom.get(reference)?;
    let class = instance.class();
    if database.is_service(class) || !database.is_subclass_of(class, MODEL_CLASS) {
        return None;
    }
    let children = instance.children().to_vec();
    (!children.is_empty()).then(|| (dom.parent(reference), children))
}

/// Whether Group would wrap anything: [`common_parent`]'s own answer, which
/// is what `group_selected` returns early on. Asked by the ribbon so the
/// tile is greyed exactly when the command would do nothing — the selection
/// is empty, holds a service, or straddles two parents.
pub(super) fn has_groupable(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    selected: &[Ref],
) -> bool {
    common_parent(dom, database, selected).is_some()
}

/// Whether Ungroup would unwrap anything. `ungroup_selected` unwraps every
/// `Model` in the selection and ignores the rest, so *one* is enough — the
/// same "skip what cannot follow along rather than refuse the whole call"
/// rule the handler itself follows.
pub(super) fn has_ungroupable(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    selected: &[Ref],
) -> bool {
    selected
        .iter()
        .any(|&reference| ungroupable(dom, database, reference).is_some())
}

/// The DOM half of a Group: creates the wrapping `Model` and moves every
/// `selected` instance under it, returning the new `Model`'s referent. Split
/// out from `Shell::group_selected` so it can be exercised without a live
/// GPUI `Context` (see `shell::history`'s own tests for why this codebase's
/// `Shell` methods stop being unit-testable past their `push_history`/
/// `take_changes` pair).
fn apply_group(dom: &mut WeakDom, selected: &[Ref], parent: Option<Ref>) -> Ref {
    let model = dom.new_instance(MODEL_CLASS, MODEL_CLASS, parent);
    for &reference in selected {
        dom.set_parent(reference, Some(model));
    }
    model
}

/// The DOM half of an Ungroup: for each `(model, parent, children)` entry
/// (see [`ungroupable`]), reparents every child onto `parent` and removes
/// the now-empty `model`. Returns every freed child, in order, for the
/// selection to land on afterwards.
fn apply_ungroup(dom: &mut WeakDom, groups: &[(Ref, Option<Ref>, Vec<Ref>)]) -> Vec<Ref> {
    let mut freed = Vec::new();
    for (model, parent, children) in groups {
        for &child in children {
            dom.set_parent(child, *parent);
            freed.push(child);
        }
        dom.remove(*model);
    }
    freed
}

impl Shell {
    /// The window-level `on_key_down` handler's group/ungroup half; called
    /// from `Shell::handle_shell_key` alongside `shell::save`/
    /// `shell::history`'s own checks — see [`action_for`] for why this runs
    /// at the window level rather than only while the Explorer has focus.
    pub(super) fn handle_group_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) {
        match action_for(&keystroke.key, keystroke.modifiers) {
            Some(Action::Group) => self.group_selected(cx),
            Some(Action::Ungroup) => self.ungroup_selected(cx),
            None => {}
        }
    }

    /// Wraps the current selection in one new `Model`, parented exactly
    /// where the selection itself was. A no-op — refused cleanly rather than
    /// doing something surprising — unless [`common_parent`] finds one
    /// shared parent for the whole selection. `pub(crate)`: also
    /// `menu_bar`'s Group item's entry point, so a menu click runs the exact
    /// same path `Ctrl+G` does.
    pub(crate) fn group_selected(&mut self, cx: &mut Context<Self>) {
        let selected = self.selected_all().to_vec();
        let Some(parent) = common_parent(&self.dom, &self.database, &selected) else {
            return;
        };

        // See `shell::history`: snapshotted before the mutations below, so
        // one Group is one undo step however many instances it wraps.
        self.push_history();
        let model = apply_group(&mut self.dom, &selected, parent);
        let changes = self.dom.take_changes();

        self.rebuild_explorer(cx);
        self.reselect(vec![model], cx);
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);
        cx.notify();
    }

    /// Unwraps every selected `Model` (or subclass) back into its own
    /// parent, removing it once its children are out. A selected instance
    /// that is not one of those is skipped rather than refusing the whole
    /// call — ungrouping a mixed selection still does whatever part of it
    /// makes sense — and a selection with nothing groupable in it is a clean
    /// no-op. `pub(crate)`: also `menu_bar`'s Ungroup item's entry point.
    pub(crate) fn ungroup_selected(&mut self, cx: &mut Context<Self>) {
        let selected = self.selected_all().to_vec();
        let groups: Vec<(Ref, Option<Ref>, Vec<Ref>)> = selected
            .iter()
            .filter_map(|&reference| {
                ungroupable(&self.dom, &self.database, reference)
                    .map(|(parent, children)| (reference, parent, children))
            })
            .collect();
        if groups.is_empty() {
            return;
        }

        // See `shell::history`: snapshotted before the mutations below, so
        // one Ungroup is one undo step however many models it unwraps.
        self.push_history();
        let freed = apply_ungroup(&mut self.dom, &groups);
        let changes = self.dom.take_changes();

        self.rebuild_explorer(cx);
        self.reselect(freed, cx);
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);
        cx.notify();
    }

    /// `RBX_STUDIO_GROUP=1` / `RBX_STUDIO_UNGROUP=1`: documented in `main`'s
    /// module doc comment. A screenshot aid, applied once right after the
    /// Explorer delete/insert debug aids (see `Shell::new`), through the
    /// exact path `Ctrl+G`/`Ctrl+Shift+G` would — nothing else can send the
    /// window a keystroke on the editor's behalf.
    pub(super) fn apply_debug_group(&mut self, cx: &mut Context<Self>) {
        if std::env::var(GROUP_VARIABLE).is_ok() {
            self.group_selected(cx);
        }
        if std::env::var(UNGROUP_VARIABLE).is_ok() {
            self.ungroup_selected(cx);
        }
    }
}

#[cfg(test)]
mod tests;
