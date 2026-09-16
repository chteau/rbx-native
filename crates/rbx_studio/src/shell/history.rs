//! Ctrl+Z / Ctrl+Y / Ctrl+Shift+Z, dispatched from the same window-level
//! `Shell::handle_shell_key` Ctrl+S already uses (see `shell::save`) rather
//! than a second `on_key_down` — a key event bubbles up from whatever holds
//! focus, and undo must go through no matter which panel is focused.
//!
//! Undo/redo takes the same fast in-place viewport patch an ordinary edit
//! does (`shell::edit::reflect_in_viewport`) whenever the reverted/reapplied
//! mutation was exactly one property write or reparent, falling back to a
//! full [`Shell::reload_viewport`] only for anything wider — an instance
//! create or delete, a multi-instance drag, a script that touched more than
//! one thing. `crate::history::History` records the `Change` log each
//! pushed snapshot's mutation produced (see `Shell::push_history`/
//! `Shell::record_history_change`, called from every mutating call site);
//! [`Shell::install`] below reads it through `shell::command::single_change`
//! — the exact classifier a script run's own viewport reflection already
//! uses — rather than a second one built for this.
//!
//! `RBX_STUDIO_UNDO=1` applies one undo once, right after startup, through
//! this exact path — a debugging aid for a screenshot that proves a mutation
//! was reverted, since nothing else can send a keystroke to the window on the
//! editor's behalf (see `AGENTS.md`'s safety rules).

use gpui_kit::Context;
use rbx_dom::{Change, WeakDom};

use crate::history;

use super::command::single_change;
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
        self.history.push(self.dom.clone());
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
        if let Some((previous, changes)) = self.history.undo(self.dom.clone()) {
            self.install(previous, &changes, cx);
        }
    }

    /// Ctrl+Y / Ctrl+Shift+Z: symmetric to [`Shell::undo`]. A no-op with
    /// nothing to redo. `pub(crate)` for the same reason as `undo` above.
    pub(crate) fn redo(&mut self, cx: &mut Context<Self>) {
        if let Some((next, changes)) = self.history.redo(self.dom.clone()) {
            self.install(next, &changes, cx);
        }
    }

    /// Installs `dom` as the canonical tree and reflects it in the viewport
    /// — the fast patch `single_change` classifies `changes` as, or a full
    /// [`Shell::reload_viewport`] for anything it can't (see this module's
    /// doc comment) — plus the Explorer and the selection: cleared when its
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
        match single_change(changes) {
            Some((reference, name)) => self.reflect_in_viewport(reference, &name, cx),
            None => self.reload_viewport(cx),
        }
        cx.notify();
    }

    /// The window-level `on_key_down` handler's undo/redo half; called from
    /// `Shell::handle_shell_key` alongside `shell::save`'s own check.
    pub(super) fn handle_history_key(
        &mut self,
        keystroke: &gpui_kit::Keystroke,
        cx: &mut Context<Self>,
    ) {
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
