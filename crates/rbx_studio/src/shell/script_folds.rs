//! Remembering which folds of a script are collapsed: restoring them when a
//! tab opens, and writing them when the gutter toggles one. The storage and
//! the matching rules live in `script_editor::folds::saved`.

use gpui_kit::Context;
use rbx_dom::Ref;

use crate::script_editor::folds::saved::{self, SavedFold};
use crate::script_editor::graph::saved::{self as layout_saved, LayoutKey};

use super::Shell;

/// How many frames to wait for the editor's first parse before giving up on
/// restoring a script's folds (a script with no blocks has none to restore).
const RESTORE_ATTEMPTS: u8 = 8;

/// One open script's fold bookkeeping.
pub(crate) struct FoldMemory {
    key: LayoutKey,
    /// What the file holds, until the folds have been restored.
    saved: Vec<SavedFold>,
    restored: bool,
    attempts: u8,
    /// The collapsed start lines last restored or written.
    last: Vec<usize>,
}

impl FoldMemory {
    pub(super) fn new(key: LayoutKey) -> Self {
        let saved = saved::load(&key);
        Self {
            last: saved.iter().map(|fold| fold.line).collect(),
            restored: saved.is_empty(),
            key,
            saved,
            attempts: 0,
        }
    }
}

impl Shell {
    /// The fold memory for a script of the edited place.
    pub(super) fn fold_memory(&self, reference: Ref) -> FoldMemory {
        let cloud = crate::home::link_of(&self.path).map(|link| link.place_id);
        FoldMemory::new(layout_saved::key_for(
            &self.path,
            cloud,
            self.edit_dom(),
            reference,
        ))
    }

    /// Brings every open tab's folds and the files in step. Cheap when
    /// nothing changed: two short vectors compared.
    pub(super) fn sync_folds(&mut self, cx: &mut Context<Self>) {
        for reference in self.scripts.open.keys().copied().collect::<Vec<_>>() {
            self.sync_script_folds(reference, cx);
        }
    }

    pub(super) fn sync_script_folds(&mut self, reference: Ref, cx: &mut Context<Self>) {
        let Some(open) = self.scripts.open.get_mut(&reference) else {
            return;
        };
        let state = open.state.clone();
        let memory = &mut open.folds;

        if !memory.restored {
            let foldable = state.read(cx).foldable_lines();
            if foldable.is_empty() {
                // The editor parses on its first paint; look again next frame.
                memory.attempts += 1;
                if memory.attempts >= RESTORE_ATTEMPTS {
                    memory.restored = true;
                } else {
                    cx.notify();
                }
                return;
            }
            let text = state.read(cx).value().to_string();
            let lines = saved::restorable(&memory.saved, &text, &foldable);
            state.update(cx, |state, cx| {
                for line in lines {
                    state.set_folded(line, true, cx);
                }
            });
            memory.restored = true;
        }

        let folded = state.read(cx).folded_lines();
        if folded != memory.last {
            let text = state.read(cx).value().to_string();
            saved::save(&memory.key, &saved::capture(&folded, &text));
            memory.last = folded;
        }
    }
}
