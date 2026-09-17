//! Where every sibling of one container goes: through its layout where it has
//! one, by its own `Position`/`Size` otherwise. The walk that emits them is
//! [`super`]'s; this only answers with rects.

use super::super::plan::{Layout, Node};
use super::{grid, list, place, sizing, table, Rect, TextMeasure};

/// What a layout comes to: one rect per sibling in the tree's own order.
pub(crate) struct Arranged {
    pub(crate) rects: Vec<Rect>,
    /// `UITableLayout` only: where each sibling's own children go, since a
    /// table lays out its cells rather than leaving them to their row.
    pub(super) cells: Option<Vec<Vec<Rect>>>,
}

/// Places `nodes` inside `parent` under `layout`, or by their own `Position`
/// and `Size` where there is none.
pub(crate) fn arrange(
    nodes: &[Node],
    layout: Option<&Layout>,
    parent: &Rect,
    measure: &mut dyn TextMeasure,
) -> Arranged {
    match layout {
        Some(Layout::List(spec)) => Arranged {
            rects: list::stacked(nodes, spec, parent, measure),
            cells: None,
        },
        Some(Layout::Grid(spec)) => Arranged {
            rects: grid::grid(nodes, spec, parent, measure),
            cells: None,
        },
        Some(Layout::Table(spec)) => {
            let laid = table::table(nodes, spec, parent, measure);
            Arranged {
                rects: laid.rects,
                cells: Some(laid.cells),
            }
        }
        None => {
            let rects: Vec<Rect> = nodes
                .iter()
                .map(|node| {
                    place(
                        node.position,
                        sizing::extent(node, parent.size(), measure),
                        node.anchor,
                        parent,
                    )
                })
                .collect();
            Arranged { rects, cells: None }
        }
    }
}

/// Sibling indices in the order a layout walks them: `SortOrder.Name`, or
/// `LayoutOrder` with ties in tree order — stable, so equal orders keep the
/// order they were added to the parent in.
pub(in crate::scene::gui::layout) fn ordered(nodes: &[Node], by_name: bool) -> Vec<usize> {
    let mut order: Vec<usize> = (0..nodes.len()).collect();
    match by_name {
        true => order.sort_by(|&a, &b| nodes[a].name.cmp(&nodes[b].name)),
        false => order.sort_by_key(|&index| nodes[index].layout_order),
    }
    order
}
