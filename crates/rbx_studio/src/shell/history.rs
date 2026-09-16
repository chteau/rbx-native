//! Ctrl+Z / Ctrl+Y / Ctrl+Shift+Z, dispatched from the same window-level
//! `Shell::handle_shell_key` Ctrl+S already uses (see `shell::save`) rather
//! than a second `on_key_down` — a key event bubbles up from whatever holds
//! focus, and undo must go through no matter which panel is focused.
//!
//! Undo/redo takes the same fast in-place viewport patch an ordinary edit
//! does (`shell::edit::reflect_in_viewport`) whenever the reverted/reapplied
//! mutation wrote to one instance only — one property, a reparent, or
//! several properties at once the way a Scale drag writes `size` and
//! `CFrame` together (see `shell::drag`) — falling back to a full
//! [`Shell::reload_viewport`] only for anything wider: an instance create or
//! delete, a multi-instance drag, a script that touched more than one thing.
//! `crate::history::History` records the `Change` log each pushed snapshot's
//! mutation produced (see `Shell::push_history`/
//! `Shell::record_history_change`, called from every mutating call site);
//! [`Shell::install`] below hands it to `Shell::reflect_changes` (see
//! `shell::command`) — the exact path a script run's own viewport reflection
//! takes, classifier and all — rather than a second one built for this.
//!
//! `RBX_STUDIO_UNDO=1` applies one undo once, right after startup, through
//! this exact path — a debugging aid for a screenshot that proves a mutation
//! was reverted, since nothing else can send a keystroke to the window on the
//! editor's behalf (see `AGENTS.md`'s safety rules).

use gpui_kit::Context;
use rbx_dom::{Change, WeakDom};

use crate::history;

use super::Shell;

/// Read once at startup by `Shell::new`; documented in this module's doc
/// comment.
pub(crate) const UNDO_VARIABLE: &str = "RBX_STUDIO_UNDO";

impl Shell {
    /// Snapshots `self.dom` onto the undo stack. Called right before every
    /// mutation call site — `shell::command`'s script run, `shell::edit`'s
    /// committed property edit, `shell::keys`'s insert and delete, a
    /// viewport drag's first step — so the snapshot always reflects the DOM
    /// as it stood immediately before that mutation was applied. Also
    /// drains whatever the change log holds already: it belongs to
    /// something already reflected before this checkpoint, not to the
    /// mutation that follows it (see `record_history_change`).
    pub(super) fn push_history(&mut self) {
        self.dom.take_changes();
        let before = self.dom.clone();
        self.push_history_snapshot(before);
    }

    /// [`Shell::push_history`] for a mutation that has to be attempted before
    /// anyone can know whether it will reach the DOM at all: the caller takes
    /// `before` itself, tries the mutation, and pushes only once it has
    /// landed. Draining the stale change log is then the caller's job too,
    /// for the reason `push_history` does it above.
    ///
    /// `shell::scripts`'s debounced `Source` write is the one call site that
    /// needs this — a tab can outlive its script by a frame, and a snapshot
    /// pushed for a write that never happened is a Ctrl+Z that reverts
    /// nothing, having cleared the redo stack to offer it.
    pub(super) fn push_history_snapshot(&mut self, before: WeakDom) {
        self.history.push(before);
    }

    /// Attaches `changes` — the `Change` log the mutation `push_history`
    /// (or, for a multi-step drag, the gesture's own most recent step) just
    /// preceded produced — to the entry currently on top of the undo stack.
    /// Called once per mutating call site, right after that mutation
    /// completes, so undo/redo can classify it later without re-diffing two
    /// `WeakDom` trees.
    pub(super) fn record_history_change(&mut self, changes: Vec<Change>) {
        self.history.record_changes(changes);
    }

    /// Ctrl+Z: installs the DOM as it stood before the last pushed mutation,
    /// if any. A no-op with nothing to undo. `pub(crate)`: also `menu_bar`'s
    /// Undo item's entry point, so a menu click runs the exact same path
    /// Ctrl+Z does.
    pub(crate) fn undo(&mut self, cx: &mut Context<Self>) {
        // A script editor's text reaches the DOM on a debounce (see
        // `shell::scripts`), so without this an undo moments after typing
        // would step over text that had not become a history entry yet, and
        // the pending write would then land on top of the undone DOM.
        self.flush_script_edits(cx);
        if let Some((previous, changes)) = self.history.undo(self.dom.clone()) {
            self.install(previous, &changes, cx);
        }
    }

    /// Ctrl+Y / Ctrl+Shift+Z: symmetric to [`Shell::undo`]. A no-op with
    /// nothing to redo. `pub(crate)` for the same reason as `undo` above.
    pub(crate) fn redo(&mut self, cx: &mut Context<Self>) {
        self.flush_script_edits(cx);
        if let Some((next, changes)) = self.history.redo(self.dom.clone()) {
            self.install(next, &changes, cx);
        }
    }

