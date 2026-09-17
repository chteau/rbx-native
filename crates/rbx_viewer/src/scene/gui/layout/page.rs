//! `UIPageLayout` placement: a row (or column) of full-size pages, scrolled so
//! that the current one fills the container.

use super::arrange::ordered;
use super::Rect;
use crate::scene::gui::plan::{Node, Page};

/// One rect per node in `nodes`'s own order.
///
/// "Positions sibling UI elements as full-size pages in a single row or
/// column" (`UIPageLayout`): every page takes the container's whole box
/// whatever its own `Size` says, and the pages are laid end to end along the
/// fill direction with `Padding` between them. The still frame is the layout
/// at rest, so the run is shifted to put `CurrentPage` exactly over the
/// container and its neighbours just outside — which is what the container's
/// own `ClipsDescendants` then hides.
pub(super) fn pages(nodes: &[Node], page: &Page, parent: &Rect) -> Vec<Rect> {
    let order = ordered(nodes, page.by_name);
    let along = usize::from(page.vertical);
    let extent = parent.size();
    let step = extent[along] + page.padding.0 * extent[along] + page.padding.1;

    // "If no page has been explicitly navigated to, it defaults to the first
    // visible GuiObject sibling in layout order" — slot 0 of the run, since
    // `nodes` holds only the visible siblings to begin with.
    let current = order
        .iter()
        .position(|&index| Some(nodes[index].referent) == page.current)
        .unwrap_or(0);

    let mut rects = vec![*parent; nodes.len()];
    for (slot, &index) in order.iter().enumerate() {
        let shift = (slot as f32 - current as f32) * step;
        match page.vertical {
            true => rects[index].y += shift,
            false => rects[index].x += shift,
        }
    }
    rects
}
