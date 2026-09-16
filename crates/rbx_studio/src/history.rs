//! Undo/redo: a bounded stack of whole-DOM snapshots, taken right before each
//! mutation (see `shell::history`, which calls [`History::push`] at every
//! call site `shell::command`, `shell::edit` and `shell::keys` already use to
//! take `Shell::dom` out, mutate it, and put it back), each paired with the
//! `Change` log that mutation went on to produce (see [`History::record_changes`]).
//!
//! Keeping a full [`WeakDom`] clone per entry rather than a diff is the same
//! trade `shell::command` already makes for a script run: places are small
//! enough in practice that a clone is cheap, and a diff format would have to
//! track every mutation path (script, property edit, insert, delete)
//! separately instead of once, here. The `Change` log carried alongside each
//! snapshot is that same log, not a second one: every mutation path already
//! writes to `WeakDom`'s own change log (`shell::command`'s `reflect_changes`
//! hands it to the viewport), so recording it here costs nothing beyond
//! draining it at the right two moments.

use gpui_kit::Modifiers;
use rbx_dom::{Change, WeakDom};

/// How many undo steps are kept before the oldest is dropped — bounds memory
/// rather than growing the stack for the length of a whole editing session.
pub(crate) const DEFAULT_CAP: usize = 50;

/// What Ctrl+Z / Ctrl+Y / Ctrl+Shift+Z do — the window-level shortcuts this
/// module owns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Action {
    Undo,
    Redo,
}

/// Maps one keystroke to an undo/redo action. Mirrors `save::action_for`'s
/// pure-function shape: the mapping is testable without a window, and
/// `shell::history` only has to call it. Both `Ctrl+Y` and `Ctrl+Shift+Z` redo
/// — Studio uses the former, most other editors the latter.
pub(crate) fn action_for(key: &str, modifiers: Modifiers) -> Option<Action> {
    match key {
        "z" if modifiers.control && modifiers.shift => Some(Action::Redo),
        "z" if modifiers.control => Some(Action::Undo),
        "y" if modifiers.control => Some(Action::Redo),
        _ => None,
    }
}

/// One snapshot on either stack: the DOM as it stood at that point, plus the
/// `Change` log the mutation right after it went on to produce. Handed by
/// `shell::history::install` to `shell::command`'s `reflect_changes` — the
/// same path an ordinary edit's viewport reflection takes — so the viewport
/// patches exactly the instances the step touched, without re-diffing two
/// `WeakDom` trees to find them.
///
/// `changes` starts empty and is filled in once, after the fact, by
/// [`History::record_changes`] — `push` runs *before* the mutation it
/// snapshots, so the log it produces isn't known yet. An entry whose
/// mutation never actually changed anything (a Properties-panel commit that
/// failed validation, say) simply keeps the empty log `push` left it with,
/// and reflecting an empty log is a no-op.
struct Entry {
    dom: WeakDom,
    changes: Vec<Change>,
}

/// A bounded stack of DOM snapshots either side of the current state: `undo`
/// holds what came before, `redo` holds what an undo just stepped back from.
pub(crate) struct History {
    undo: Vec<Entry>,
    redo: Vec<Entry>,
    cap: usize,
}

impl History {
    pub(crate) fn new(cap: usize) -> Self {
        History {
            undo: Vec::new(),
            redo: Vec::new(),
            cap,
        }
    }

    /// Snapshots `dom` onto the undo stack right before a mutation is
    /// applied to it, and clears the redo stack: a new action invalidates
    /// whatever could have been redone. Drops the oldest snapshot once the
    /// stack exceeds `cap`, so the stack never grows past it.
    pub(crate) fn push(&mut self, dom: WeakDom) {
        if self.undo.len() >= self.cap {
            self.undo.remove(0);
        }
        self.undo.push(Entry {
            dom,
            changes: Vec::new(),
        });
        self.redo.clear();
    }

    /// Attaches `changes` to the entry `push` most recently added — see
    /// [`Entry`]'s doc comment. A no-op if nothing has been pushed yet
    /// (should not happen given `Shell::push_history`'s own call sites, but
    /// costs nothing to guard).
    pub(crate) fn record_changes(&mut self, changes: Vec<Change>) {
        if let Some(entry) = self.undo.last_mut() {
            entry.changes = changes;
        }
    }

    /// Pops the last snapshot, pushes `current` onto the redo stack — paired
    /// with the same `Change` log, since redoing this step reapplies exactly
    /// the mutation undoing it just reverted — and returns the popped DOM
    /// plus that log, for the caller to classify and install. `None` with
    /// nothing to undo — a no-op that leaves both stacks untouched.
    pub(crate) fn undo(&mut self, current: WeakDom) -> Option<(WeakDom, Vec<Change>)> {
        let previous = self.undo.pop()?;
        self.redo.push(Entry {
            dom: current,
            changes: previous.changes.clone(),
        });
        Some((previous.dom, previous.changes))
    }

