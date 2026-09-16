//! Running a Command Bar script: swapping the canonical DOM into a throwaway
//! `rbx_lua::Runtime` and reflecting whatever it changed in the Explorer and
//! the viewport.

use std::rc::Rc;

use gpui_kit::{Context, ScrollStrategy};
use rbx_dom::{Change, Ref, WeakDom};

use crate::command_bar::{self, Feedback};
use crate::explorer::{self, Explorer};

use super::Shell;

impl Shell {
    /// Selects `name`'s first match in the current DOM the way clicking its
    /// row would, expanding whatever ancestors were collapsed — the only way
    /// a screenshot can show an instance `RBX_STUDIO_RUN` just created, since
    /// nothing else can click the tree on the editor's behalf.
    pub(super) fn select_by_name(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(reference) = explorer::find_by_name(&self.dom, name) else {
            return;
        };
        self.select(reference, cx);
    }

    /// Adds `name`'s first match to the selection exactly as a
    /// `Shift`/`Ctrl`/`Cmd`-click on it would (see [`Shell::extend_selection`]),
    /// rather than replacing it the way [`Shell::select_by_name`] does. A
    /// no-op if nothing resolves.
    pub(super) fn extend_by_name(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(reference) = explorer::find_by_name(&self.dom, name) else {
            return;
        };
        self.extend_selection(reference, cx);
    }

