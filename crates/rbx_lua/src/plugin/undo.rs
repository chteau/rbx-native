//! The place as it stood before a dispatch's first write to it, so the
//! editor can make an undo step of edits no user input caused (a timer, a
//! `Heartbeat` connection, a selection or change handler).
//!
//! Copying the place before every dispatch would cost a full copy per tick;
//! instead every VM's `Ctx` carries a write hook that copies it at the
//! first write landing in the place (a root the editor's DOM had when the
//! dispatch began). Writes to a plugin's own parked instances never copy.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use rbx_dom::{Ref, WeakDom};

use crate::ctx::WriteHook;
use crate::instance::tree;

#[derive(Default)]
pub(crate) struct UndoPoint {
    /// The place's roots for the running dispatch; `None` between them.
    roots: Option<HashSet<Ref>>,
    before: Option<WeakDom>,
}

impl UndoPoint {
    /// Called once the plugins' own roots are merged in: `roots` are the
    /// place's.
    pub(crate) fn arm(&mut self, roots: HashSet<Ref>) {
        self.roots = Some(roots);
        self.before = None;
    }

    /// Ends the dispatch: the copy, when a write reached the place.
    pub(crate) fn disarm(&mut self) -> Option<WeakDom> {
        self.roots = None;
        self.before.take()
    }

    fn note(&mut self, dom: &WeakDom, referent: Ref) {
        let Some(roots) = &self.roots else {
            return;
        };
        if self.before.is_some() {
            return;
        }
        let mut top = referent;
        while let Some(parent) = tree::parent_of(dom, top) {
            top = parent;
        }
        if !roots.contains(&top) {
            return;
        }
        let mut copy = dom.clone();
        for root in copy.root_refs().to_vec() {
            if !roots.contains(&root) {
                copy.remove(root);
            }
        }
        copy.take_changes();
        self.before = Some(copy);
    }
}

/// The hook every plugin VM's `Ctx` carries.
pub(crate) fn hook(point: Rc<RefCell<UndoPoint>>) -> WriteHook {
    Rc::new(move |dom, referent| {
        if let Ok(mut point) = point.try_borrow_mut() {
            point.note(dom, referent);
        }
    })
}
