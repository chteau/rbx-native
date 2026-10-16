//! The undo step plugin edits accumulate into.
//!
//! A step opens at the first dispatch that changed the place, from the
//! host's `Outcome::before` (the place before that dispatch's first write),
//! and takes in every later dispatch's changes until it is committed: by a
//! user input (before and after it is handled), by the release ending a
//! press in the 3D view, by `ChangeHistoryService:SetWaypoint`, and by any
//! other history push, undo or redo. So a button click is one step, a press
//! through its release is one step, and edits from timers, `Heartbeat` or
//! change handlers are one step up to the next waypoint or user action —
//! roughly what Studio's waypoints give.

use rbx_dom::{Change, WeakDom};

use crate::history::History;

pub(crate) struct Step {
    before: WeakDom,
    changes: Vec<Change>,
}

/// Folds one dispatch's place changes into the open step, opening one from
/// `before` when none is. An edit with no `before` (a write the host could
/// not see coming) and no open step cannot be undone.
pub(crate) fn extend(step: &mut Option<Step>, before: Option<WeakDom>, changes: &[Change]) {
    if changes.is_empty() {
        return;
    }
    match step {
        Some(step) => step.changes.extend_from_slice(changes),
        None => {
            if let Some(before) = before {
                *step = Some(Step {
                    before,
                    changes: changes.to_vec(),
                });
            }
        }
    }
}

/// Pushes the open step onto `history`; whether there was one.
pub(crate) fn commit(step: &mut Option<Step>, history: &mut History) -> bool {
    let Some(step) = step.take() else {
        return false;
    };
    history.push(step.before);
    history.record_changes(step.changes);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbx_dom::Ref;

    fn named(name: &str) -> WeakDom {
        let mut dom = WeakDom::new();
        dom.new_instance("Part", name, None);
        dom
    }

    fn name(dom: &WeakDom) -> &str {
        dom.get(dom.root_refs()[0]).unwrap().name()
    }

    #[test]
    fn dispatches_accumulate_into_one_step_from_the_first_before() {
        let mut step = None;
        let mut history = History::new(10);
        let a = Change::Added(Ref::new(1));
        let b = Change::Added(Ref::new(2));
        extend(&mut step, None, &[]);
        assert!(step.is_none());
        extend(&mut step, Some(named("first")), std::slice::from_ref(&a));
        extend(&mut step, Some(named("second")), std::slice::from_ref(&b));
        assert!(commit(&mut step, &mut history));
        assert!(!commit(&mut step, &mut history));
        let (before, changes) = history.undo(named("now")).unwrap();
        assert_eq!(name(&before), "first");
        assert_eq!(changes, vec![a, b]);
        assert!(history.undo(named("now")).is_none());
    }

    #[test]
    fn a_change_with_nothing_to_restore_opens_no_step() {
        let mut step = None;
        extend(&mut step, None, &[Change::Added(Ref::new(1))]);
        assert!(step.is_none());
    }

    #[test]
    fn a_dispatch_with_a_copy_but_no_change_opens_no_step() {
        let mut step = None;
        extend(&mut step, Some(named("x")), &[]);
        assert!(step.is_none());
    }
}