    /// `RBX_STUDIO_SELECT=<name>[,<name>...]`: selects the first name, then
    /// adds each further one the way `Shift`/`Ctrl`/`Cmd`-click would —
    /// documented on its call sites in `Shell::new`.
    pub(super) fn apply_debug_select(&mut self, spec: &str, cx: &mut Context<Self>) {
        let mut names = spec
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty());
        if let Some(first) = names.next() {
            self.select_by_name(first, cx);
        }
        for name in names {
            self.extend_by_name(name, cx);
        }
    }

    /// Selects `reference` alone in the Explorer the way clicking its row
    /// would, expanding whatever ancestors were collapsed and replacing
    /// whatever else was selected. Shared with `shell::keys`, which selects a
    /// freshly inserted instance the same way.
    pub(super) fn select(&mut self, reference: Ref, cx: &mut Context<Self>) {
        let Some(item) = self.explorer.item(reference) else {
            return;
        };
        // `set_selected_item` notifies the tree-change observer that mirrors
        // it into `self.selection` (`Shell::sync_selection`), but only once
        // GPUI gets around to flushing it — soon enough after a real click,
        // but never before this same call chain returns when it runs here,
        // synchronously, from a debug var applied during `Shell::new`.
        // Setting `self.selection` directly below, rather than waiting for
        // that observer, is what makes it, the Properties panel and the
        // viewport outline agree with the row immediately; by the time the
        // observer does eventually run, they already match, so it is a
        // deliberate no-op there.
        let tree = self.tree.clone();
        tree.update(cx, |tree, cx| {
            tree.set_selected_item(Some(&item), cx);
            tree.reveal_item(&item.id, ScrollStrategy::Center, cx);
        });
        if self.selection.set(Some(reference)) {
            self.selection_changed(cx);
        }
    }

    /// Clears the whole selection, synchronously (see [`Shell::select`]'s doc
    /// comment) — `shell::keys`' delete key, once the row it removed was the
    /// whole selection, leaves nothing behind to keep selected.
    pub(super) fn deselect(&mut self, cx: &mut Context<Self>) {
        let tree = self.tree.clone();
        tree.update(cx, |tree, cx| tree.set_selected_item(None, cx));
        if self.selection.set(None) {
            self.selection_changed(cx);
        }
    }

    /// `Shift`/`Ctrl`/`Cmd`-click in the viewport: toggles one more top-level
    /// object into or out of the selection instead of replacing it the way
    /// [`Shell::select`] does. The Explorer's `TreeState` can track only one
    /// selected row, so rather than fight it this points that one row at
    /// whatever the toggle leaves as the anchor (see `shell::selection`) and
    /// leaves the rest of the set for `self.selection` alone to remember —
    /// the outline, the gizmo and the Explorer's own highlight (see
    /// `shell::panels::instance_tree`) all read that directly rather than the
    /// tree's idea of "selected".
    pub(super) fn extend_selection(&mut self, reference: Ref, cx: &mut Context<Self>) {
        self.selection.toggle(reference);

        let anchor = self.selection.get().and_then(|r| self.explorer.item(r));
        let tree = self.tree.clone();
        tree.update(cx, |tree, cx| tree.set_selected_item(anchor.as_ref(), cx));

        self.selection_changed(cx);
    }

    /// Enter in the Command Bar: reads the input's current text and runs it.
    pub(super) fn run_typed_command(&mut self, cx: &mut Context<Self>) {
        let source = self.command_bar.input().read(cx).value().to_string();
        self.run_command(&source, cx);
    }

    /// Runs `source` against the place, the way the Command Bar always does:
    /// the DOM is swapped into a throwaway `rbx_lua::Runtime` (which has to own
    /// it while the script runs) and swapped back out, mutated or not, once it
    /// returns.
    pub(super) fn run_command(&mut self, source: &str, cx: &mut Context<Self>) {
        // Snapshotted before the swap below, whether or not the script ends
        // up mutating anything — see `shell::history`. Also drains the
        // change log: everything logged so far was already reflected as it
        // happened (a Properties panel commit, say), so only what this
        // script itself does is of interest to `rebuild_after_script`.
        self.push_history();
        // `WeakDom::new()` is only ever seen back if `run` itself could not be
        // built (an engine fault, not a script one) — see `command_bar::run`.
        let dom = std::mem::replace(&mut self.dom, WeakDom::new());
        let (dom, result) = command_bar::run(dom, &self.database, source);
        self.dom = dom;

        let succeeded = result.is_ok();
        let feedback = Feedback::from_run(result);
        // Logged before the Command Bar's own label is overwritten below: the
        // Output panel is the only place a run's outcome survives past the
        // next one (see `shell::output`).
        self.output.push(source, feedback.clone());
        self.command_bar.set_feedback(feedback);
        if succeeded {
            self.rebuild_after_script(cx);
        }
        cx.notify();
    }

    /// Reflects a successful script in everything that was built from the old
    /// DOM: the Explorer's rows and the viewport's scene. The Properties panel
    /// reads `self.dom` itself, so the re-render is all it needs.
    ///
    /// A script that wrote to one instance only (`workspace.Part.Transparency
    /// = 0.5`, `part.Parent = model`, a part given a `Size` and a `CFrame`
    /// in one go — the bulk of what gets typed into the bar) takes the same
    /// viewport path a Properties-panel commit of each of those edits would
    /// (see [`Shell::reflect_changes`]); any other run rebuilds the whole
    /// scene, the one answer that is right whatever the script did.
    fn rebuild_after_script(&mut self, cx: &mut Context<Self>) {
        self.rebuild_explorer(cx);
        let changes = self.dom.take_changes();
        self.reflect_changes(&changes, cx);
        // See `shell::history`: pairs this run's log with the snapshot
        // `push_history` took before it, so undoing it can be classified the
        // same way redoing it just was, above.
        self.record_history_change(changes);
    }

    /// Reflects a mutation's `Change` log in the 3D view: the in-place patch
    /// `shell::edit::reflect_in_viewport` takes, once per property
    /// [`single_instance_change`] finds written on the one instance the log
    /// touched, or a full [`Shell::reload_viewport`] for anything it cannot
    /// classify. Shared with `shell::history`'s undo/redo, which reflects
    /// the same log from the other end — so the two can never disagree about
    /// which edits are cheap to show.
    pub(super) fn reflect_changes(&mut self, changes: &[Change], cx: &mut Context<Self>) {
        match single_instance_change(changes) {
            Some((reference, names)) => {
                for name in &names {
                    self.reflect_in_viewport(reference, name, cx);
                }
            }
            None => self.reload_viewport(cx),
        }
    }

    /// Rebuilds the Explorer's rows from the current `self.dom`, keeping the
    /// selection when its referent still resolves. Shared with the
    /// Properties panel's own edit path (see `shell::edit`), since renaming
    /// an instance moves its row the same way a script's rename would.
    pub(super) fn rebuild_explorer(&mut self, cx: &mut Context<Self>) {
        let explorer = Explorer::from_dom(&self.dom, self.icons.as_ref());
        let items = explorer.items(self.show_all_services);
        // A rename, reparent or destroy can invalidate the selection; kept
        // only if its referent still resolves in the rebuilt tree.
        let kept = self
            .selected()
            .filter(|reference| self.dom.get(*reference).is_some());
        let preselected = kept.and_then(|reference| explorer.item(reference));

        self.explorer = Rc::new(explorer);
        self.tree.update(cx, |tree, cx| {
            tree.set_items(items, cx);
            tree.set_selected_item(preselected.as_ref(), cx);
        });
    }

    /// Reflects `self.dom` in the 3D view. Cheap enough to call after every
    /// committed edit, not just a script run: `Headless::reload` is what a
    /// single-property change needs too.
    pub(super) fn reload_viewport(&mut self, cx: &mut Context<Self>) {
        let dom = self.dom.clone();
        // The draggers are placed from a copy of every selected part's
        // transform held on this side (see `transform::Targets`), and a
        // reload is exactly the case where whatever moved one of them was not
        // one of the edits `reflect_in_viewport` refreshes that copy for.
        let targets = crate::transform::Targets::read(&self.dom, self.selected_all());
        self.viewport.update(cx, |viewport, _| {
            viewport.reload(dom);
            viewport.set_targets(targets);
        });
        // A reload is also the one case where parts other than the selected
        // one may have moved, appeared or gone — so what a drag can soft-snap
        // onto has to be read again too.
        self.sync_snap_neighbours(cx);
    }
}

