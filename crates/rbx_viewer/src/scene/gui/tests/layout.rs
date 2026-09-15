//! Resolution of a planned tree against a viewport: `UDim2` boxes, anchor
//! points, paint order and `ClipsDescendants`.

use super::*;

#[test]
fn a_top_level_frame_resolves_against_the_viewport() {
    let (mut dom, gui) = screen_gui();
    frame(
        &mut dom,
        gui,
        udim2(0.5, -50, 0.5, -50),
        udim2(0.0, 100, 0.0, 100),
    );

    let elements = resolve(&screens(&dom), VIEWPORT);

    assert_eq!(
        elements[0].rect,
        Rect {
            x: 350.0,
            y: 250.0,
            width: 100.0,
            height: 100.0,
        }
    );
}

#[test]
fn a_child_resolves_against_its_parents_pixel_rect_not_the_viewport() {
    let (mut dom, gui) = screen_gui();
    let parent = frame(
        &mut dom,
        gui,
        udim2(0.0, 100, 0.0, 50),
        udim2(0.0, 200, 0.0, 400),
    );
    // Half of the parent, offset a quarter of the way in — none of which lines
    // up with any fraction of the viewport.
    frame(
        &mut dom,
        parent,
        udim2(0.25, 0, 0.0, 10),
        udim2(0.5, 0, 0.5, 0),
    );

    let elements = resolve(&screens(&dom), VIEWPORT);

    assert_eq!(
        elements[1].rect,
        Rect {
            x: 150.0,
            y: 60.0,
            width: 100.0,
            height: 200.0,
        }
    );
}

#[test]
fn an_anchor_point_slides_the_box_back_by_its_own_size() {
    let (mut dom, gui) = screen_gui();
    let centred = frame(
        &mut dom,
        gui,
        udim2(0.5, 0, 0.5, 0),
        udim2(0.0, 200, 0.0, 100),
    );
    dom.set_property(
        centred,
        "AnchorPoint",
        Variant::Vector2(Vector2Data { x: 0.5, y: 0.5 }),
    )
    .unwrap();

    let elements = resolve(&screens(&dom), VIEWPORT);

    // Centre of an 800x600 viewport, less half of 200x100.
    assert_eq!(elements[0].rect.x, 300.0);
    assert_eq!(elements[0].rect.y, 250.0);
}

#[test]
fn siblings_paint_by_z_index_then_by_tree_order() {
    let (mut dom, gui) = screen_gui();
    let zero = udim2(0.0, 0, 0.0, 0);
    // Each frame is tagged by a width the ordering never looks at.
    let first = frame(&mut dom, gui, zero.clone(), udim2(0.0, 1, 0.0, 0));
    let second = frame(&mut dom, gui, zero.clone(), udim2(0.0, 2, 0.0, 0));
    let third = frame(&mut dom, gui, zero.clone(), udim2(0.0, 3, 0.0, 0));
    dom.set_property(first, "ZIndex", Variant::Int32(5))
        .unwrap();
    dom.set_property(second, "ZIndex", Variant::Int32(1))
        .unwrap();
    dom.set_property(third, "ZIndex", Variant::Int32(1))
        .unwrap();

    let elements = resolve(&screens(&dom), VIEWPORT);

    // The two sharing a `ZIndex` keep tree order; the higher one goes last.
    let tags: Vec<f32> = elements.iter().map(|element| element.rect.width).collect();
    assert_eq!(tags, vec![2.0, 3.0, 1.0]);
}

#[test]
fn a_child_always_paints_over_its_parent_whatever_the_parents_z_index() {
    let (mut dom, gui) = screen_gui();
    let zero = udim2(0.0, 0, 0.0, 0);
    let parent = frame(&mut dom, gui, zero.clone(), zero.clone());
    let child = frame(&mut dom, parent, zero.clone(), zero.clone());
    dom.set_property(parent, "ZIndex", Variant::Int32(50))
        .unwrap();
    dom.set_property(child, "ZIndex", Variant::Int32(1))
        .unwrap();

    let elements = resolve(&screens(&dom), VIEWPORT);

    assert_eq!(elements.len(), 2);
    // The child is second, so it is painted last.
    assert!(elements[1].clip.is_none());
}

