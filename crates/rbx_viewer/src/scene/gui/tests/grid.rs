//! `UIGridLayout`: uniform cells, filled a line at a time from one corner.

use super::*;

/// A grid of 100 px cells 10 px apart under `parent`, aligned top-left so the
/// arithmetic is bare. Returns the referent for further properties.
fn grid(dom: &mut WeakDom, parent: Ref) -> Ref {
    let layout = dom.new_instance("UIGridLayout", "UIGridLayout", Some(parent));
    dom.set_property(layout, "CellSize", udim2(0.0, 100, 0.0, 100))
        .unwrap();
    dom.set_property(layout, "CellPadding", udim2(0.0, 10, 0.0, 10))
        .unwrap();
    dom.set_property(layout, "HorizontalAlignment", Variant::Enum(1))
        .unwrap();
    dom.set_property(layout, "VerticalAlignment", Variant::Enum(1))
        .unwrap();
    layout
}

/// Seven items, so the last line of a three-wide grid is a short one.
fn seven(dom: &mut WeakDom, parent: Ref) {
    for _ in 0..7 {
        // A size the grid is expected to overrule entirely.
        frame(dom, parent, udim2(0.5, 7, 0.5, 7), udim2(0.0, 33, 0.0, 44));
    }
}

fn origins(dom: &WeakDom) -> Vec<[f32; 2]> {
    resolve(&screens(dom), VIEWPORT)
        .iter()
        .map(|element| [element.rect.x, element.rect.y])
        .collect()
}

/// A grid three wide fills left to right, top to bottom.
#[test]
fn max_cells_caps_a_line_and_the_next_item_starts_a_new_one() {
    let (mut dom, gui) = screen_gui();
    let layout = grid(&mut dom, gui);
    dom.set_property(layout, "FillDirectionMaxCells", Variant::Int32(3))
        .unwrap();
    seven(&mut dom, gui);

    assert_eq!(
        origins(&dom),
        [
            [0.0, 0.0],
            [110.0, 0.0],
            [220.0, 0.0],
            [0.0, 110.0],
            [110.0, 110.0],
            [220.0, 110.0],
            [0.0, 220.0],
        ]
    );
}

#[test]
fn every_item_takes_the_cell_size_whatever_its_own_size_says() {
    let (mut dom, gui) = screen_gui();
    let layout = grid(&mut dom, gui);
    dom.set_property(layout, "FillDirectionMaxCells", Variant::Int32(3))
        .unwrap();
    seven(&mut dom, gui);

    let sizes: Vec<[f32; 2]> = resolve(&screens(&dom), VIEWPORT)
        .iter()
        .map(|element| [element.rect.width, element.rect.height])
        .collect();
    assert_eq!(sizes, [[100.0, 100.0]; 7]);
}

/// `StartCorner` mirrors the corner the grid grows out of. The grid is 320 px
/// square either way, so every corner is the top-left one reflected.
fn corner(start: u32) -> Vec<[f32; 2]> {
    let (mut dom, gui) = screen_gui();
    let layout = grid(&mut dom, gui);
    dom.set_property(layout, "FillDirectionMaxCells", Variant::Int32(3))
        .unwrap();
    dom.set_property(layout, "StartCorner", Variant::Enum(start))
        .unwrap();
    seven(&mut dom, gui);
    origins(&dom)
}

#[test]
fn start_corner_top_right_fills_right_to_left() {
    assert_eq!(
        corner(1),
        [
            [220.0, 0.0],
            [110.0, 0.0],
            [0.0, 0.0],
            [220.0, 110.0],
            [110.0, 110.0],
            [0.0, 110.0],
            [220.0, 220.0],
        ]
    );
}

#[test]
fn start_corner_bottom_left_fills_bottom_to_top() {
    assert_eq!(
        corner(2),
        [
            [0.0, 220.0],
            [110.0, 220.0],
            [220.0, 220.0],
            [0.0, 110.0],
            [110.0, 110.0],
            [220.0, 110.0],
            [0.0, 0.0],
        ]
    );
}

#[test]
fn start_corner_bottom_right_fills_both_ways_back() {
    assert_eq!(
        corner(3),
        [
            [220.0, 220.0],
            [110.0, 220.0],
            [0.0, 220.0],
            [220.0, 110.0],
            [110.0, 110.0],
            [0.0, 110.0],
            [220.0, 0.0],
        ]
    );
}

#[test]
fn start_corner_top_left_is_the_default_corner() {
    assert_eq!(
        corner(0),
        [
            [0.0, 0.0],
            [110.0, 0.0],
            [220.0, 0.0],
            [0.0, 110.0],
            [110.0, 110.0],
            [220.0, 110.0],
            [0.0, 220.0],
        ]
    );
}

#[test]
fn max_cells_of_zero_fits_as_many_as_the_parent_holds() {
    let (mut dom, gui) = screen_gui();
    grid(&mut dom, gui);
    seven(&mut dom, gui);

    // 800 px of viewport takes seven 100 px cells 10 px apart (770 px), so
    // nothing wraps at all.
    let row: Vec<f32> = origins(&dom).iter().map(|origin| origin[1]).collect();
    assert_eq!(row, [0.0; 7]);
    assert_eq!(origins(&dom)[6][0], 660.0);
}

#[test]
fn a_vertical_fill_direction_runs_down_a_column_before_starting_the_next() {
    let (mut dom, gui) = screen_gui();
    let layout = grid(&mut dom, gui);
    dom.set_property(layout, "FillDirection", Variant::Enum(1))
        .unwrap();
    dom.set_property(layout, "FillDirectionMaxCells", Variant::Int32(3))
        .unwrap();
    seven(&mut dom, gui);

    assert_eq!(
        origins(&dom),
        [
            [0.0, 0.0],
            [0.0, 110.0],
            [0.0, 220.0],
            [110.0, 0.0],
            [110.0, 110.0],
            [110.0, 220.0],
            [220.0, 0.0],
        ]
    );
}

#[test]
fn alignment_places_the_whole_grid_inside_its_parent() {
    let (mut dom, gui) = screen_gui();
    let layout = grid(&mut dom, gui);
    dom.set_property(layout, "FillDirectionMaxCells", Variant::Int32(3))
        .unwrap();
    dom.set_property(layout, "HorizontalAlignment", Variant::Enum(0))
        .unwrap();
    dom.set_property(layout, "VerticalAlignment", Variant::Enum(2))
        .unwrap();
    seven(&mut dom, gui);

    // A 320 px square grid centred across 800 px and sat on 600 px of floor.
    assert_eq!(origins(&dom)[0], [240.0, 280.0]);
}

#[test]
fn a_grid_sorts_its_items_like_a_list_does() {
    let (mut dom, gui) = screen_gui();
    let layout = grid(&mut dom, gui);
    dom.set_property(layout, "FillDirectionMaxCells", Variant::Int32(3))
        .unwrap();
    dom.set_property(layout, "SortOrder", Variant::Enum(0))
        .unwrap();
    for name in ["c", "a", "b"] {
        let referent = dom.new_instance("Frame", name, Some(gui));
        dom.set_property(referent, "Size", udim2(0.0, 10, 0.0, 10))
            .unwrap();
    }

    // Tree order c, a, b; alphabetical order puts c last.
    assert_eq!(origins(&dom), [[220.0, 0.0], [0.0, 0.0], [110.0, 0.0]]);
}
