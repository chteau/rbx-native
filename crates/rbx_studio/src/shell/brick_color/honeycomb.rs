//! Where each colour sits in Studio's BrickColor picker, and how the arrow
//! keys move between them.
//!
//! The arrangement is read off Roblox's own screenshot of the picker
//! (creator-docs, `assets/studio/general/Toolbar-Color-Picker.png`) against
//! the palette order `BrickColor.palette` documents: sampled cell by cell,
//! palette 0–126 fill a hexagon of pointy-topped cells row by row, 7 wide at
//! the top, 13 across the middle and 7 again at the bottom; palette 127 opens
//! a separate row of 12 beneath it, whose other 11 cells repeat greys from
//! the hexagon.

/// The hexagon's rows, top to bottom.
const ROWS: [usize; 13] = [7, 8, 9, 10, 11, 12, 13, 12, 11, 10, 9, 8, 7];
/// The widest row, which every other is centred on.
const WIDEST: usize = 13;
/// The row beneath the hexagon, after palette 127, in the screenshot's
/// order: greys the hexagon already holds.
const GREYS: [u8; 11] = [122, 123, 108, 49, 97, 3, 10, 29, 50, 75, 86];

/// One cell: the palette entry it picks, and where it sits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Cell {
    pub(super) palette: u8,
    pub(super) row: usize,
    /// The centre across, in cell widths from the widest row's left edge.
    pub(super) x: f32,
}

/// Every cell, in reading order.
pub(super) fn cells() -> Vec<Cell> {
    let mut cells = Vec::with_capacity(139);
    let mut palette = 0u8;
    for (row, &width) in ROWS.iter().enumerate() {
        let left = (WIDEST - width) as f32 / 2.0;
        for column in 0..width {
            cells.push(Cell {
                palette,
                row,
                x: left + column as f32 + 0.5,
            });
            palette += 1;
        }
    }
    // Half a cell in, as the screenshot draws it.
    for (column, palette) in [127].into_iter().chain(GREYS).enumerate() {
        cells.push(Cell {
            palette,
            row: ROWS.len(),
            x: column as f32 + 1.0,
        });
    }
    cells
}

/// A move the keyboard asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Step {
    Left,
    Right,
    Up,
    Down,
    /// The row's first or last cell.
    RowStart,
    RowEnd,
    /// The picker's first or last cell.
    First,
    Last,
}

/// Where `step` lands from cell `from`: along the row, or to the nearest
/// cell in the row above or below — the left one of the two a honeycomb
/// offers when they are equally near. An edge stays put.
pub(super) fn step(cells: &[Cell], from: usize, step: Step) -> usize {
    let Some(here) = cells.get(from) else {
        return 0;
    };
    let in_row = |row: usize| (0..cells.len()).filter(move |&index| cells[index].row == row);
    match step {
        Step::Left => in_row(here.row).rfind(|&index| index < from),
        Step::Right => in_row(here.row).find(|&index| index > from),
        Step::Up | Step::Down => {
            let row = match step {
                Step::Up => here.row.checked_sub(1),
                _ => Some(here.row + 1),
            };
            row.and_then(|row| {
                in_row(row).min_by(|&a, &b| {
                    let distance = |index: usize| (cells[index].x - here.x).abs();
                    distance(a).total_cmp(&distance(b))
                })
            })
        }
        Step::RowStart => in_row(here.row).next(),
        Step::RowEnd => in_row(here.row).next_back(),
        Step::First => Some(0),
        Step::Last => cells.len().checked_sub(1),
    }
    .unwrap_or(from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hexagon_holds_the_palette_in_order_and_the_last_row_its_last_entry() {
        let cells = cells();
        assert_eq!(cells.len(), 127 + 12);
        let hexagon: Vec<u8> = cells[..127].iter().map(|cell| cell.palette).collect();
        assert_eq!(hexagon, (0..127).collect::<Vec<u8>>());
        assert_eq!(cells[127].palette, 127);
        // Every palette entry is somewhere.
        for palette in 0..128u8 {
            assert!(cells.iter().any(|cell| cell.palette == palette));
        }
    }

    #[test]
    fn rows_are_offset_by_half_a_cell() {
        let cells = cells();
        // Top row: 7 cells centred on 13, so starting 3 cells in.
        assert_eq!(cells[0].x, 3.5);
        // Second row: 8 cells, half a cell further out.
        assert_eq!(cells[7].x, 3.0);
        // The middle row fills the width.
        let middle: Vec<&Cell> = cells.iter().filter(|cell| cell.row == 6).collect();
        assert_eq!(middle.len(), 13);
        assert_eq!(middle[0].x, 0.5);
    }

    #[test]
    fn arrows_follow_the_layout() {
        let cells = cells();
        assert_eq!(step(&cells, 0, Step::Right), 1);
        assert_eq!(step(&cells, 0, Step::Left), 0);
        assert_eq!(step(&cells, 0, Step::Up), 0);
        // Down from 3.5 to the two cells at 3.0 and 4.0: the left one.
        assert_eq!(step(&cells, 0, Step::Down), 7);
        assert_eq!(step(&cells, 6, Step::Down), 13);
        assert_eq!(step(&cells, 7, Step::Up), 0);
        assert_eq!(step(&cells, 3, Step::RowEnd), 6);
        assert_eq!(step(&cells, 3, Step::RowStart), 0);
        assert_eq!(step(&cells, 50, Step::First), 0);
        assert_eq!(step(&cells, 50, Step::Last), 138);
        // From the hexagon's last row into the greys below it.
        assert_eq!(cells[step(&cells, 120, Step::Down)].row, 13);
        assert_eq!(step(&cells, 138, Step::Down), 138);
    }
}
