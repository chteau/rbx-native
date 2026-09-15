//! Undo/redo: a bounded stack of whole-DOM snapshots, taken right before each
//! mutation (see `shell::history`, which calls [`History::push`] at every
//! call site `shell::command`, `shell::edit` and `shell::keys` already use to
//! take `Shell::dom` out, mutate it, and put it back).
//!
//! Keeping a full [`WeakDom`] clone per entry rather than a diff is the same
//! trade `shell::command` already makes for a script run: places are small
//! enough in practice that a clone is cheap, and a diff format would have to
//! track every mutation path (script, property edit, insert, delete)
//! separately instead of once, here.

use gpui_kit::Modifiers;
use rbx_dom::WeakDom;

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

/// A bounded stack of DOM snapshots either side of the current state: `undo`
/// holds what came before, `redo` holds what an undo just stepped back from.
pub(crate) struct History {
    undo: Vec<WeakDom>,
    redo: Vec<WeakDom>,
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
        self.undo.push(dom);
        self.redo.clear();
    }

    /// Pops the last snapshot, pushes `current` onto the redo stack so a
    /// following redo can restore it, and returns the popped snapshot to
    /// install as the new DOM. `None` with nothing to undo — a no-op that
    /// leaves both stacks untouched.
    pub(crate) fn undo(&mut self, current: WeakDom) -> Option<WeakDom> {
        let previous = self.undo.pop()?;
        self.redo.push(current);
        Some(previous)
    }

    /// Symmetric to [`History::undo`]: pops the last undone snapshot, pushes
    /// `current` back onto the undo stack, and returns the popped snapshot.
    pub(crate) fn redo(&mut self, current: WeakDom) -> Option<WeakDom> {
        let next = self.redo.pop()?;
        self.undo.push(current);
        Some(next)
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
        let undone = history.undo(after.clone()).expect("something to undo");
        assert_eq!(root_name(&undone), "Before");

        let redone = history.redo(undone).expect("something to redo");
        assert_eq!(root_name(&redone), "After");
    }

    #[test]
    fn pushing_past_cap_drops_the_oldest_and_does_not_grow_unbounded() {
        let mut history = History::new(2);
        history.push(named("One"));
        history.push(named("Two"));
        history.push(named("Three"));

        assert_eq!(history.undo.len(), 2);
        let top = history.undo(named("Current")).expect("something to undo");
        assert_eq!(root_name(&top), "Three");
        let next = history
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
        let undone = history.undo(named("After")).expect("something to undo");
        assert!(!history.redo.is_empty());

        history.push(undone);
        assert!(
            history.redo.is_empty(),
            "a new action must invalidate any redo history"
        );
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