#[test]
fn a_higher_display_order_paints_over_a_lower_one() {
    let mut dom = WeakDom::new();
    let zero = udim2(0.0, 0, 0.0, 0);
    for (name, order, width) in [("top", 10, 11), ("bottom", -1, 22)] {
        let gui = dom.new_instance("ScreenGui", name, None);
        dom.set_property(gui, "DisplayOrder", Variant::Int32(order))
            .unwrap();
        frame(&mut dom, gui, zero.clone(), udim2(0.0, width, 0.0, 0));
    }

    let elements = resolve(&screens(&dom), VIEWPORT);

    assert_eq!(elements[0].rect.width, 22.0);
    assert_eq!(elements[1].rect.width, 11.0);
}

#[test]
fn clips_descendants_scissors_children_to_the_frame_and_compounds() {
    let (mut dom, gui) = screen_gui();
    let outer = frame(
        &mut dom,
        gui,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 200, 0.0, 200),
    );
    dom.set_property(outer, "ClipsDescendants", Variant::Bool(true))
        .unwrap();
    // Sticks out to x=300, and clips again 50 px narrower than that.
    let inner = frame(
        &mut dom,
        outer,
        udim2(0.0, 100, 0.0, 0),
        udim2(0.0, 200, 0.0, 100),
    );
    dom.set_property(inner, "ClipsDescendants", Variant::Bool(true))
        .unwrap();
    frame(
        &mut dom,
        inner,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 10, 0.0, 10),
    );

    let elements = resolve(&screens(&dom), VIEWPORT);

    // The clipping frame itself is never clipped by its own flag.
    assert_eq!(elements[0].clip, None);
    assert_eq!(
        elements[1].clip,
        Some(Rect {
            x: 0.0,
            y: 0.0,
            width: 200.0,
            height: 200.0,
        })
    );
    // Both rects intersected: x 100..200, y 0..100.
    assert_eq!(
        elements[2].clip,
        Some(Rect {
            x: 100.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        })
    );
}

#[test]
fn a_clip_that_misses_its_parent_entirely_is_empty_rather_than_negative() {
    let left = Rect {
        x: 0.0,
        y: 0.0,
        width: 10.0,
        height: 10.0,
    };
    let right = Rect {
        x: 100.0,
        y: 100.0,
        width: 10.0,
        height: 10.0,
    };

    let overlap = left.intersect(&right);

    assert_eq!(overlap.width, 0.0);
    assert_eq!(overlap.height, 0.0);
}

// `UIListLayout`: siblings keep their `Size`, lose their `Position`, and are
// stacked along the fill direction in their own sort order.

/// A `UIListLayout` under `parent`; `FillDirection` vertical, aligned top-left
/// so the arithmetic is bare. Returns the referent for further properties.
fn list(dom: &mut WeakDom, parent: Ref) -> Ref {
    let layout = dom.new_instance("UIListLayout", "UIListLayout", Some(parent));
    dom.set_property(layout, "FillDirection", Variant::Enum(1))
        .unwrap();
    dom.set_property(layout, "HorizontalAlignment", Variant::Enum(1))
        .unwrap();
    dom.set_property(layout, "VerticalAlignment", Variant::Enum(1))
        .unwrap();
    layout
}

fn origins(dom: &WeakDom) -> Vec<[f32; 2]> {
    resolve(&screens(dom), VIEWPORT)
        .iter()
        .map(|element| [element.rect.x, element.rect.y])
        .collect()
}

#[test]
fn a_vertical_list_stacks_siblings_and_ignores_their_position() {
    let (mut dom, gui) = screen_gui();
    list(&mut dom, gui);
    for _ in 0..3 {
        // A position that would scatter them if it were honoured.
        frame(
            &mut dom,
            gui,
            udim2(0.5, 0, 0.5, 0),
            udim2(0.0, 100, 0.0, 30),
        );
    }

    assert_eq!(origins(&dom), [[0.0, 0.0], [0.0, 30.0], [0.0, 60.0]]);
}

#[test]
fn a_horizontal_list_stacks_along_x() {
    let (mut dom, gui) = screen_gui();
    let layout = list(&mut dom, gui);
    dom.set_property(layout, "FillDirection", Variant::Enum(0))
        .unwrap();
    for _ in 0..2 {
        frame(
            &mut dom,
            gui,
            udim2(0.0, 0, 0.0, 0),
            udim2(0.0, 100, 0.0, 30),
        );
    }

    assert_eq!(origins(&dom), [[0.0, 0.0], [100.0, 0.0]]);
}

