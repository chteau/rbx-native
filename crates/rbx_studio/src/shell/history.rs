//! Ctrl+Z / Ctrl+Y / Ctrl+Shift+Z, dispatched from the same window-level
//! `Shell::handle_shell_key` Ctrl+S already uses (see `shell::save`) rather
//! than a second `on_key_down` — a key event bubbles up from whatever holds
//! focus, and undo must go through no matter which panel is focused.
//!
//! `RBX_STUDIO_UNDO=1` applies one undo once, right after startup, through
//! this exact path — a debugging aid for a screenshot that proves a mutation
//! was reverted, since nothing else can send a keystroke to the window on the
//! editor's behalf (see `AGENTS.md`'s safety rules).

use gpui_kit::Context;
use rbx_dom::WeakDom;

use crate::history;

use super::Shell;

/// Read once at startup by `Shell::new`; documented in this module's doc
/// comment.
pub(crate) const UNDO_VARIABLE: &str = "RBX_STUDIO_UNDO";

impl Shell {
    /// Snapshots `self.dom` onto the undo stack. Called right before every
    /// mutation call site — `shell::command`'s script run, `shell::edit`'s
    /// committed property edit, `shell::keys`'s insert and delete — so the
    /// snapshot always reflects the DOM as it stood immediately before that
    /// mutation was applied.
    pub(super) fn push_history(&mut self) {
        self.history.push(self.dom.clone());
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
        if let Some(previous) = self.history.undo(self.dom.clone()) {
            self.install(previous, cx);
        }
    }

    /// Ctrl+Y / Ctrl+Shift+Z: symmetric to [`Shell::undo`]. A no-op with
    /// nothing to redo. `pub(crate)` for the same reason as `undo` above.
    pub(crate) fn redo(&mut self, cx: &mut Context<Self>) {
        self.flush_script_edits(cx);
        if let Some(next) = self.history.redo(self.dom.clone()) {
            self.install(next, cx);
        }
    }

    /// Installs `dom` as the canonical tree and reflects it exactly as a
    /// script mutation would (see `shell::command::rebuild_after_script`),
    /// plus the selection: cleared when its referent no longer resolves in
    /// `dom`, the same rule `shell::keys::selection_after_removal` applies to
    /// a delete.
    fn install(&mut self, dom: WeakDom, cx: &mut Context<Self>) {
        self.dom = dom;
        self.rebuild_explorer(cx);
        match self
            .selected()
            .filter(|reference| self.dom.get(*reference).is_some())
        {
            Some(kept) => self.select(kept, cx),
            None => self.deselect(cx),
        }
        self.reload_viewport(cx);
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
