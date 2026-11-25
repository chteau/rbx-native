//! `UIGridLayout` placement: uniform cells filled a line at a time.

use super::arrange::ordered;
use super::{offset, sizing, Rect, TextMeasure};
use crate::scene::gui::plan::{Grid, Node};

/// One rect per node in `nodes`'s own order, plus the extent the grid covers.
///
/// Every item takes `CellSize` whatever its own `Size` says, "the actual cell
/// sizes are the same for all cells", and then its own
/// `UIAspectRatioConstraint`/`UISizeConstraint`, centred in the cell whatever
/// its `AnchorPoint` (GuiLayout golden: a square item capped at 60 px sits
/// 7.35 px into a 74.7 px cell with anchors (0, 0), (0.5, 0.5) and (1, 1)
/// alike). The cells themselves, and so the grid's extent, stay `CellSize`.
pub(super) fn grid(
    nodes: &[Node],
    grid: &Grid,
    parent: &Rect,
    _measure: &mut dyn TextMeasure,
) -> Vec<Rect> {
    let extent = parent.size();
    let Cells {
        cell,
        padding,
        per_line,
        content,
    } = cells(nodes.len(), grid, extent);
    let along = usize::from(grid.vertical);
    let across = 1 - along;
    let order = ordered(nodes, grid.by_name);
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
        let [width, height] = sizing::constrained(&nodes[index], cell, extent);
        rects[index] = Rect {
            x: origin[0] + at[0] + (cell[0] - width) * 0.5,
            y: origin[1] + at[1] + (cell[1] - height) * 0.5,
            width,
            height,
        };
    }
    rects
}

/// A grid's cells in a parent of `extent` pixels: their size and spacing,
/// how many make a line, and the extent `count` of them cover, which is what
/// an automatically sized parent grows to (not its items, which may be
/// smaller than their cells).
pub(super) struct Cells {
    cell: [f32; 2],
    padding: [f32; 2],
    per_line: usize,
    pub(super) content: [f32; 2],
}

pub(super) fn cells(count: usize, grid: &Grid, extent: [f32; 2]) -> Cells {
    let cell = grid.cell.against(extent);
    let padding = grid.cell_padding.against(extent);
    let along = usize::from(grid.vertical);
    let across = 1 - along;
    // `FillDirectionMaxCells` of zero means "however many fit", which is the
    // grid's own wrapping rule.
    let per_line = match grid.max_cells {
        0 => fit(extent[along], cell[along], padding[along]),
        limit => limit,
    };
    let mut content = [0.0; 2];
    content[along] = run(count.min(per_line), cell[along], padding[along]);
    content[across] = run(count.div_ceil(per_line), cell[across], padding[across]);
    Cells {
        cell,
        padding,
        per_line,
        content,
    }
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
