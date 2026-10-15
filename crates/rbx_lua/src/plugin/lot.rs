//! Parking the plugins' own instances outside the place between
//! dispatches, and turning a dispatch's raw change log into what the editor
//! should see.

use std::collections::HashSet;

use rbx_dom::{Change, Ref, WeakDom};

/// The host's parked instances: every plugin-owned root and its subtree,
/// held as the roots of a DOM of their own with their referents unchanged.
#[derive(Default)]
pub(crate) struct Lot {
    pub(crate) parked: WeakDom,
}

/// What [`Lot::merge`] saw, for [`Lot::split`].
pub(crate) struct Merge {
    /// Every referent that was parked (roots and descendants).
    pub(crate) hidden: HashSet<Ref>,
    /// The place's own roots before the merge.
    pub(crate) place_roots: HashSet<Ref>,
}

impl Lot {
    /// Moves every parked subtree into `dom` as roots (see
    /// `WeakDom::move_subtrees`; nothing is logged). A parked root whose
    /// referent the place already uses stays parked (it cannot happen while
    /// the editor keeps its allocator monotonic).
    pub(crate) fn merge(&mut self, dom: &mut WeakDom) -> Merge {
        todo!()
    }

    /// Parks every root of `dom` that is not in `merge.place_roots` (the
    /// plugin roots merged back, plus whatever a plugin created or
    /// parented to `nil`), then returns `log` rewritten for the editor:
    /// - a change about an instance hidden before (or created during the
    ///   dispatch) and hidden after is dropped;
    /// - an instance visible before and parked now is reported `Removed`,
    ///   itself and every descendant, once, at its first mention;
    /// - an instance hidden before and visible now (a plugin parented it
    ///   into the place) is reported `Added`, itself and every descendant,
    ///   once, at its first mention, and its `Parent` change dropped;
    /// - `Added` for an instance visible now is kept, `Removed` for one
    ///   visible before is kept, any other change is kept only when the
    ///   instance is visible both before (or was added) and after.
    /// Order is otherwise the log's.
    pub(crate) fn split(&mut self, dom: &mut WeakDom, merge: Merge, log: Vec<Change>) -> Vec<Change> {
        todo!()
    }
}

/// Every referent in the subtree under `root` (root included), in `dom`.
pub(crate) fn subtree(dom: &WeakDom, root: Ref) -> Vec<Ref> {
    todo!()
}
