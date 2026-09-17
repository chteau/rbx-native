//! `UIListLayout`'s flex family: `HorizontalFlex`/`VerticalFlex`, `Wraps`,
//! `ItemLineAlignment` and the per-item `UIFlexItem`. The plain stacking the
//! same layout does without any of them lives in [`super::layout`].

use super::*;

/// A `UIListLayout` under `parent`, horizontal and aligned top-left so the
/// arithmetic is bare, with no padding between items.
fn flex_list(dom: &mut WeakDom, parent: Ref, vertical: bool) -> Ref {
    let layout = dom.new_instance("UIListLayout", "UIListLayout", Some(parent));
    dom.set_property(layout, "FillDirection", Variant::Enum(u32::from(vertical)))
        .unwrap();
    dom.set_property(layout, "HorizontalAlignment", Variant::Enum(1))
        .unwrap();
    dom.set_property(layout, "VerticalAlignment", Variant::Enum(1))
        .unwrap();
    layout
}

/// A `UIFlexItem` under `item`. `mode` is an `Enum.UIFlexMode` ordinal.
fn flex_item(dom: &mut WeakDom, item: Ref, mode: u32) -> Ref {
    let flex = dom.new_instance("UIFlexItem", "UIFlexItem", Some(item));
    dom.set_property(flex, "FlexMode", Variant::Enum(mode))
        .unwrap();
    flex
}

fn rects(dom: &WeakDom) -> Vec<Rect> {
    resolve(&screens(dom), VIEWPORT)
        .iter()
        .map(|element| element.rect)
        .collect()
}

/// `HorizontalFlex = Fill` on a *vertical* list is the cross direction: the
/// items keep their stacked heights and take the whole width.
///
/// These are `Fragment.rbxl`'s own numbers. Its "Content" frame is 522x290
/// with a 15 px `UIPadding`, leaving 492x260 for the list; the two buttons
/// are `{0, 200}, {0, 50}` yet come out 492 wide in Studio.
#[test]
fn a_vertical_lists_horizontal_fill_stretches_items_to_the_full_width() {
    let (mut dom, gui) = screen_gui();
    let content = frame(
        &mut dom,
        gui,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 492, 0.0, 260),
    );
    let layout = flex_list(&mut dom, content, true);
    dom.set_property(layout, "HorizontalFlex", Variant::Enum(1))
        .unwrap();
    dom.set_property(
        layout,
        "Padding",
        Variant::UDim(UDim {
            scale: 0.0,
            offset: 10,
        }),
    )
    .unwrap();
    for _ in 0..2 {
        frame(
            &mut dom,
            content,
            udim2(0.5, 0, 0.5, 0),
            udim2(0.0, 200, 0.0, 50),
        );
    }

    assert_eq!(
        rects(&dom)[1..],
        [
            Rect {
                x: 0.0,
                y: 0.0,
                width: 492.0,
                height: 50.0,
            },
            Rect {
                x: 0.0,
                y: 60.0,
                width: 492.0,
                height: 50.0,
            },
        ]
    );
}

/// `Fragment.rbxl`'s stats frame: `VerticalFlex = Fill` along the fill
/// direction makes two full-height labels split the height between them,
/// since Fill's 1:1 ratio shrinks as readily as it grows.
#[test]
fn fill_along_the_stack_shrinks_oversized_items_to_share_the_space() {
    let (mut dom, gui) = screen_gui();
    let stats = frame(
        &mut dom,
        gui,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 160, 0.0, 90),
    );
    let layout = flex_list(&mut dom, stats, true);
    dom.set_property(layout, "VerticalFlex", Variant::Enum(1))
        .unwrap();
    dom.set_property(layout, "HorizontalFlex", Variant::Enum(1))
        .unwrap();
    dom.set_property(
        layout,
        "Padding",
        Variant::UDim(UDim {
            scale: 0.0,
            offset: 10,
        }),
    )
    .unwrap();
    for _ in 0..2 {
        frame(
            &mut dom,
            stats,
            udim2(0.0, 0, 0.0, 0),
            udim2(1.0, 0, 1.0, 0),
        );
    }

    // 90 px of room for 180 px of labels plus a 10 px gap: each gives up 50.
    assert_eq!(
        rects(&dom)[1..],
        [
            Rect {
                x: 0.0,
                y: 0.0,
                width: 160.0,
                height: 40.0,
            },
            Rect {
                x: 0.0,
                y: 50.0,
                width: 160.0,
                height: 40.0,
            },
        ]
    );
}