/// The one instance a change log amounts to, with every property written on
/// it in the order first written — each the `(instance, property)` pair
/// `Shell::reflect_in_viewport` classifies, a reparent spelled `Parent` —
/// or `None` for a log the viewport can only reflect by rebuilding: one that
/// touches a second instance, or creates or removes one (an empty log is
/// `None` too: nothing to patch is not the same as nothing to do).
///
/// Several writes on one instance stay a fast patch because one gesture
/// produces them: a Scale drag writes `size` and `CFrame` together (see
/// `shell::drag`), and a script that sets three properties on one part is
/// no wider a change than one that sets one. A name written twice is listed
/// once — the patch reads the value back from the DOM, so reflecting it
/// twice would only redo the same work.
///
/// `pub(super)`: also `shell::history`'s classifier for undo/redo, reused
/// rather than duplicated (see that module's doc comment).
pub(super) fn single_instance_change(changes: &[Change]) -> Option<(Ref, Vec<String>)> {
    let mut edits: Option<(Ref, Vec<String>)> = None;
    for change in changes {
        let (referent, name) = match change {
            Change::Property { referent, name } => (*referent, name.as_str()),
            Change::Parent { referent, .. } => (*referent, "Parent"),
            Change::Added(_) | Change::Removed(_) => return None,
        };
        let (instance, names) = edits.get_or_insert_with(|| (referent, Vec::new()));
        if *instance != referent {
            return None;
        }
        if !names.iter().any(|known| known == name) {
            names.push(name.to_string());
        }
    }
    edits
}

#[cfg(test)]
mod tests {
    use rbx_dom::{Change, Ref};

    use super::single_instance_change;

    fn write(id: u32, name: &str) -> Change {
        Change::Property {
            referent: Ref::new(id),
            name: name.to_string(),
        }
    }

    fn names(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn one_property_write_is_the_edit_it_names() {
        assert_eq!(
            single_instance_change(&[write(7, "Transparency")]),
            Some((Ref::new(7), names(&["Transparency"])))
        );
    }

    // A reparent is logged under its own variant, but classifies exactly as
    // a `Parent` property write would.
    #[test]
    fn one_reparent_is_a_parent_edit() {
        let changes = [Change::Parent {
            referent: Ref::new(7),
            old: Some(Ref::new(1)),
            new: Some(Ref::new(2)),
        }];

        assert_eq!(
            single_instance_change(&changes),
            Some((Ref::new(7), names(&["Parent"])))
        );
    }

    // The shape a Scale drag step logs (see `shell::drag::resize_part`):
    // two properties, one part. Each is its own in-place patch, in the
    // order written, rather than a reason to rebuild the scene.
    #[test]
    fn several_writes_on_one_instance_are_each_an_edit_on_it() {
        assert_eq!(
            single_instance_change(&[write(7, "size"), write(7, "CFrame")]),
            Some((Ref::new(7), names(&["size", "CFrame"])))
        );
    }

    // A script that sets the same property twice, or reparents a part and
    // then moves it, is still one instance's worth of patches — and a name
    // is patched once however many times it was written.
    #[test]
    fn a_repeated_name_is_listed_once_and_a_reparent_sits_among_the_rest() {
        let reparent = Change::Parent {
            referent: Ref::new(7),
            old: None,
            new: Some(Ref::new(2)),
        };
        let changes = [
            write(7, "Transparency"),
            reparent,
            write(7, "CFrame"),
            write(7, "Transparency"),
        ];

        assert_eq!(
            single_instance_change(&changes),
            Some((Ref::new(7), names(&["Transparency", "Parent", "CFrame"])))
        );
    }

    // Anything wider — nothing at all, a second instance, an instance added
    // or removed (even alongside writes on that same instance, the way
    // `Instance.new` followed by its setup logs) — leaves nothing one patch
    // can show, so the caller rebuilds.
    #[test]
    fn anything_wider_than_one_instance_is_not_classified() {
        assert_eq!(single_instance_change(&[]), None);
        assert_eq!(
            single_instance_change(&[write(1, "CFrame"), write(2, "CFrame")]),
            None,
            "a group drag: one write per part"
        );
        assert_eq!(single_instance_change(&[Change::Added(Ref::new(1))]), None);
        assert_eq!(
            single_instance_change(&[Change::Removed(Ref::new(1))]),
            None
        );
        assert_eq!(
            single_instance_change(&[Change::Added(Ref::new(1)), write(1, "Name")]),
            None,
            "an insert is a create however many properties it then sets"
        );
        assert_eq!(
            single_instance_change(&[write(1, "Name"), Change::Removed(Ref::new(1))]),
            None,
            "a delete is a delete whatever was written first"
        );
    }
}
