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
    /// A script that wrote exactly one property or moved exactly one instance
    /// (`workspace.Part.Transparency = 0.5`, `part.Parent = model` — the
    /// bulk of what gets typed into the bar) takes the same viewport path a
    /// Properties-panel commit of that edit would (see `shell::edit`); any
    /// other run rebuilds the whole scene, the one answer that is right
    /// whatever the script did.
    fn rebuild_after_script(&mut self, cx: &mut Context<Self>) {
        self.rebuild_explorer(cx);
        let changes = self.dom.take_changes();
        match single_change(&changes) {
            Some((reference, name)) => self.reflect_in_viewport(reference, &name, cx),
            None => self.reload_viewport(cx),
        }
        // See `shell::history`: pairs this run's log with the snapshot
        // `push_history` took before it, so undoing it can be classified the
        // same way redoing it just was, above.
        self.record_history_change(changes);
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
        self.viewport.update(cx, |viewport, _| viewport.reload(dom));
        // The box and the draggers are both placed from what this side reads
        // out of the DOM (see `shell::selection`), and a reload is exactly the
        // case where whatever moved them was not one of the edits
        // `reflect_in_viewport` refreshes that reading for — a script that
        // parents a `Part` under the selected model included, which widens the
        // box as well as adding a target.
        self.sync_viewport_selection(cx);
        // A reload is also the one case where parts other than the selected
        // one may have moved, appeared or gone — so what a drag can soft-snap
        // onto has to be read again too.
        self.sync_snap_neighbours(cx);
    }
}

/// The one edit a script's change log amounts to, as the `(instance,
/// property)` pair `Shell::reflect_in_viewport` classifies — `None` for any
/// log that is not exactly one property write or one reparent. `pub(super)`:
/// also `shell::history`'s classifier for undo/redo, reused rather than
/// duplicated (see that module's doc comment).
pub(super) fn single_change(changes: &[Change]) -> Option<(Ref, String)> {
    match changes {
        [Change::Property { referent, name }] => Some((*referent, name.clone())),
        [Change::Parent { referent, .. }] => Some((*referent, "Parent".to_string())),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use rbx_dom::{Change, Ref};

    use super::single_change;

    #[test]
    fn one_property_write_is_the_edit_it_names() {
        let changes = [Change::Property {
            referent: Ref::new(7),
            name: "Transparency".to_string(),
        }];

        assert_eq!(
            single_change(&changes),
            Some((Ref::new(7), "Transparency".to_string()))
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
            single_change(&changes),
            Some((Ref::new(7), "Parent".to_string()))
        );
    }

    // Anything else — nothing, several writes, an instance added or removed
    // — has no single edit to classify, so the caller rebuilds.
    #[test]
    fn anything_else_is_not_a_single_edit() {
        let write = |id: u32| Change::Property {
            referent: Ref::new(id),
            name: "Name".to_string(),
        };

        assert_eq!(single_change(&[]), None);
        assert_eq!(single_change(&[write(1), write(2)]), None);
        assert_eq!(single_change(&[Change::Added(Ref::new(1))]), None);
        assert_eq!(single_change(&[Change::Removed(Ref::new(1))]), None);
    }
}