    /// Symmetric to [`History::undo`]: pops the last undone snapshot, pushes
    /// `current` back onto the undo stack (with the same log, for the same
    /// reason `undo` carries it onto `redo`), and returns the popped DOM
    /// plus that log.
    pub(crate) fn redo(&mut self, current: WeakDom) -> Option<(WeakDom, Vec<Change>)> {
        let next = self.redo.pop()?;
        self.undo.push(Entry {
            dom: current,
            changes: next.changes.clone(),
        });
        Some((next.dom, next.changes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(name: &str) -> WeakDom {
        let mut dom = WeakDom::new();
        dom.new_instance("Part", name, None);
        dom
    }

    fn root_name(dom: &WeakDom) -> String {
        dom.root_refs()
            .first()
            .and_then(|r| dom.get(*r))
            .map(|instance| instance.name().to_string())
            .expect("a root instance")
    }

    #[test]
    fn undo_then_redo_returns_to_the_same_state() {
        let mut history = History::new(DEFAULT_CAP);
        let before = named("Before");
        let after = named("After");

        history.push(before);
        let (undone, _) = history.undo(after.clone()).expect("something to undo");
        assert_eq!(root_name(&undone), "Before");

        let (redone, _) = history.redo(undone).expect("something to redo");
        assert_eq!(root_name(&redone), "After");
    }

    #[test]
    fn pushing_past_cap_drops_the_oldest_and_does_not_grow_unbounded() {
        let mut history = History::new(2);
        history.push(named("One"));
        history.push(named("Two"));
        history.push(named("Three"));

        assert_eq!(history.undo.len(), 2);
        let (top, _) = history.undo(named("Current")).expect("something to undo");
        assert_eq!(root_name(&top), "Three");
        let (next, _) = history
            .undo(named("Current"))
            .expect("still something to undo");
        assert_eq!(
            root_name(&next),
            "Two",
            "the oldest entry (\"One\") was evicted"
        );
    }

    #[test]
    fn a_new_push_clears_the_redo_stack() {
        let mut history = History::new(DEFAULT_CAP);
        history.push(named("Before"));
        let (undone, _) = history.undo(named("After")).expect("something to undo");
        assert!(!history.redo.is_empty());

        history.push(undone);
        assert!(
            history.redo.is_empty(),
            "a new action must invalidate any redo history"
        );
    }

    #[test]
    fn a_recorded_change_log_survives_undo_and_carries_onto_redo() {
        let mut history = History::new(DEFAULT_CAP);
        history.push(named("Before"));
        let change = Change::Property {
            referent: rbx_dom::Ref::new(1),
            name: "Transparency".to_string(),
        };
        history.record_changes(vec![change.clone()]);

        let (undone, changes) = history.undo(named("After")).expect("something to undo");
        assert_eq!(
            changes,
            vec![change.clone()],
            "the log recorded before undo travels with the snapshot"
        );

        let (_, changes) = history.redo(undone).expect("something to redo");
        assert_eq!(
            changes,
            vec![change],
            "redo reapplies the same mutation, so it carries the same log"
        );
    }

    #[test]
    fn record_changes_with_nothing_pushed_is_a_no_op() {
        let mut history = History::new(DEFAULT_CAP);
        history.record_changes(vec![Change::Added(rbx_dom::Ref::new(1))]);
        assert!(history.undo.is_empty());
    }

    #[test]
    fn undo_with_an_empty_stack_is_a_no_op() {
        let mut history = History::new(DEFAULT_CAP);
        assert!(history.undo(named("Current")).is_none());
        assert!(history.undo.is_empty());
        assert!(history.redo.is_empty());
    }

    #[test]
    fn redo_with_an_empty_stack_is_a_no_op() {
        let mut history = History::new(DEFAULT_CAP);
        assert!(history.redo(named("Current")).is_none());
    }

    #[test]
    fn ctrl_z_undoes() {
        let modifiers = Modifiers {
            control: true,
            ..Modifiers::none()
        };
        assert_eq!(action_for("z", modifiers), Some(Action::Undo));
    }

    #[test]
    fn ctrl_y_and_ctrl_shift_z_both_redo() {
        let ctrl_y = Modifiers {
            control: true,
            ..Modifiers::none()
        };
        assert_eq!(action_for("y", ctrl_y), Some(Action::Redo));

        let ctrl_shift_z = Modifiers {
            control: true,
            shift: true,
            ..Modifiers::none()
        };
        assert_eq!(action_for("z", ctrl_shift_z), Some(Action::Redo));
    }

    #[test]
    fn z_without_control_does_nothing() {
        assert_eq!(action_for("z", Modifiers::none()), None);
    }
}
