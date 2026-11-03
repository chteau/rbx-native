//! Which undo step a Command Bar run's changes belong to.
//!
//! A run is one undo step even when it yields across many frames, so every
//! frame's change log is added to one accumulated log recorded on the entry
//! the run pushed. Anything else that pushes an entry meanwhile (a Properties
//! edit, a second command, an undo) ends that step: the run's later changes
//! open a fresh entry rather than being filed under, or overwriting, the
//! other action's.
//!
//! The entry is only pushed once the run changes something. A run that only
//! reads (a `print`) is no step at all, or Ctrl+Z would revert nothing and
//! the redo stack would be cleared to offer it.

use rbx_dom::{Change, WeakDom};

pub(crate) struct RunUndo {
    /// `History::revision` when `before` was taken, then right after this
    /// step's entry was pushed.
    revision: u64,
    log: Vec<Change>,
    /// The place as it stood before the run, until the entry is pushed.
    before: Option<WeakDom>,
}

impl RunUndo {
    /// A step for a run about to start on `before`, with no entry yet.
    pub(crate) fn pending(before: WeakDom, revision: u64) -> Self {
        RunUndo {
            revision,
            log: Vec::new(),
            before: Some(before),
        }
    }

    /// Whether something else touched the history since `before` was taken
    /// or this step's entry was pushed, so the run's next changes need a
    /// step of their own.
    pub(crate) fn interrupted(&self, revision: u64) -> bool {
        self.revision != revision
    }

    /// The snapshot to push as this step's entry, the first time the run
    /// changes something; `None` once it was.
    pub(crate) fn take_before(&mut self) -> Option<WeakDom> {
        self.before.take()
    }

    /// The entry was pushed, which moved the history to `revision`.
    pub(crate) fn opened(&mut self, revision: u64) {
        self.revision = revision;
    }

    /// Adds one frame's changes; returns the step's whole log so far.
    pub(crate) fn absorb(&mut self, changes: Vec<Change>) -> &[Change] {
        self.log.extend(changes);
        &self.log
    }
}

#[cfg(test)]
mod tests {
    use rbx_dom::Ref;

    use super::*;

    fn change(id: u32) -> Change {
        Change::Property {
            referent: Ref::new(id),
            name: "Name".to_string(),
        }
    }

    #[test]
    fn frames_accumulate_into_one_log() {
        let mut undo = RunUndo::pending(WeakDom::new(), 4);
        undo.absorb(vec![change(1)]);
        assert_eq!(undo.absorb(vec![change(2)]), [change(1), change(2)]);
    }

    #[test]
    fn a_step_is_interrupted_once_the_history_moved() {
        let undo = RunUndo::pending(WeakDom::new(), 4);
        assert!(!undo.interrupted(4));
        assert!(undo.interrupted(5));
    }

    #[test]
    fn the_snapshot_is_handed_over_once() {
        let mut undo = RunUndo::pending(WeakDom::new(), 4);
        assert!(undo.take_before().is_some());
        undo.opened(5);
        assert!(undo.take_before().is_none());
        assert!(!undo.interrupted(5));
    }
}