#[test]
fn list_padding_is_a_udim_against_the_parent_along_the_fill_axis() {
    let (mut dom, gui) = screen_gui();
    let layout = list(&mut dom, gui);
    // 1 % of the 600 px viewport height plus 4 px: 10 px between items.
    dom.set_property(
        layout,
        "Padding",
        Variant::UDim(UDim {
            scale: 0.01,
            offset: 4,
        }),
    )
    .unwrap();
    for _ in 0..2 {
        frame(
            &mut dom,
            gui,
            udim2(0.0, 0, 0.0, 0),
            udim2(0.0, 100, 0.0, 30),
        );
    }

    assert_eq!(origins(&dom), [[0.0, 0.0], [0.0, 40.0]]);
}

#[test]
fn list_alignment_places_the_stack_and_each_item_across_it() {
    let (mut dom, gui) = screen_gui();
    let layout = list(&mut dom, gui);
    // Centre across, bottom along: the enums' own defaults are Center.
    dom.set_property(layout, "HorizontalAlignment", Variant::Enum(0))
        .unwrap();
    dom.set_property(layout, "VerticalAlignment", Variant::Enum(2))
        .unwrap();
    frame(
        &mut dom,
        gui,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 200, 0.0, 30),
    );
    frame(
        &mut dom,
        gui,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 100, 0.0, 50),
    );

    // Stack is 80 px tall on a 600 px viewport; each item centred on 800 px.
    assert_eq!(origins(&dom), [[300.0, 520.0], [350.0, 550.0]]);
}

#[test]
fn list_order_is_layout_order_with_ties_in_tree_order() {
    let (mut dom, gui) = screen_gui();
    list(&mut dom, gui);
    let last = frame(
        &mut dom,
        gui,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 10, 0.0, 10),
    );
    dom.set_property(last, "LayoutOrder", Variant::Int32(5))
        .unwrap();
    frame(
        &mut dom,
        gui,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 10, 0.0, 20),
    );
    frame(
        &mut dom,
        gui,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 10, 0.0, 30),
    );

    // Elements come back in tree order; the first sits under the other two.
    assert_eq!(origins(&dom), [[0.0, 50.0], [0.0, 0.0], [0.0, 20.0]]);
}

#[test]
fn sort_order_name_sorts_siblings_alphabetically() {
    let (mut dom, gui) = screen_gui();
    let layout = list(&mut dom, gui);
    dom.set_property(layout, "SortOrder", Variant::Enum(0))
        .unwrap();
    for name in ["b", "a"] {
        let referent = dom.new_instance("Frame", name, Some(gui));
        dom.set_property(referent, "Size", udim2(0.0, 10, 0.0, 10))
            .unwrap();
    }

    assert_eq!(origins(&dom), [[0.0, 10.0], [0.0, 0.0]]);
}

#[test]
fn a_list_lays_out_a_frames_children_against_that_frame() {
    let (mut dom, gui) = screen_gui();
    let parent = frame(
        &mut dom,
        gui,
        udim2(0.0, 100, 0.0, 200),
        udim2(0.0, 300, 0.0, 300),
    );
    list(&mut dom, parent);
    for _ in 0..2 {
        frame(
            &mut dom,
            parent,
            udim2(0.0, 0, 0.0, 0),
            udim2(0.5, 0, 0.0, 30),
        );
    }

    let elements = resolve(&screens(&dom), VIEWPORT);
    assert_eq!(
        elements[2].rect,
        Rect {
            x: 100.0,
            y: 230.0,
            width: 150.0,
            height: 30.0,
        }
    );
}

#[test]
fn a_list_does_not_change_paint_order() {
    let (mut dom, gui) = screen_gui();
    list(&mut dom, gui);
    let top = frame(
        &mut dom,
        gui,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 10, 0.0, 10),
    );
    dom.set_property(top, "ZIndex", Variant::Int32(2)).unwrap();
    frame(
        &mut dom,
        gui,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 10, 0.0, 10),
    );

    // Painted last for its `ZIndex`, yet still first in the stack.
    assert_eq!(origins(&dom), [[0.0, 10.0], [0.0, 0.0]]);
}