    /// Installs `dom` as the canonical tree and reflects it in the viewport
    /// — through [`Shell::reflect_changes`]: the fast patch, per property
    /// `changes` shows written on one instance, or a full
    /// [`Shell::reload_viewport`] for anything wider (see this module's doc
    /// comment) — plus the Explorer and the selection: cleared when its
    /// referent no longer resolves in `dom`, the same rule
    /// `shell::keys::selection_after_removal` applies to a delete.
    fn install(&mut self, dom: WeakDom, changes: &[Change], cx: &mut Context<Self>) {
        self.dom = dom;
        self.rebuild_explorer(cx);
        match self
            .selected()
            .filter(|reference| self.dom.get(*reference).is_some())
        {
            Some(kept) => self.select(kept, cx),
            None => self.deselect(cx),
        }
        self.reflect_changes(changes, cx);
        cx.notify();
    }

    /// The window-level `on_key_down` handler's undo/redo half; called from
    /// `Shell::handle_shell_key` alongside `shell::save`'s own check.
    pub(super) fn handle_history_key(
        &mut self,
        keystroke: &gpui_kit::Keystroke,
        window: &gpui_kit::Window,
        cx: &mut Context<Self>,
    ) {
        // See `Shell::script_editor_focused`: the script editor owns Ctrl+Z
        // while it has focus.
        if self.script_editor_focused(window, cx) {
            return;
        }
        match history::action_for(&keystroke.key, keystroke.modifiers) {
            Some(history::Action::Undo) => self.undo(cx),
            Some(history::Action::Redo) => self.redo(cx),
            None => {}
        }
    }

    /// `RBX_STUDIO_UNDO=1`: documented in this module's doc comment.
    pub(super) fn apply_debug_undo(&mut self, cx: &mut Context<Self>) {
        if std::env::var(UNDO_VARIABLE).is_ok() {
            self.undo(cx);
        }
    }
}

// `Shell::undo`/`redo`/`install` themselves need a live GPUI `Context` (a
// window, a viewport entity, an Explorer tree) this crate has no headless
// harness for — see the other `shell::*` test modules, which stop at the
// same boundary and test the pure logic underneath a GPUI call instead of
// the call itself. What is tested below is that same underneath: a real
// `History` (from `crate::history`), fed real `WeakDom` mutations and read
// back through the exact `single_instance_change` classifier
// `Shell::reflect_changes` runs for `install` above, which is the whole of
// what decides fast patch vs. full reload — `install` itself is a thin,
// untestable-without-a-window wrapper around it.
#[cfg(test)]
mod tests {
    use rbx_dom::{CFrameData, Variant, Vector3Data, WeakDom};

    use crate::history::{History, DEFAULT_CAP};
    use crate::script_editor::source;
    use crate::shell::command::single_instance_change;

