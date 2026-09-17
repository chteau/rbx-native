//! `UITableLayout`: the siblings are rows, their children are the cells. It
//! is the only layout that places grandchildren, so the rects asserted here
//! cover two levels at once.

use super::*;

/// A `UITableLayout` under `parent`, aligned top-left with no padding.
fn table(dom: &mut WeakDom, parent: Ref) -> Ref {
    let layout = dom.new_instance("UITableLayout", "UITableLayout", Some(parent));
    dom.set_property(layout, "HorizontalAlignment", Variant::Enum(1))
        .unwrap();
    dom.set_property(layout, "VerticalAlignment", Variant::Enum(1))
        .unwrap();
    dom.set_property(layout, "Padding", udim2(0.0, 0, 0.0, 0))
        .unwrap();
    layout
}

/// One sibling holding cells of the given pixel sizes.
fn row(dom: &mut WeakDom, parent: Ref, cells: &[(i32, i32)]) -> Ref {
    let referent = frame(dom, parent, udim2(0.0, 0, 0.0, 0), udim2(0.0, 0, 0.0, 0));
    for &(width, height) in cells {
        frame(
            dom,
            referent,
            udim2(0.0, 0, 0.0, 0),
            udim2(0.0, width, 0.0, height),
        );
    }
    referent
}

fn rects(dom: &WeakDom) -> Vec<Rect> {
    resolve(&screens(dom), VIEWPORT)
        .iter()
        .map(|element| element.rect)
        .collect()
}

/// Two rows of two cells: a column is as wide as its widest cell, a row as
/// tall as its tallest, and the row itself spans every column.
#[test]
fn a_column_is_as_wide_as_its_widest_cell_and_a_row_as_tall_as_its_tallest() {
    let (mut dom, gui) = screen_gui();
    table(&mut dom, gui);
    row(&mut dom, gui, &[(100, 40), (150, 50)]);
    row(&mut dom, gui, &[(120, 30), (90, 70)]);

    // Columns 120 and 150 wide; rows 50 and 70 tall. Elements come back in
    // tree order: a row, then its own cells.
    assert_eq!(
        rects(&dom),
        [
            Rect {
                x: 0.0,
                y: 0.0,
                width: 270.0,
                height: 50.0,
            },
            Rect {
                x: 0.0,
                y: 0.0,
                width: 120.0,
                height: 50.0,
            },
            Rect {
                x: 120.0,
                y: 0.0,
                width: 150.0,
                height: 50.0,
            },
            Rect {
                x: 0.0,
                y: 50.0,
                width: 270.0,
                height: 70.0,
            },
            Rect {
                x: 0.0,
                y: 50.0,
                width: 120.0,
                height: 70.0,
            },
            Rect {
                x: 120.0,
                y: 50.0,
                width: 150.0,
                height: 70.0,
            },
        ]
    );
}

#[test]
fn fill_empty_space_columns_shares_the_parents_width_between_the_columns() {
    let (mut dom, gui) = screen_gui();
    let layout = table(&mut dom, gui);
    dom.set_property(layout, "FillEmptySpaceColumns", Variant::Bool(true))
        .unwrap();
    row(&mut dom, gui, &[(100, 40), (150, 50)]);
    row(&mut dom, gui, &[(120, 30), (90, 70)]);

    // Two columns across 800 px of viewport; the row heights are untouched.
    let laid = rects(&dom);
    assert_eq!(
        laid.iter().map(|rect| rect.width).collect::<Vec<_>>(),
        [800.0, 400.0, 400.0, 800.0, 400.0, 400.0]
    );
    assert_eq!(laid[1].x, 0.0);
    assert_eq!(laid[2].x, 400.0);
    assert_eq!(laid[1].height, 50.0);
}

#[test]
fn fill_empty_space_rows_shares_the_parents_height_between_the_rows() {
    let (mut dom, gui) = screen_gui();
    let layout = table(&mut dom, gui);
    dom.set_property(layout, "FillEmptySpaceRows", Variant::Bool(true))
        .unwrap();
    row(&mut dom, gui, &[(100, 40), (150, 50)]);
    row(&mut dom, gui, &[(120, 30), (90, 70)]);

    let laid = rects(&dom);
    assert_eq!(
        laid.iter().map(|rect| rect.height).collect::<Vec<_>>(),
        [300.0, 300.0, 300.0, 300.0, 300.0, 300.0]
    );
    assert_eq!(laid[3].y, 300.0);
    // Column widths still come from the cells.
    assert_eq!(laid[1].width, 120.0);
}

#[test]
fn padding_goes_between_the_cells_on_both_axes() {
    let (mut dom, gui) = screen_gui();
    let layout = table(&mut dom, gui);
    dom.set_property(layout, "Padding", udim2(0.0, 20, 0.0, 10))
        .unwrap();
    row(&mut dom, gui, &[(100, 40), (150, 50)]);
    row(&mut dom, gui, &[(120, 30), (90, 70)]);

    let laid = rects(&dom);
    assert_eq!(laid[2].x, 140.0);
    assert_eq!(laid[3].y, 60.0);
    // The row spans both columns and the gap between them.
    assert_eq!(laid[0].width, 290.0);
}

#[test]
fn column_major_makes_the_siblings_columns_instead_of_rows() {
    let (mut dom, gui) = screen_gui();
    let layout = table(&mut dom, gui);
    dom.set_property(layout, "MajorAxis", Variant::Enum(1))
        .unwrap();
    row(&mut dom, gui, &[(100, 40), (150, 50)]);
    row(&mut dom, gui, &[(120, 30), (90, 70)]);

    // The same numbers with the axes swapped: the siblings are now columns
    // 150 and 120 wide, and their cells share rows 40 and 70 tall.
    let laid = rects(&dom);
    assert_eq!(
        laid[0],
        Rect {
            x: 0.0,
            y: 0.0,
            width: 150.0,
            height: 110.0,
        }
    );
    assert_eq!(
        laid[2],
        Rect {
            x: 0.0,
            y: 40.0,
            width: 150.0,
            height: 70.0,
        }
    );
    assert_eq!(laid[3].x, 150.0);
}

#[test]
fn a_ragged_row_leaves_the_missing_cells_empty_rather_than_shifting_them() {
    let (mut dom, gui) = screen_gui();
    table(&mut dom, gui);
    row(&mut dom, gui, &[(100, 40), (150, 50), (60, 20)]);
    row(&mut dom, gui, &[(120, 30)]);

    // The short row still spans the whole table, and its one cell sits in the
    // first column.
    let laid = rects(&dom);
    assert_eq!(laid[4].width, 330.0);
    assert_eq!(laid[5].x, 0.0);
    assert_eq!(laid[5].width, 120.0);
}

#[test]
fn a_table_is_placed_inside_its_parent_by_its_alignment() {
    let (mut dom, gui) = screen_gui();
    let layout = table(&mut dom, gui);
    dom.set_property(layout, "HorizontalAlignment", Variant::Enum(2))
        .unwrap();
    dom.set_property(layout, "VerticalAlignment", Variant::Enum(0))
        .unwrap();
    row(&mut dom, gui, &[(100, 40), (150, 50)]);
    row(&mut dom, gui, &[(120, 30), (90, 70)]);

    // A 270x120 table against the right edge of 800 px, centred on 600 px.
    assert_eq!(rects(&dom)[0].x, 530.0);
    assert_eq!(rects(&dom)[0].y, 240.0);
}
