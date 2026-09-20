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

use crate::explorer;

use super::Shell;

const WORKSPACE_NAME: &str = "Workspace";

/// What one keystroke does at the window level, or `None` if it means
/// something else — see [`action_for`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Action {
    Copy,
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
    if !modifiers.control {
        return None;
    }
    match key {
        "c" => Some(Action::Copy),
        "v" if modifiers.shift => Some(Action::PasteInto),
        "v" => Some(Action::Paste),
        "d" => Some(Action::Duplicate),
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
        // See `shell::history`: snapshotted before the inserts below, so one
        // Paste is one undo step however many entries the clipboard holds.
        self.push_history();
        let mut dom = std::mem::replace(&mut self.dom, WeakDom::new());
        let mut pasted: Vec<Ref> = Vec::new();
        for &parent in parents {
            for node in &self.clipboard {
                pasted.push(materialize(&mut dom, node, parent));
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

        // See `shell::history`: snapshotted before the inserts below, so one
        // Duplicate is one undo step however many instances it duplicates.
        self.push_history();
        let mut dom = std::mem::replace(&mut self.dom, WeakDom::new());
        let duplicated: Vec<Ref> = selected
            .into_iter()
            .filter_map(|reference| {
                let parent = dom.parent(reference);
                snapshot(&dom, reference).map(|node| materialize(&mut dom, &node, parent))
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
mod tests {
    use rbx_dom::Change;
    use rbx_reflection::ReflectionDatabase;

    use super::*;

    fn database() -> ReflectionDatabase {
        ReflectionDatabase::embedded()
    }

    fn ctrl() -> Modifiers {
        Modifiers {
            control: true,
            ..Modifiers::none()
        }
    }

    #[test]
    fn ctrl_c_v_d_map_to_copy_paste_duplicate() {
        assert_eq!(action_for("c", ctrl()), Some(Action::Copy));
        assert_eq!(action_for("v", ctrl()), Some(Action::Paste));
        assert_eq!(action_for("d", ctrl()), Some(Action::Duplicate));
    }

    #[test]
    fn ctrl_shift_v_is_paste_into_not_plain_paste() {
        let shifted = Modifiers {
            shift: true,
            ..ctrl()
        };
        assert_eq!(action_for("v", shifted), Some(Action::PasteInto));
        assert_eq!(action_for("v", ctrl()), Some(Action::Paste));
    }

    #[test]
    fn without_control_nothing_matches() {
        assert_eq!(action_for("c", Modifiers::none()), None);
        assert_eq!(action_for("v", Modifiers::none()), None);
        assert_eq!(action_for("d", Modifiers::none()), None);
    }

    #[test]
    fn an_unrelated_key_does_nothing_even_with_control() {
        assert_eq!(action_for("z", ctrl()), None);
    }

    #[test]
    fn a_service_is_not_copyable() {
        let mut dom = WeakDom::new();
        let workspace = dom.new_instance("Workspace", "Workspace", None);
        assert_eq!(copyable(&dom, &database(), &[workspace]), Vec::<Ref>::new());
    }

    #[test]
    fn a_mixed_selection_keeps_only_the_non_service_entries() {
        let mut dom = WeakDom::new();
        let workspace = dom.new_instance("Workspace", "Workspace", None);
        let part = dom.new_instance("Part", "Part", Some(workspace));
        assert_eq!(copyable(&dom, &database(), &[workspace, part]), vec![part]);
    }

    #[test]
    fn nothing_selected_is_nothing_copyable() {
        let dom = WeakDom::new();
        assert_eq!(copyable(&dom, &database(), &[]), Vec::<Ref>::new());
    }

    #[test]
    fn a_reference_no_longer_in_the_dom_cannot_be_copied() {
        let mut dom = WeakDom::new();
        let part = dom.new_instance("Part", "Part", None);
        dom.remove(part);
        assert_eq!(copyable(&dom, &database(), &[part]), Vec::<Ref>::new());
    }

    #[test]
    fn snapshotting_a_model_carries_its_descendants() {
        let mut dom = WeakDom::new();
        let model = dom.new_instance("Model", "Model", None);
        let a = dom.new_instance("Part", "A", Some(model));
        dom.new_instance("Part", "B", Some(a));

        let clipped = snapshot(&dom, model).expect("model resolves");
        assert_eq!(clipped.class, "Model");
        assert_eq!(clipped.children.len(), 1);
        assert_eq!(clipped.children[0].name, "A");
        assert_eq!(clipped.children[0].children[0].name, "B");
    }

    #[test]
    fn a_non_archivable_descendant_and_its_own_subtree_are_excluded() {
        let mut dom = WeakDom::new();
        let model = dom.new_instance("Model", "Model", None);
        dom.new_instance("Part", "Kept", Some(model));
        let skipped = dom.new_instance("Part", "Skipped", Some(model));
        dom.set_property(skipped, "Archivable", Variant::Bool(false))
            .unwrap();
        dom.new_instance("Part", "GrandchildOfSkipped", Some(skipped));

        let clipped = snapshot(&dom, model).unwrap();
        assert_eq!(clipped.children.len(), 1, "only the archivable child");
        assert_eq!(clipped.children[0].name, "Kept");
    }

    #[test]
    fn a_non_archivable_root_is_still_copied() {
        // Real Studio's Copy/Duplicate ignore the copied instance's *own*
        // Archivable — unlike `Instance:Clone()`, which would refuse it —
        // so this only applies to descendants, never to the thing that was
        // actually selected and copied.
        let mut dom = WeakDom::new();
        let part = dom.new_instance("Part", "Part", None);
        dom.set_property(part, "Archivable", Variant::Bool(false))
            .unwrap();

        let clipped = snapshot(&dom, part);
        assert!(clipped.is_some());
    }

    #[test]
    fn the_copy_is_always_archivable_even_if_the_original_was_not() {
        let mut dom = WeakDom::new();
        let part = dom.new_instance("Part", "Part", None);
        dom.set_property(part, "Archivable", Variant::Bool(false))
            .unwrap();

        let clipped = snapshot(&dom, part).unwrap();
        let copy = materialize(&mut dom, &clipped, None);

        assert_eq!(
            dom.get(copy).unwrap().properties().get("Archivable"),
            Some(&Variant::Bool(true))
        );
        // The original is untouched — only the copy was forced.
        assert_eq!(
            dom.get(part).unwrap().properties().get("Archivable"),
            Some(&Variant::Bool(false))
        );
    }

    #[test]
    fn materializing_a_model_recreates_its_whole_subtree_under_new_referents() {
        let mut dom = WeakDom::new();
        let model = dom.new_instance("Model", "Model", None);
        let original_child = dom.new_instance("Part", "Child", Some(model));

        let clipped = snapshot(&dom, model).unwrap();
        let copy = materialize(&mut dom, &clipped, None);

        assert_ne!(copy, model, "the copy is a new instance, not the original");
        let copied_child = dom.get(copy).unwrap().children()[0];
        assert_ne!(copied_child, original_child);
        assert_eq!(dom.get(copied_child).unwrap().name(), "Child");
        assert_eq!(dom.get(copied_child).unwrap().class(), "Part");
    }

    #[test]
    fn duplicating_a_script_carries_its_source_along() {
        // `Source` is just another property in `properties()` (see
        // `script_editor::source`) — no script-specific handling exists
        // anywhere in this module, so this is really a check that the
        // generic property copy above doesn't quietly drop it.
        let mut dom = WeakDom::new();
        let script = dom.new_instance("Script", "Script", None);
        dom.set_property(
            script,
            "Source",
            Variant::String("print(\"hi\")".to_owned()),
        )
        .unwrap();

        let clipped = snapshot(&dom, script).unwrap();
        let copy = materialize(&mut dom, &clipped, None);

        assert_eq!(
            dom.get(copy).unwrap().properties().get("Source"),
            Some(&Variant::String("print(\"hi\")".to_owned()))
        );
    }

    #[test]
    fn mutating_the_copy_does_not_affect_the_original() {
        let mut dom = WeakDom::new();
        let part = dom.new_instance("Part", "Part", None);
        dom.set_property(part, "Transparency", Variant::Float32(0.0))
            .unwrap();

        let clipped = snapshot(&dom, part).unwrap();
        let copy = materialize(&mut dom, &clipped, None);

        dom.set_property(copy, "Transparency", Variant::Float32(1.0))
            .unwrap();

        assert_eq!(
            dom.get(part).unwrap().properties().get("Transparency"),
            Some(&Variant::Float32(0.0)),
            "writing the copy must not reach the original"
        );
        assert_eq!(
            dom.get(copy).unwrap().properties().get("Transparency"),
            Some(&Variant::Float32(1.0))
        );
    }

    #[test]
    fn mutating_the_original_after_copying_does_not_affect_the_copy() {
        let mut dom = WeakDom::new();
        let part = dom.new_instance("Part", "Part", None);
        dom.set_property(part, "Transparency", Variant::Float32(0.0))
            .unwrap();

        let clipped = snapshot(&dom, part).unwrap();
        let copy = materialize(&mut dom, &clipped, None);

        dom.set_property(part, "Transparency", Variant::Float32(0.5))
            .unwrap();

        assert_eq!(
            dom.get(copy).unwrap().properties().get("Transparency"),
            Some(&Variant::Float32(0.0)),
            "a later edit to the original must not reach the earlier copy"
        );
    }

    #[test]
    fn an_internal_reference_is_remapped_to_the_copy() {
        let mut dom = WeakDom::new();
        let model = dom.new_instance("Model", "Model", None);
        let target = dom.new_instance("Part", "Target", Some(model));
        let value = dom.new_instance("ObjectValue", "Link", Some(model));
        dom.set_property(value, "Value", Variant::Ref(target))
            .unwrap();

        let clipped = snapshot(&dom, model).unwrap();
        let copy = materialize(&mut dom, &clipped, None);

        let copy_instance = dom.get(copy).unwrap();
        let copied_target = copy_instance
            .children()
            .iter()
            .copied()
            .find(|&r| dom.get(r).unwrap().name() == "Target")
            .unwrap();
        let copied_value = copy_instance
            .children()
            .iter()
            .copied()
            .find(|&r| dom.get(r).unwrap().name() == "Link")
            .unwrap();

        assert_eq!(
            dom.get(copied_value).unwrap().properties().get("Value"),
            Some(&Variant::Ref(copied_target)),
            "a reference to a sibling that was copied along with it must follow the copy"
        );
    }

    #[test]
    fn a_reference_outside_the_copied_subtree_keeps_pointing_at_the_original() {
        let mut dom = WeakDom::new();
        let outside = dom.new_instance("Part", "Outside", None);
        let value = dom.new_instance("ObjectValue", "Link", None);
        dom.set_property(value, "Value", Variant::Ref(outside))
            .unwrap();

        let clipped = snapshot(&dom, value).unwrap();
        let copy = materialize(&mut dom, &clipped, None);

        assert_eq!(
            dom.get(copy).unwrap().properties().get("Value"),
            Some(&Variant::Ref(outside)),
            "a reference to something outside the copy must not be rewritten"
        );
    }

    #[test]
    fn duplicating_two_instances_is_one_batch_of_changes() {
        let mut dom = WeakDom::new();
        let workspace = dom.new_instance("Workspace", "Workspace", None);
        let a = dom.new_instance("Part", "A", Some(workspace));
        let b = dom.new_instance("Part", "B", Some(workspace));
        dom.take_changes(); // the three creates above, not what's under test

        let clipped_a = snapshot(&dom, a).unwrap();
        let clipped_b = snapshot(&dom, b).unwrap();
        materialize(&mut dom, &clipped_a, Some(workspace));
        materialize(&mut dom, &clipped_b, Some(workspace));
        // One drain, mirroring `Shell::duplicate_selected`'s single
        // `push_history`/`take_changes` pair — this is what makes
        // duplicating a multi-instance selection one undo step rather than
        // one per instance.
        let changes = dom.take_changes();

        let added = changes
            .iter()
            .filter(|change| matches!(change, Change::Added(_)))
            .count();
        assert_eq!(added, 2, "both duplicates land in the same change log");
    }
}
