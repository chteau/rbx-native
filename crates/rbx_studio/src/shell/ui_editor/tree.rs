//! Structural edits the canvas makes — an element drawn, a group made or
//! undone, a stroke or a corner added or taken off — each as one undo step
//! with the values that finish it.

use gpui_kit::*;
use rbx_dom::{Change, Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::Shell;
use crate::properties;

/// Property writes, as `properties::edit::commit` text.
pub(super) type Writes = Vec<(Ref, &'static str, String)>;

impl Shell {
    /// `build` adds, moves or removes instances and hands back the writes
    /// that finish the edit and, when it moves the selection, where to. All
    /// of it lands under the one history entry: an insert and its first
    /// values have to undo together, since `History::record_changes` keeps
    /// a single log per entry. The log comes back for a gesture that goes
    /// on writing under the same entry (see `Shell::write_drag_after`).
    pub(super) fn edit_gui_tree(
        &mut self,
        what: &str,
        build: impl FnOnce(&mut WeakDom, &ReflectionDatabase) -> (Writes, Option<Vec<Ref>>),
        cx: &mut Context<Self>,
    ) -> Vec<Change> {
        self.push_history();
        let mut dom = std::mem::replace(&mut self.dom, WeakDom::new());
        let (writes, select) = build(&mut dom, &self.database);
        let written = writes.iter().try_for_each(|(referent, name, text)| {
            properties::edit::commit(&mut dom, &self.database, *referent, name, text).map(|_| ())
        });
        self.dom = dom;
        let changes = self.dom.take_changes();

        self.rebuild_explorer(cx);
        if let Some(select) = select {
            self.reselect(select, cx);
        }
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes.clone());
        if let Err(err) = written {
            self.output.push_warning(&format!("{what}: {err}"));
        }
        cx.notify();
        changes
    }
}
