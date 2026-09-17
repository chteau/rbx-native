//! `UITableLayout` placement: the siblings are the rows and *their* children
//! are the cells, so this is the one layout that places grandchildren.
//!
//! Everything below is written in terms of a *major* axis, the one the
//! siblings stack along, and a *minor* axis, the one their cells run along.
//! `MajorAxis.RowMajor` makes the major axis Y; `ColumnMajor` swaps the two.

use super::{offset, ordered, Rect};
use crate::scene::gui::plan::{Node, Table};

/// One rect per sibling, one rect per cell of each sibling (in that sibling's
/// own child order), and the extent the table covers.
pub(super) fn table(nodes: &[Node], table: &Table, parent: &Rect) -> Laid {
    let extent = parent.size();
    let padding = table.padding.against(extent);
    let major = usize::from(table.row_major);
    let minor = 1 - major;

    let order = ordered(nodes, table.by_name);
    // Cells follow the table's own `SortOrder` too. The docs say nothing
    // either way, and a table whose rows sort one way and whose columns sort
    // another would put the same column in different places on each row.
    let cells: Vec<Vec<usize>> = nodes
        .iter()
        .map(|node| ordered(&node.children, table.by_name))
        .collect();

    // "Each cell within a row has the same height, and each cell within a
    // column has the same width" — so a column is as wide as its widest cell
    // and a row as tall as its tallest.
    let columns = order.iter().map(|&i| cells[i].len()).max().unwrap_or(0);
    let mut minors = vec![0.0f32; columns];
    let mut majors = vec![0.0f32; order.len()];
    for (slot, &row) in order.iter().enumerate() {
        for (column, &cell) in cells[row].iter().enumerate() {
            let size = nodes[row].children[cell].size.against(extent);
            minors[column] = minors[column].max(size[minor]);
            majors[slot] = majors[slot].max(size[major]);
        }
        if cells[row].is_empty() {
            // A sibling with no cells of its own still takes up a band, and
            // its own `Size` is the only thing left to measure it by.
            majors[slot] = nodes[row].size.against(extent)[major];
        }
    }

    // `FillEmptySpaceColumns`/`FillEmptySpaceRows`: "the column widths will be
    // approximately equal to the parent's AbsoluteSize.X divided by the number
    // of columns"; the padding between them comes off first.
    fill(
        &mut minors,
        table.fill[minor],
        extent[minor],
        padding[minor],
    );
    fill(
        &mut majors,
        table.fill[major],
        extent[major],
        padding[major],
    );

    let mut content = [0.0; 2];
    content[minor] = run(&minors, padding[minor]);
    content[major] = run(&majors, padding[major]);
    let mut origin = [0.0; 2];
    origin[0] = parent.x + offset(table.horizontal, extent[0], content[0]);
    origin[1] = parent.y + offset(table.vertical_align, extent[1], content[1]);

    let empty = Rect {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    };
    let mut rects = vec![empty; nodes.len()];
    let mut cell_rects: Vec<Vec<Rect>> = nodes
        .iter()
        .map(|node| vec![empty; node.children.len()])
        .collect();

    let mut band = 0.0;
    for (slot, &row) in order.iter().enumerate() {
        // The sibling itself spans every column, so a row's own background
        // shows behind its cells.
        rects[row] = boxed(origin, [0.0, band], [content[minor], majors[slot]], minor);
        let mut at = 0.0;
        for (column, &cell) in cells[row].iter().enumerate() {
            cell_rects[row][cell] =
                boxed(origin, [at, band], [minors[column], majors[slot]], minor);
            at += minors[column] + padding[minor];
        }
        band += majors[slot] + padding[major];
    }

    Laid {
        rects,
        cells: cell_rects,
        size: content,
    }
}

/// What a table comes to: the siblings' rects, each sibling's cells' rects in
/// its own child order, and `AbsoluteContentSize`.
pub(super) struct Laid {
    pub(super) rects: Vec<Rect>,
    pub(super) cells: Vec<Vec<Rect>>,
    pub(super) size: [f32; 2],
}

/// A rect from minor-then-major coordinates, back in x/y.
fn boxed(origin: [f32; 2], at: [f32; 2], size: [f32; 2], minor: usize) -> Rect {
    let mut position = [0.0; 2];
    let mut span = [0.0; 2];
    position[minor] = at[0];
    span[minor] = size[0];
    position[1 - minor] = at[1];
    span[1 - minor] = size[1];
    Rect {
        x: origin[0] + position[0],
        y: origin[1] + position[1],
        width: span[0],
        height: span[1],
    }
}

fn fill(sizes: &mut [f32], enabled: bool, extent: f32, padding: f32) {
    if !enabled || sizes.is_empty() {
        return;
    }
    let count = sizes.len();
    let each = (extent - padding * (count - 1) as f32) / count as f32;
    sizes.fill(each);
}

fn run(sizes: &[f32], padding: f32) -> f32 {
    match sizes.len() {
        0 => 0.0,
        count => sizes.iter().sum::<f32>() + (count - 1) as f32 * padding,
    }
}
