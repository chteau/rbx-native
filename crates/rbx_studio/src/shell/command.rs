//! Running a Command Bar script: swapping the canonical DOM into a throwaway
//! `rbx_lua::Runtime` and reflecting whatever it changed in the Explorer and
//! the viewport.

use std::rc::Rc;

use gpui_kit::{Context, ScrollStrategy};
use rbx_dom::{Change, Ref, WeakDom};

use crate::command_bar::{self, Feedback};
use crate::explorer::{self, Explorer};
use crate::transform::Targets;

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
    /// Sets the selection to `kept` as a whole — every entry of a
    /// multi-selection that an undo, redo or rebuild left standing, in its
    /// order, with the first as the Explorer's row — rather than collapsing
    /// it to its anchor the way [`Shell::select`] would.
    pub(super) fn reselect(&mut self, kept: Vec<Ref>, cx: &mut Context<Self>) {
        let anchor = kept.first().and_then(|&r| self.explorer.item(r));
        let tree = self.tree.clone();
        tree.update(cx, |tree, cx| {
            tree.set_selected_item(anchor.as_ref(), cx);
            if let Some(anchor) = &anchor {
                tree.reveal_item(&anchor.id, ScrollStrategy::Center, cx);
            }
        });
        if self.selection.replace(kept) {
            self.selection_changed(cx);
        }
    }

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
    /// Whatever the script did — one property, a hundred parts moved, a
    /// model built or destroyed — reaches the viewport as the change log it
    /// produced, patched instance by instance (see
    /// [`Shell::reflect_changes`]); there is no script-shaped fallback.
    fn rebuild_after_script(&mut self, cx: &mut Context<Self>) {
        self.rebuild_explorer(cx);
        let changes = self.dom.take_changes();
        self.reflect_changes(&changes, cx);
        // See `shell::history`: pairs this run's log with the snapshot
        // `push_history` took before it, so undoing it reflects the same log
        // from the other end.
        self.record_history_change(changes);
    }

    /// Reflects a mutation's `Change` log in the 3D view: every instance it
    /// names is patched in place on the render thread, whatever and however
    /// many they are (see `rbx_viewer::Headless::apply_changes`), and the
    /// two things on this side that mirror the DOM through the viewport are
    /// refreshed as the log warrants (see [`refresh_for`]): the draggers'
    /// copy of the selected parts' transforms (`transform::Targets`) when a
    /// selected part or the tree itself changed, and the boxes a free drag
    /// soft-snaps onto when anything *other* than the selection did — a drag
    /// moves nothing but the selection, and re-reading those per mouse move
    /// would walk the whole workspace every frame.
    ///
    /// The one path every mutation takes — a Command Bar script, a
    /// Properties row, a viewport drag, the Explorer's insert, delete and
    /// drag-drop, and undo/redo of each — so no two of them can disagree
    /// about what an edit costs to show.
    pub(super) fn reflect_changes(&mut self, changes: &[Change], cx: &mut Context<Self>) {
        if changes.is_empty() {
            return;
        }
        let refresh = refresh_for(changes, self.selected_all());
        // The instances the log names, not the tree: a drag reflects a
        // change every mouse move, and copying the whole place per move
        // would cost what the patch itself was made to save.
        let snapshots = self.dom.snapshot(changes);
        let targets = refresh
            .targets
            .then(|| Targets::read(&self.dom, &self.database, self.selected_all()));
        self.viewport.update(cx, |viewport, _| {
            viewport.apply_changes(snapshots, changes.to_vec());
            if let Some(targets) = targets {
                viewport.set_targets(targets);
            }
        });
        if refresh.neighbours {
            self.sync_snap_neighbours(cx);
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
}

/// What on this side of the viewport has to be re-read after `changes`:
/// see [`Shell::reflect_changes`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Refresh {
    /// The selected parts' transforms, for the draggers.
    pub(super) targets: bool,
    /// Every unselected part's box, for a drag's soft snap.
    pub(super) neighbours: bool,
}

/// Which of the two mirrors a log invalidates. A structural change — an
/// instance added, removed or moved — invalidates both: the selection may
/// have lost a part or gained one, and so may the neighbours. A property
/// write invalidates only the side it landed on: the targets if it touched
/// a selected part (a typed coordinate, an undo of one), the neighbours if
/// it touched anything else (a script moving parts the user has not
/// selected). A drag writes only selected parts, so it never pays for the
/// workspace walk the neighbours cost.
pub(super) fn refresh_for(changes: &[Change], selected: &[Ref]) -> Refresh {
    let mut refresh = Refresh {
        targets: false,
        neighbours: false,
    };
    for change in changes {
        let referent = match change {
            Change::Property { referent, .. } => *referent,
            Change::Parent { .. } | Change::Added(_) | Change::Removed(_) => {
                return Refresh {
                    targets: true,
                    neighbours: true,
                }
            }
        };
        if selected.contains(&referent) {
            refresh.targets = true;
        } else {
            refresh.neighbours = true;
        }
    }
    refresh
}

#[cfg(test)]
mod tests {
    use rbx_dom::{Change, Ref};

    use super::{refresh_for, Refresh};

    fn write(id: u32, name: &str) -> Change {
        Change::Property {
            referent: Ref::new(id),
            name: name.to_string(),
        }
    }

    const BOTH: Refresh = Refresh {
        targets: true,
        neighbours: true,
    };

    // A drag step: one `CFrame` per selected part, and nothing else moved.
    // The draggers follow; the neighbours are not walked again per frame.
    #[test]
    fn writes_on_the_selection_refresh_the_targets_alone() {
        let selected = [Ref::new(1), Ref::new(2)];
        assert_eq!(
            refresh_for(&[write(1, "CFrame"), write(2, "CFrame")], &selected),
            Refresh {
                targets: true,
                neighbours: false,
            }
        );
    }

    // A script moving parts the user never selected: the draggers stand
    // where they were, but what a drag can snap onto has moved.
    #[test]
    fn writes_off_the_selection_refresh_the_neighbours_alone() {
        assert_eq!(
            refresh_for(&[write(7, "CFrame")], &[Ref::new(1)]),
            Refresh {
                targets: false,
                neighbours: true,
            }
        );
    }

    #[test]
    fn writes_on_both_sides_refresh_both() {
        assert_eq!(
            refresh_for(&[write(1, "size"), write(7, "size")], &[Ref::new(1)]),
            BOTH
        );
    }

    // An insert, a delete or a move can take a part into or out of either
    // set, whichever instance it names.
    #[test]
    fn anything_structural_refreshes_both() {
        let reparent = Change::Parent {
            referent: Ref::new(3),
            old: None,
            new: Some(Ref::new(2)),
        };
        assert_eq!(refresh_for(&[Change::Added(Ref::new(9))], &[]), BOTH);
        assert_eq!(refresh_for(&[Change::Removed(Ref::new(9))], &[]), BOTH);
        assert_eq!(
            refresh_for(&[write(1, "Name"), reparent], &[Ref::new(1)]),
            BOTH
        );
    }

    #[test]
    fn an_empty_log_refreshes_nothing() {
        assert_eq!(
            refresh_for(&[], &[Ref::new(1)]),
            Refresh {
                targets: false,
                neighbours: false,
            }
        );
    }
}