    fn names(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    fn vector3(x: f32, y: f32, z: f32) -> Variant {
        Variant::Vector3(Vector3Data { x, y, z })
    }

    /// An unrotated frame centred at `(x, y, z)` — the whole of what a move
    /// or Scale step changes about a part's `CFrame`.
    fn cframe_at(x: f32, y: f32, z: f32) -> Variant {
        Variant::CFrame(CFrameData {
            position: Vector3Data { x, y, z },
            rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        })
    }

    #[test]
    fn undoing_a_single_property_edit_classifies_as_a_fast_patch_and_restores_the_old_value() {
        let mut dom = WeakDom::new();
        let part = dom.new_instance("Part", "Part", None);
        dom.set_property(part, "Transparency", Variant::Float32(0.0))
            .unwrap();
        dom.take_changes(); // the write above is the part's creation, not the edit under test

        let mut history = History::new(DEFAULT_CAP);
        history.push(dom.clone()); // `Shell::push_history`'s snapshot, before the edit

        dom.set_property(part, "Transparency", Variant::Float32(0.5))
            .unwrap();
        history.record_changes(dom.take_changes()); // `Shell::record_history_change`, right after it

        let (previous, changes) = history.undo(dom.clone()).expect("something to undo");
        assert_eq!(
            single_instance_change(&changes),
            Some((part, names(&["Transparency"]))),
            "a lone property write must classify as the fast path"
        );
        assert_eq!(
            previous.get(part).unwrap().properties().get("Transparency"),
            Some(&Variant::Float32(0.0)),
            "undo must actually restore the old value, not just classify the edit"
        );

        let (next, changes) = history.redo(previous).expect("something to redo");
        assert_eq!(
            single_instance_change(&changes),
            Some((part, names(&["Transparency"]))),
            "redo reapplies the same single edit, so it classifies the same way"
        );
        assert_eq!(
            next.get(part).unwrap().properties().get("Transparency"),
            Some(&Variant::Float32(0.5)),
            "redo must actually reapply the new value"
        );
    }

    #[test]
    fn undoing_a_reparent_classifies_as_a_fast_patch_and_restores_the_old_parent() {
        let mut dom = WeakDom::new();
        let a = dom.new_instance("Model", "A", None);
        let b = dom.new_instance("Model", "B", None);
        let part = dom.new_instance("Part", "Part", Some(a));
        dom.take_changes();

        let mut history = History::new(DEFAULT_CAP);
        history.push(dom.clone());

        dom.set_parent(part, Some(b));
        history.record_changes(dom.take_changes());

        let (previous, changes) = history.undo(dom.clone()).expect("something to undo");
        assert_eq!(
            single_instance_change(&changes),
            Some((part, names(&["Parent"])))
        );
        assert_eq!(
            previous.parent(part),
            Some(a),
            "undo must restore the old parent"
        );
    }

    #[test]
    fn undoing_an_instance_delete_falls_back_and_restores_the_whole_subtree() {
        let mut dom = WeakDom::new();
        let model = dom.new_instance("Model", "Model", None);
        let child = dom.new_instance("Part", "Child", Some(model));
        dom.take_changes();

        let mut history = History::new(DEFAULT_CAP);
        history.push(dom.clone()); // `Shell::push_history`, before the delete

        dom.remove(model); // `WeakDom::remove` logs one `Change::Removed` per instance in the subtree
        history.record_changes(dom.take_changes());

        let (previous, changes) = history.undo(dom.clone()).expect("something to undo");
        assert_eq!(
            single_instance_change(&changes),
            None,
            "a subtree delete is never one instance's property writes"
        );
        assert!(
            previous.get(model).is_some() && previous.get(child).is_some(),
            "undo must restore the whole removed subtree, not just its root"
        );
    }

    #[test]
    fn undoing_an_instance_create_falls_back() {
        let mut dom = WeakDom::new();
        dom.take_changes();

        let mut history = History::new(DEFAULT_CAP);
        history.push(dom.clone());

        let part = dom.new_instance("Part", "Part", None);
        history.record_changes(dom.take_changes());

        let (previous, changes) = history.undo(dom.clone()).expect("something to undo");
        assert_eq!(
            single_instance_change(&changes),
            None,
            "an insert is never one instance's property writes"
        );
        assert!(
            previous.get(part).is_none(),
            "undo must remove the instance the insert created"
        );
    }

    #[test]
    fn undoing_a_multi_property_edit_on_one_instance_is_a_fast_patch_per_property() {
        let mut dom = WeakDom::new();
        let part = dom.new_instance("Part", "Part", None);
        dom.set_property(part, "Transparency", Variant::Float32(0.0))
            .unwrap();
        dom.set_property(part, "Reflectance", Variant::Float32(0.0))
            .unwrap();
        dom.take_changes();

        let mut history = History::new(DEFAULT_CAP);
        history.push(dom.clone());

        // The kind of batch a Command Bar script (or, here, anything that
        // writes more than one property in one go) produces on one part.
        dom.set_property(part, "Transparency", Variant::Float32(0.5))
            .unwrap();
        dom.set_property(part, "Reflectance", Variant::Float32(0.3))
            .unwrap();
        history.record_changes(dom.take_changes());

        let (previous, changes) = history.undo(dom.clone()).expect("something to undo");
        assert_eq!(
            single_instance_change(&changes),
            Some((part, names(&["Transparency", "Reflectance"]))),
            "two writes on one instance are two in-place patches, not a reload"
        );
        let properties = previous.get(part).unwrap().properties();
        assert_eq!(properties.get("Transparency"), Some(&Variant::Float32(0.0)));
        assert_eq!(properties.get("Reflectance"), Some(&Variant::Float32(0.0)));
    }

    #[test]
    fn undoing_a_scale_drag_is_a_fast_patch_for_both_properties_it_wrote() {
        // What one Scale step logs (see `shell::drag::resize_part`): `size`
        // and `CFrame` together, on the one part, because the face opposite
        // the grabbed one holds still and the centre moves by half the
        // growth. Undoing it has to put both back, and a scene rebuild is
        // the wrong price for that — on a large place it is what made undo
        // look broken.
        let mut dom = WeakDom::new();
        let part = dom.new_instance("Part", "Part", None);
        dom.set_property(part, "size", vector3(4.0, 1.0, 2.0))
            .unwrap();
        dom.set_property(part, "CFrame", cframe_at(0.0, 0.0, 0.0))
            .unwrap();
        dom.take_changes();

        let mut history = History::new(DEFAULT_CAP);
        history.push(dom.clone()); // the drag's `first` step

        dom.set_property(part, "size", vector3(6.0, 1.0, 2.0))
            .unwrap();
        dom.set_property(part, "CFrame", cframe_at(1.0, 0.0, 0.0))
            .unwrap();
        history.record_changes(dom.take_changes());

        let (previous, changes) = history.undo(dom.clone()).expect("something to undo");
        assert_eq!(
            single_instance_change(&changes),
            Some((part, names(&["size", "CFrame"]))),
            "a Scale step is two patches on one part, never a reload"
        );
        let properties = previous.get(part).unwrap().properties();
        assert_eq!(properties.get("size"), Some(&vector3(4.0, 1.0, 2.0)));
        assert_eq!(properties.get("CFrame"), Some(&cframe_at(0.0, 0.0, 0.0)));

        let (next, changes) = history.redo(previous).expect("something to redo");
        assert_eq!(
            single_instance_change(&changes),
            Some((part, names(&["size", "CFrame"]))),
            "redo reapplies the same step, so it classifies the same way"
        );
        let properties = next.get(part).unwrap().properties();
        assert_eq!(properties.get("size"), Some(&vector3(6.0, 1.0, 2.0)));
        assert_eq!(properties.get("CFrame"), Some(&cframe_at(1.0, 0.0, 0.0)));
    }

    #[test]
    fn undoing_a_group_drag_falls_back_and_restores_every_part() {
        // `shell::drag::move_parts` writes one `CFrame` per part carried, so
        // a group drag's log names two instances — the fallback that stays
        // deliberate (see that method's doc comment).
        let mut dom = WeakDom::new();
        let a = dom.new_instance("Part", "A", None);
        let b = dom.new_instance("Part", "B", None);
        dom.set_property(a, "CFrame", cframe_at(0.0, 0.0, 0.0))
            .unwrap();
        dom.set_property(b, "CFrame", cframe_at(2.0, 0.0, 0.0))
            .unwrap();
        dom.take_changes();

        let mut history = History::new(DEFAULT_CAP);
        history.push(dom.clone());

        dom.set_property(a, "CFrame", cframe_at(1.0, 0.0, 0.0))
            .unwrap();
        dom.set_property(b, "CFrame", cframe_at(3.0, 0.0, 0.0))
            .unwrap();
        history.record_changes(dom.take_changes());

        let (previous, changes) = history.undo(dom.clone()).expect("something to undo");
        assert_eq!(
            single_instance_change(&changes),
            None,
            "two parts moved is wider than one instance"
        );
        assert_eq!(
            previous.get(a).unwrap().properties().get("CFrame"),
            Some(&cframe_at(0.0, 0.0, 0.0))
        );
        assert_eq!(
            previous.get(b).unwrap().properties().get("CFrame"),
            Some(&cframe_at(2.0, 0.0, 0.0))
        );
    }

    #[test]
    fn undoing_a_script_source_edit_classifies_as_a_fast_patch_and_re_seeds_its_tab() {
        // Both halves of what an undo owes an open script tab, on the one
        // return value `History::undo` now hands back: the `Change` log
        // decides the viewport takes the in-place patch rather than a full
        // reload, and the DOM beside it is what `Shell::resync_scripts`
        // compares an open tab's last-synced text against. A tab is re-seeded
        // exactly when that comparison fails.
        let mut dom = WeakDom::new();
        let script = dom.new_instance("Script", "Greeter", None);
        dom.set_property(
            script,
            source::SOURCE_PROPERTY,
            Variant::String("print(1)\n".into()),
        )
        .unwrap();

        let mut history = History::new(DEFAULT_CAP);
        // `shell::scripts::commit_script`, in the order it runs.
        dom.take_changes();
        history.push(dom.clone());
        assert!(source::write(&mut dom, script, "print(2)\n"));
        history.record_changes(dom.take_changes());

        // What the tab holds on screen once its own debounced write landed.
        let synced = "print(2)\n";
        assert!(source::is(&dom, script, synced), "nothing to re-seed yet");

        let (previous, changes) = history.undo(dom.clone()).expect("the edit to undo");
        assert_eq!(
            single_instance_change(&changes),
            Some((script, names(&[source::SOURCE_PROPERTY]))),
            "a lone `Source` write is a single edit, so undoing it must take the fast patch"
        );
        assert!(
            !source::is(&previous, script, synced),
            "the undone DOM must read to the tab as the mismatch it re-seeds on"
        );
        assert_eq!(
            source::read(&previous, script).as_deref(),
            Some("print(1)\n"),
            "and re-seeding must put the pre-edit source back, not something else"
        );

        let (next, changes) = history.redo(previous).expect("the edit to redo");
        assert_eq!(
            single_instance_change(&changes),
            Some((script, names(&[source::SOURCE_PROPERTY]))),
            "redo reapplies the same single write, so it classifies the same way"
        );
        assert!(
            source::is(&next, script, synced),
            "redo puts the tab back in agreement with the DOM, so nothing re-seeds"
        );
    }
}
