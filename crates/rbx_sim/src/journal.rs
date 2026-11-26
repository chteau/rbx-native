//! Following a DOM's edits across drained batches and the live log, for an
//! index that only wants to re-read what changed since it last looked.

use rbx_dom::{Change, WeakDom};

/// Which mutations (numbered as `WeakDom::mutations` counts them) were seen
/// since the last [`Journal::reset`].
#[derive(Default)]
pub(crate) struct Journal {
    /// The last mutation seen without a gap.
    covered: u64,
    /// Some mutation went by unseen.
    gap: bool,
    started: bool,
}

impl Journal {
    /// The part of `changes` (numbered from `first`) not seen yet.
    pub(crate) fn fresh<'a>(&mut self, first: u64, changes: &'a [Change]) -> &'a [Change] {
        if !self.started {
            return &[];
        }
        if first > self.covered + 1 {
            self.gap = true;
        }
        let skip = (self.covered + 1).saturating_sub(first) as usize;
        self.covered = self.covered.max(first + changes.len() as u64 - 1);
        &changes[skip.min(changes.len())..]
    }

    /// The part of `dom`'s live log not seen yet.
    pub(crate) fn log<'a>(&mut self, dom: &'a WeakDom) -> &'a [Change] {
        let log = dom.changes();
        self.fresh(dom.mutations() + 1 - log.len() as u64, log)
    }

    /// Whether every mutation since the last reset was seen.
    pub(crate) fn whole(&self) -> bool {
        self.started && !self.gap
    }

    /// [`Journal::whole`], then starts over
    /// from `dom` as it is now.
    pub(crate) fn reset(&mut self, dom: &WeakDom) -> bool {
        let whole = self.whole();
        self.covered = dom.mutations();
        self.gap = false;
        self.started = true;
        whole
    }
}
