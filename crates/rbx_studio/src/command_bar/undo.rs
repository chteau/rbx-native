//! Which undo step a Command Bar run's changes belong to.
//!
//! A run is one undo step even when it yields across many frames, so every
//! frame's change log is added to one accumulated log recorded on the entry
//! the run pushed. Anything else that pushes an entry meanwhile (a Properties
//! edit, a second command, an undo) ends that step: the run's later changes
//! open a fresh entry rather than being filed under, or overwriting, the
//! other action's.

use rbx_dom::Change;

#[derive(Debug)]
pub(crate) struct RunUndo {
    /// `History::revision` right after this step's entry was pushed.
    revision: u64,
    log: Vec<Change>,
}

impl RunUndo {
    pub(crate) fn new(revision: u64) -> Self {
        RunUndo {
            revision,
            log: Vec::new(),
        }
    }

    /// Whether something else touched the history since this step's entry
    /// was pushed, so the run's next changes need an entry of their own.
    pub(crate) fn interrupted(&self, revision: u64) -> bool {
        self.revision != revision
    }

    /// Adds one frame's changes; returns the step's whole log so far.
    pub(crate) fn absorb(&mut self, changes: Vec<Change>) -> &[Change] {
        self.log.extend(changes);
        &self.log
    }
}

#[cfg(test)]
mod tests {
    use rbx_dom::{Change, Ref};

    use super::*;

    fn change(id: u32) -> Change {
        Change::Property {
            referent: Ref::new(id),
            name: "Name".to_string(),
        }
    }

    #[test]
    fn frames_accumulate_into_one_log() {
        let mut undo = RunUndo::new(4);
        undo.absorb(vec![change(1)]);
        assert_eq!(undo.absorb(vec![change(2)]), [change(1), change(2)]);
    }

    #[test]
    fn a_step_is_interrupted_once_the_history_moved() {
        let undo = RunUndo::new(4);
        assert!(!undo.interrupted(4));
        assert!(undo.interrupted(5));
    }
}