/// Four 125 px items on an 800 px line leave 300 px of slack; each mode
/// hands it out differently.
fn four_items(flex: u32) -> Vec<Rect> {
    let (mut dom, gui) = screen_gui();
    let layout = flex_list(&mut dom, gui, false);
    dom.set_property(layout, "HorizontalFlex", Variant::Enum(flex))
        .unwrap();
    for _ in 0..4 {
        frame(
            &mut dom,
            gui,
            udim2(0.0, 0, 0.0, 0),
            udim2(0.0, 125, 0.0, 30),
        );
    }
    rects(&dom)
}

#[test]
fn space_between_adds_the_slack_between_items_but_not_around_them() {
    let origins: Vec<f32> = four_items(3).iter().map(|rect| rect.x).collect();

    // Three gaps of 100, and the run still starts hard against the edge.
    assert_eq!(origins, [0.0, 225.0, 450.0, 675.0]);
}

#[test]
fn space_around_gives_every_item_equal_space_on_both_sides() {
    let origins: Vec<f32> = four_items(2).iter().map(|rect| rect.x).collect();

    // 37.5 on each outer edge, 75 between: each item owns 75 of the slack.
    assert_eq!(origins, [37.5, 237.5, 437.5, 637.5]);
}

#[test]
fn space_evenly_makes_every_gap_the_same_including_the_outer_two() {
    let origins: Vec<f32> = four_items(4).iter().map(|rect| rect.x).collect();

    // Five equal gaps of 60.
    assert_eq!(origins, [60.0, 245.0, 430.0, 615.0]);
}

#[test]
fn fill_along_the_stack_grows_every_item_equally() {
    let laid = four_items(1);
    let sizes: Vec<f32> = laid.iter().map(|rect| rect.width).collect();
    let origins: Vec<f32> = laid.iter().map(|rect| rect.x).collect();

    assert_eq!(sizes, [200.0; 4]);
    assert_eq!(origins, [0.0, 200.0, 400.0, 600.0]);
}

#[test]
fn a_space_mode_leaves_the_items_own_sizes_alone() {
    let sizes: Vec<f32> = four_items(3).iter().map(|rect| rect.width).collect();

    assert_eq!(sizes, [125.0; 4]);
}

#[test]
fn wraps_moves_an_item_that_no_longer_fits_onto_a_second_line() {
    let (mut dom, gui) = screen_gui();
    let layout = flex_list(&mut dom, gui, false);
    dom.set_property(layout, "Wraps", Variant::Bool(true))
        .unwrap();
    for _ in 0..3 {
        frame(
            &mut dom,
            gui,
            udim2(0.0, 0, 0.0, 0),
            udim2(0.0, 300, 0.0, 40),
        );
    }

    // Two of the three fit across 800 px; the third starts a line of its own,
    // one line-height down.
    let origins: Vec<[f32; 2]> = rects(&dom).iter().map(|rect| [rect.x, rect.y]).collect();
    assert_eq!(origins, [[0.0, 0.0], [300.0, 0.0], [0.0, 40.0]]);
}

#[test]
fn without_wraps_a_line_overflows_rather_than_breaking() {
    let (mut dom, gui) = screen_gui();
    flex_list(&mut dom, gui, false);
    for _ in 0..3 {
        frame(
            &mut dom,
            gui,
            udim2(0.0, 0, 0.0, 0),
            udim2(0.0, 300, 0.0, 40),
        );
    }

    let origins: Vec<[f32; 2]> = rects(&dom).iter().map(|rect| [rect.x, rect.y]).collect();
    assert_eq!(origins, [[0.0, 0.0], [300.0, 0.0], [600.0, 0.0]]);
}

#[test]
fn item_line_alignment_stretch_fills_the_lines_cross_direction() {
    let (mut dom, gui) = screen_gui();
    let layout = flex_list(&mut dom, gui, false);
    dom.set_property(layout, "ItemLineAlignment", Variant::Enum(4))
        .unwrap();
    for height in [30, 50] {
        frame(
            &mut dom,
            gui,
            udim2(0.0, 0, 0.0, 0),
            udim2(0.0, 100, 0.0, height),
        );
    }

    // The line is as tall as its tallest item, and both fill it.
    let laid = rects(&dom);
    assert_eq!(
        laid.iter().map(|rect| rect.height).collect::<Vec<_>>(),
        [50.0, 50.0]
    );
    assert_eq!(
        laid.iter().map(|rect| rect.y).collect::<Vec<_>>(),
        [0.0, 0.0]
    );
}

