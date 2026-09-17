//! `UIGridLayout` placement: uniform cells filled a line at a time.

use super::arrange::ordered;
use super::{offset, Rect, TextMeasure};
use crate::scene::gui::plan::{Grid, Node};

/// One rect per node in `nodes`'s own order, plus the extent the grid covers.
///
/// Every item takes `CellSize` whatever its own `Size` says — "the actual cell
/// sizes are the same for all cells". The docs also have a grid respect a
/// `UISizeConstraint`/`UIAspectRatioConstraint` on an item, letting it span
/// several cells; constraints are another module's business, so a cell here is
/// always exactly one cell.
pub(super) fn grid(
    nodes: &[Node],
    grid: &Grid,
    parent: &Rect,
    _measure: &mut dyn TextMeasure,
) -> (Vec<Rect>, [f32; 2]) {
    let extent = parent.size();
    let cell = grid.cell.against(extent);
    let padding = grid.cell_padding.against(extent);
    let along = usize::from(grid.vertical);
    let across = 1 - along;

    let order = ordered(nodes, grid.by_name);
    // `FillDirectionMaxCells` of zero means "however many fit", which is the
    // grid's own wrapping rule.
    let per_line = match grid.max_cells {
        0 => fit(extent[along], cell[along], padding[along]),
        limit => limit,
    };
    let lines = order.len().div_ceil(per_line);

    let mut content = [0.0; 2];
    content[along] = run(order.len().min(per_line), cell[along], padding[along]);
    content[across] = run(lines, cell[across], padding[across]);
    let origin = [
        parent.x + offset(grid.horizontal, extent[0], content[0]),
        parent.y + offset(grid.vertical_align, extent[1], content[1]),
    ];

    let mut rects = vec![
        Rect {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0
        };
        nodes.len()
    ];
    for (slot, &index) in order.iter().enumerate() {
        let mut at = [0.0; 2];
        at[along] = (slot % per_line) as f32 * (cell[along] + padding[along]);
        at[across] = (slot / per_line) as f32 * (cell[across] + padding[across]);
        // `StartCorner` only mirrors the corner the grid grows out of; the
        // fill direction and the order cells are filled in are unchanged.
        for axis in 0..2 {
            if grid.flip[axis] {
                at[axis] = content[axis] - cell[axis] - at[axis];
            }
        }
        rects[index] = Rect {
            x: origin[0] + at[0],
            y: origin[1] + at[1],
            width: cell[0],
            height: cell[1],
        };
    }
    (rects, content)
}

/// How many cells of `cell` pixels, `padding` apart, fit across `extent`.
/// Always at least one: a cell wider than its parent still gets a line.
fn fit(extent: f32, cell: f32, padding: f32) -> usize {
    let stride = cell + padding;
    if stride <= 0.0 {
        return 1;
    }
    (((extent + padding) / stride).floor().max(1.0) as usize).max(1)
}

/// The extent `count` cells cover, gaps between them included.
fn run(count: usize, cell: f32, padding: f32) -> f32 {
    match count {
        0 => 0.0,
        count => count as f32 * cell + (count - 1) as f32 * padding,
    }
}
