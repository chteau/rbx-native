//! Which drawn box a renderer record stands for — see [`PartId`].

use rbx_dom::Ref;

/// The identity of one box in the picture: the DOM instance it belongs to
/// and, where that instance is drawn as several boxes, which one of them.
///
/// Every pass that patches a single instance finds its GPU record by this id
/// (`renderer::slots::keyed::Keyed`, `renderer::translucent`,
/// `renderer::shadow::casters`), so an instance drawn as several boxes needs
/// as many distinct ids or the records collide and only the last one can
/// ever be addressed again. A `UnionOperation` whose boolean failed is
/// exactly that case: it is drawn as the additive pieces recovered from its
/// operation tree (see `scene::union`), all of them standing for the one
/// referent the DOM knows.
///
/// A piece's index is its position in the operation tree's own additive
/// order, which is a function of the union's asset bytes alone — never of
/// where the union stands or what colour it is. So moving, recolouring or
/// re-reading a union cannot renumber its pieces, and the record an edit
/// rewrites is the record that leaf already had.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct PartId {
    referent: Ref,
    /// `None` is the instance's own box — every ordinary `BasePart`, and the
    /// (suppressed) box of an instance a mesh or a set of pieces draws for.
    piece: Option<u32>,
}

impl PartId {
    /// The instance's own box.
    pub(crate) fn whole(referent: Ref) -> Self {
        PartId {
            referent,
            piece: None,
        }
    }

    /// The `index`-th recovered piece of a union drawn as its pieces.
    pub(crate) fn piece(referent: Ref, index: u32) -> Self {
        PartId {
            referent,
            piece: Some(index),
        }
    }

    /// The DOM instance this box belongs to: what an edit, a selection, a
    /// click and the Explorer all name, whether the box is the instance's
    /// own or one piece of it.
    pub(crate) fn referent(&self) -> Ref {
        self.referent
    }

    pub(crate) fn piece_index(&self) -> Option<u32> {
        self.piece
    }

    /// Whether this is the instance's own box rather than a piece of it —
    /// the one box per referent that carries its placement (see
    /// [`super::Scene::placements`]) and its share of the scene's extent.
    pub(crate) fn is_whole(&self) -> bool {
        self.piece.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The property every patch depends on: an instance's own box and each of
    // its pieces are distinct keys, and two pieces of the same union never
    // collide however many there are.
    #[test]
    fn a_whole_box_and_every_piece_of_it_are_distinct_ids() {
        let union = Ref::new(7);
        let mut ids = vec![PartId::whole(union)];
        ids.extend((0..64).map(|index| PartId::piece(union, index)));

        let distinct: std::collections::HashSet<PartId> = ids.iter().copied().collect();
        assert_eq!(distinct.len(), ids.len());
        assert!(ids.iter().all(|id| id.referent() == union));
        assert_eq!(ids[0].piece_index(), None);
        assert_eq!(ids[1].piece_index(), Some(0));
    }

    // Two unions' pieces are as distinct as the unions themselves, which is
    // what lets a place copy-paste the same rock and still patch one of them.
    #[test]
    fn the_same_piece_index_under_two_referents_is_two_ids() {
        assert_ne!(PartId::piece(Ref::new(1), 3), PartId::piece(Ref::new(2), 3));
        assert_ne!(PartId::whole(Ref::new(1)), PartId::whole(Ref::new(2)));
    }

    #[test]
    fn only_the_instances_own_box_is_whole() {
        assert!(PartId::whole(Ref::new(1)).is_whole());
        assert!(!PartId::piece(Ref::new(1), 0).is_whole());
    }
}