#[test]
fn item_line_alignment_center_centres_a_short_item_in_its_line() {
    let (mut dom, gui) = screen_gui();
    let layout = flex_list(&mut dom, gui, false);
    dom.set_property(layout, "ItemLineAlignment", Variant::Enum(2))
        .unwrap();
    for height in [30, 50] {
        frame(
            &mut dom,
            gui,
            udim2(0.0, 0, 0.0, 0),
            udim2(0.0, 100, 0.0, height),
        );
    }

    let laid = rects(&dom);
    assert_eq!(laid[0].y, 10.0);
    assert_eq!(laid[0].height, 30.0);
    assert_eq!(laid[1].y, 0.0);
}

#[test]
fn a_flex_item_grow_ratio_splits_the_free_space_in_proportion() {
    let (mut dom, gui) = screen_gui();
    flex_list(&mut dom, gui, false);
    for ratio in [1.0f32, 4.0] {
        let item = frame(
            &mut dom,
            gui,
            udim2(0.0, 0, 0.0, 0),
            udim2(0.0, 100, 0.0, 30),
        );
        let flex = flex_item(&mut dom, item, 4);
        dom.set_property(flex, "GrowRatio", Variant::Float32(ratio))
            .unwrap();
        dom.set_property(flex, "ShrinkRatio", Variant::Float32(0.0))
            .unwrap();
    }
    // A third item with no `UIFlexItem` at all: it neither grows nor shrinks.
    frame(
        &mut dom,
        gui,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 100, 0.0, 30),
    );

    // 500 px spare, split 1:4, with the last item left at its own width.
    let laid = rects(&dom);
    assert_eq!(
        laid.iter().map(|rect| rect.width).collect::<Vec<_>>(),
        [200.0, 500.0, 100.0]
    );
    assert_eq!(
        laid.iter().map(|rect| rect.x).collect::<Vec<_>>(),
        [0.0, 200.0, 700.0]
    );
}

#[test]
fn flex_mode_grow_takes_every_spare_pixel_and_leaves_its_neighbour_alone() {
    let (mut dom, gui) = screen_gui();
    flex_list(&mut dom, gui, false);
    frame(
        &mut dom,
        gui,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 100, 0.0, 30),
    );
    let grower = frame(
        &mut dom,
        gui,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 100, 0.0, 30),
    );
    flex_item(&mut dom, grower, 1);

    let widths: Vec<f32> = rects(&dom).iter().map(|rect| rect.width).collect();
    assert_eq!(widths, [100.0, 700.0]);
}

#[test]
fn flex_mode_shrink_absorbs_the_overflow_on_its_own() {
    let (mut dom, gui) = screen_gui();
    flex_list(&mut dom, gui, false);
    let shrinker = frame(
        &mut dom,
        gui,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 500, 0.0, 30),
    );
    flex_item(&mut dom, shrinker, 2);
    frame(
        &mut dom,
        gui,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 500, 0.0, 30),
    );

    // 200 px over an 800 px line, and only the shrinkable item gives ground.
    let laid = rects(&dom);
    assert_eq!(
        laid.iter().map(|rect| rect.width).collect::<Vec<_>>(),
        [300.0, 500.0]
    );
    assert_eq!(
        laid.iter().map(|rect| rect.x).collect::<Vec<_>>(),
        [0.0, 300.0]
    );
}

#[test]
fn flex_mode_grow_never_shrinks_when_the_line_overflows() {
    let (mut dom, gui) = screen_gui();
    flex_list(&mut dom, gui, false);
    for _ in 0..2 {
        let item = frame(
            &mut dom,
            gui,
            udim2(0.0, 0, 0.0, 0),
            udim2(0.0, 500, 0.0, 30),
        );
        flex_item(&mut dom, item, 1);
    }

    // "Objects set to Grow never shrink below their basis size, so overflow
    // may occur" — both keep their 500 and run off the end.
    let widths: Vec<f32> = rects(&dom).iter().map(|rect| rect.width).collect();
    assert_eq!(widths, [500.0, 500.0]);
}

#[test]
fn a_flex_items_own_line_alignment_overrides_the_layouts() {
    let (mut dom, gui) = screen_gui();
    let layout = flex_list(&mut dom, gui, false);
    dom.set_property(layout, "ItemLineAlignment", Variant::Enum(1))
        .unwrap();
    let odd = frame(
        &mut dom,
        gui,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 100, 0.0, 30),
    );
    let flex = flex_item(&mut dom, odd, 0);
    dom.set_property(flex, "ItemLineAlignment", Variant::Enum(3))
        .unwrap();
    frame(
        &mut dom,
        gui,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 100, 0.0, 50),
    );

    // The layout says Start; this one item says End, so it drops to the
    // bottom of the 50 px line.
    let laid = rects(&dom);
    assert_eq!(laid[0].y, 20.0);
    assert_eq!(laid[1].y, 0.0);
}
