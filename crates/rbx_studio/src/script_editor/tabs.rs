//! Which scripts have a tab open, and which of them is in front.
//!
//! Only the bookkeeping: the editor widget behind each tab lives in
//! [`super::ScriptEditor`], keyed by the same referent. Kept apart so the
//! open/focus/close rules — the part with actual behaviour to get wrong — can
//! be exercised without a window.

#[cfg(test)]
mod tests;

use rbx_dom::Ref;

/// What [`Tabs::open`] had to do, so the caller knows whether it still has to
/// build an editor widget for this script or already had one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Opened {
    /// The script had no tab. The caller must seed a new editor from the DOM.
    New,
    /// A tab was already open; it has been brought to the front, and its
    /// editor — including an in-progress, uncommitted edit — is untouched.
    Existing,
}

#[derive(Default)]
pub(crate) struct Tabs {
    /// Open tabs left to right, in the order they were first opened.
    open: Vec<Ref>,
    active: Option<Ref>,
}

impl Tabs {
    /// Opens `reference`, or brings its existing tab to the front. Re-opening
    /// never re-seeds: a script already open keeps whatever is in its editor,
    /// which is what makes double-clicking the Explorer row of a script being
    /// edited a focus action rather than a way to lose the edit.
    pub(crate) fn open(&mut self, reference: Ref) -> Opened {
        self.active = Some(reference);
        if self.open.contains(&reference) {
            return Opened::Existing;
        }
        self.open.push(reference);
        Opened::New
    }

    /// Closes one tab. The tab to its right takes the front, falling back to
    /// the one on its left and then to nothing — closing a tab should leave
    /// the neighbour under the cursor in front, not jump to the far end.
    /// Closing a tab that is not in front leaves the front one alone.
    pub(crate) fn close(&mut self, reference: Ref) {
        let Some(index) = self.open.iter().position(|open| *open == reference) else {
            return;
        };
        self.open.remove(index);
        if self.active != Some(reference) {
            return;
        }
        self.active = self
            .open
            .get(index)
            .or_else(|| self.open.get(index.wrapping_sub(1)))
            .copied();
    }

    /// Brings an already-open tab to the front. A referent with no tab is
    /// ignored rather than opening one: the dock's own tab bar can only ever
    /// activate a tab it is already drawing.
    pub(crate) fn activate(&mut self, reference: Ref) {
        if self.open.contains(&reference) {
            self.active = Some(reference);
        }
    }

    /// Closes every tab whose script `keep` rejects, applying [`Self::close`]'s
    /// front-tab rule to each. Called after anything that can remove an
    /// instance — an undo, a delete, a Command Bar script — so a tab never
    /// outlives the script it was editing.
    pub(crate) fn retain(&mut self, keep: impl Fn(Ref) -> bool) {
        for reference in self.open.clone() {
            if !keep(reference) {
                self.close(reference);
            }
        }
    }

    pub(crate) fn all(&self) -> &[Ref] {
        &self.open
    }

    pub(crate) fn active(&self) -> Option<Ref> {
        self.active
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.open.is_empty()
    }
}
