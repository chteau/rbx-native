//! The editor's selection: at most one instance, in DOM terms.

use gpui_kit::component::tree::TreeItem;
use rbx_dom::Ref;

use crate::explorer;

/// Single selection. Kept apart from the tree's own selected row because the
/// tree forgets it whenever its rows are replaced, and because the viewport
/// and Properties panel want a referent, not a row index.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct Selection(Option<Ref>);

impl Selection {
    pub(super) fn new(selected: Option<Ref>) -> Self {
        Selection(selected)
    }

    pub(super) fn get(self) -> Option<Ref> {
        self.0
    }

    /// Replaces the selection, reporting whether anything changed so a caller
    /// redrawing on every tree update can stay quiet when it did not.
    pub(super) fn set(&mut self, selected: Option<Ref>) -> bool {
        let changed = self.0 != selected;
        self.0 = selected;
        changed
    }

    /// The tree's selected row, read back as a referent.
    pub(super) fn of_item(item: Option<&TreeItem>) -> Option<Ref> {
        item.and_then(|item| explorer::item_ref(&item.id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selecting_moves_between_instances_and_back_to_nothing() {
        let a = Ref::new(1);
        let b = Ref::new(2);
        let mut selection = Selection::default();
        assert_eq!(selection.get(), None);

        assert!(selection.set(Some(a)));
        assert_eq!(selection.get(), Some(a));

        assert!(selection.set(Some(b)));
        assert_eq!(selection.get(), Some(b));

        // Re-selecting the same instance is not a change worth a redraw.
        assert!(!selection.set(Some(b)));

        assert!(selection.set(None));
        assert_eq!(selection.get(), None);
        assert!(!selection.set(None));
    }

    #[test]
    fn a_tree_row_reads_back_as_its_referent() {
        let item = TreeItem::new(explorer::item_id(Ref::new(42)), "Baseplate");

        assert_eq!(Selection::of_item(Some(&item)), Some(Ref::new(42)));
        assert_eq!(Selection::of_item(None), None);
        // A row whose id is not a referent (none exist, but the tree does not
        // know that) must never turn into a bogus selection.
        let stray = TreeItem::new("not-a-ref", "?");
        assert_eq!(Selection::of_item(Some(&stray)), None);
    }
}
