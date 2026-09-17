//! The modifiers that change how big a box is before it is placed:
//! `UIPadding`, `UIScale`, the `UIConstraint` family, `AutomaticSize` and
//! `SizeConstraint`. Where the resulting box lands is [`super::placement`].

use super::*;

/// A `UIComponent` of `class` hung off `parent`, its properties left for the
/// caller to fill in.
fn component(dom: &mut WeakDom, parent: Ref, class: &str) -> Ref {
    dom.new_instance(class, class, Some(parent))
}

fn udim(scale: f32, offset: i32) -> Variant {
    Variant::UDim(UDim { scale, offset })
}

fn vector2(x: f32, y: f32) -> Variant {
    Variant::Vector2(Vector2Data { x, y })
}

/// A `UIPadding` with the same `UDim` on all four sides.
fn padding(dom: &mut WeakDom, parent: Ref, scale: f32, offset: i32) {
    let referent = component(dom, parent, "UIPadding");
    for side in ["PaddingLeft", "PaddingRight", "PaddingTop", "PaddingBottom"] {
        dom.set_property(referent, side, udim(scale, offset))
            .unwrap();
    }
}

#[test]
fn ui_padding_shrinks_the_box_children_resolve_against() {
    let (mut dom, gui) = screen_gui();
    let parent = frame(
        &mut dom,
        gui,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 200, 0.0, 100),
    );
    padding(&mut dom, parent, 0.0, 15);
    // Full width of whatever it is given, so the padding is all that shows.
    frame(
        &mut dom,
        parent,
        udim2(0.0, 0, 0.0, 0),
        udim2(1.0, 0, 1.0, 0),
    );

    assert_eq!(
        rects(&dom)[1],
        Rect {
            x: 15.0,
            y: 15.0,
            width: 170.0,
            height: 70.0,
        }
    );
}

#[test]
fn ui_padding_scale_is_a_fraction_of_the_padded_elements_own_size() {
    let (mut dom, gui) = screen_gui();
    let parent = frame(
        &mut dom,
        gui,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 200, 0.0, 100),
    );
    padding(&mut dom, parent, 0.1, 0);
    frame(
        &mut dom,
        parent,
        udim2(0.0, 0, 0.0, 0),
        udim2(1.0, 0, 1.0, 0),
    );

    // 10% of 200 on the left and right, 10% of 100 top and bottom.
    assert_eq!(
        rects(&dom)[1],
        Rect {
            x: 20.0,
            y: 10.0,
            width: 160.0,
            height: 80.0,
        }
    );
}

#[test]
fn a_list_layout_stacks_inside_the_padding_not_around_it() {
    let (mut dom, gui) = screen_gui();
    let parent = frame(
        &mut dom,
        gui,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 200, 0.0, 200),
    );
    padding(&mut dom, parent, 0.0, 15);
    let list = component(&mut dom, parent, "UIListLayout");
    dom.set_property(list, "FillDirection", Variant::Enum(1))
        .unwrap();
    dom.set_property(list, "HorizontalAlignment", Variant::Enum(1))
        .unwrap();
    dom.set_property(list, "VerticalAlignment", Variant::Enum(1))
        .unwrap();
    for _ in 0..2 {
        frame(
            &mut dom,
            parent,
            udim2(0.0, 0, 0.0, 0),
            udim2(1.0, 0, 0.0, 30),
        );
    }

    let rects = rects(&dom);
    assert_eq!(rects[1].x, 15.0);
    assert_eq!(rects[1].y, 15.0);
    // A `{1, 0}` item fills the padded width, not the parent's own.
    assert_eq!(rects[1].width, 170.0);
    assert_eq!(rects[2].y, 45.0);
}

#[test]
fn ui_scale_multiplies_the_size_and_so_every_descendant_with_it() {
    let (mut dom, gui) = screen_gui();
    let parent = frame(
        &mut dom,
        gui,
        udim2(0.0, 100, 0.0, 100),
        udim2(0.0, 200, 0.0, 100),
    );
    let scale = component(&mut dom, parent, "UIScale");
    dom.set_property(scale, "Scale", Variant::Float32(0.5))
        .unwrap();
    frame(
        &mut dom,
        parent,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.5, 0, 1.0, 0),
    );

    let rects = rects(&dom);
    // `Position` is untouched: `UIScale` is documented only as a multiplier on
    // `AbsoluteSize`, and the default `AnchorPoint` pins the top-left corner.
    assert_eq!(
        rects[0],
        Rect {
            x: 100.0,
            y: 100.0,
            width: 100.0,
            height: 50.0,
        }
    );
    assert_eq!(rects[1].width, 50.0);
    assert_eq!(rects[1].height, 50.0);
}

/// One frame with an aspect ratio constraint, sized `size` against the
/// viewport; returns its resolved rect.
fn with_aspect(size: Variant, ratio: f32, aspect_type: u32, dominant: u32) -> Rect {
    let (mut dom, gui) = screen_gui();
    let referent = frame(&mut dom, gui, udim2(0.0, 0, 0.0, 0), size);
    let constraint = component(&mut dom, referent, "UIAspectRatioConstraint");
    dom.set_property(constraint, "AspectRatio", Variant::Float32(ratio))
        .unwrap();
    dom.set_property(constraint, "AspectType", Variant::Enum(aspect_type))
        .unwrap();
    dom.set_property(constraint, "DominantAxis", Variant::Enum(dominant))
        .unwrap();
    rects(&dom)[0]
}

#[test]
fn fit_within_max_size_takes_the_largest_box_of_the_ratio_inside_the_elements_own() {
    // 400x400 at 2:1 can only be 400x200, whichever axis dominates.
    for dominant in [0, 1] {
        let rect = with_aspect(udim2(0.0, 400, 0.0, 400), 2.0, 0, dominant);
        assert_eq!((rect.width, rect.height), (400.0, 200.0));
    }
    // 100x400 at 2:1 is height-limited the other way about.
    for dominant in [0, 1] {
        let rect = with_aspect(udim2(0.0, 100, 0.0, 400), 2.0, 0, dominant);
        assert_eq!((rect.width, rect.height), (100.0, 50.0));
    }
}

#[test]
fn scale_with_parent_size_keeps_the_dominant_axis_and_only_clamps_to_the_parent() {
    // The viewport is 800x600, so neither result is clamped and the dominant
    // axis is the one the size is actually taken from.
    let width = with_aspect(udim2(0.0, 300, 0.0, 100), 2.0, 1, 0);
    assert_eq!((width.width, width.height), (300.0, 150.0));

    let height = with_aspect(udim2(0.0, 300, 0.0, 100), 2.0, 1, 1);
    assert_eq!((height.width, height.height), (200.0, 100.0));
}

#[test]
fn scale_with_parent_size_shrinks_a_candidate_that_overflows_the_parent() {
    // Width-dominant on a full-width element at 2:1 wants 800x400, which fits
    // the 800x600 viewport; ask for 4:1 of the height instead and it does not.
    let rect = with_aspect(udim2(1.0, 0, 0.0, 700), 0.5, 1, 1);
    // 700 tall at 0.5:1 is 350x700, clamped by the 600px viewport to 300x600.
    assert_eq!((rect.width, rect.height), (300.0, 600.0));
}

#[test]
fn the_fragment_windows_aspect_ratio_matches_the_reference_screenshot() {
    // `Fragment.rbxl`'s CounterWindow: half the canvas each way, 1.8:1,
    // FitWithinMaxSize, width-dominant. Against the 1506x580 canvas the
    // reference was captured at, Roblox draws it 522x290.
    let (mut dom, gui) = screen_gui();
    let referent = frame(&mut dom, gui, udim2(0.5, 0, 0.5, 0), udim2(0.5, 0, 0.5, 0));
    dom.set_property(referent, "AnchorPoint", vector2(0.5, 0.5))
        .unwrap();
    let constraint = component(&mut dom, referent, "UIAspectRatioConstraint");
    dom.set_property(constraint, "AspectRatio", Variant::Float32(1.8))
        .unwrap();

    let elements = resolve(&screens(&dom), [1506.0, 580.0]);
    assert!((elements[0].rect.width - 522.0).abs() < 0.5);
    assert_eq!(elements[0].rect.height, 290.0);
    // Centred, as the reference shows it.
    assert_eq!(elements[0].rect.y + elements[0].rect.height * 0.5, 290.0);
}

#[test]
fn a_size_constraint_clamps_the_resolved_size_both_ways() {
    let (mut dom, gui) = screen_gui();
    let referent = frame(&mut dom, gui, udim2(0.0, 0, 0.0, 0), udim2(1.0, 0, 0.0, 10));
    let constraint = component(&mut dom, referent, "UISizeConstraint");
    dom.set_property(constraint, "MinSize", vector2(0.0, 40.0))
        .unwrap();
    dom.set_property(constraint, "MaxSize", vector2(300.0, 1000.0))
        .unwrap();

    let rect = rects(&dom)[0];
    assert_eq!((rect.width, rect.height), (300.0, 40.0));
}

#[test]
fn a_size_constraint_outranks_an_aspect_ratio() {
    let (mut dom, gui) = screen_gui();
    let referent = frame(
        &mut dom,
        gui,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 400, 0.0, 400),
    );
    let aspect = component(&mut dom, referent, "UIAspectRatioConstraint");
    dom.set_property(aspect, "AspectRatio", Variant::Float32(2.0))
        .unwrap();
    let size = component(&mut dom, referent, "UISizeConstraint");
    dom.set_property(size, "MaxSize", vector2(100.0, 100.0))
        .unwrap();

    // The ratio would give 400x200; the clamp is applied last and wins.
    let rect = rects(&dom)[0];
    assert_eq!((rect.width, rect.height), (100.0, 100.0));
}

#[test]
fn relative_xx_and_yy_take_both_size_scales_against_one_parent_axis() {
    let (mut dom, gui) = screen_gui();
    let xx = frame(&mut dom, gui, udim2(0.0, 0, 0.0, 0), udim2(0.5, 0, 0.5, 0));
    dom.set_property(xx, "SizeConstraint", Variant::Enum(1))
        .unwrap();
    let yy = frame(&mut dom, gui, udim2(0.0, 0, 0.0, 0), udim2(0.5, 0, 0.5, 0));
    dom.set_property(yy, "SizeConstraint", Variant::Enum(2))
        .unwrap();

    let rects = rects(&dom);
    // The viewport is 800x600: RelativeXX squares off on the width, RelativeYY
    // on the height.
    assert_eq!((rects[0].width, rects[0].height), (400.0, 400.0));
    assert_eq!((rects[1].width, rects[1].height), (300.0, 300.0));
}

/// A frame with `AutomaticSize` set to `mode`, holding one 60x40 child placed
/// 10px in from its corner.
fn automatic(mode: u32) -> Rect {
    let (mut dom, gui) = screen_gui();
    let parent = frame(
        &mut dom,
        gui,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 30, 0.0, 30),
    );
    dom.set_property(parent, "AutomaticSize", Variant::Enum(mode))
        .unwrap();
    frame(
        &mut dom,
        parent,
        udim2(0.0, 10, 0.0, 10),
        udim2(0.0, 60, 0.0, 40),
    );
    rects(&dom)[0]
}

#[test]
fn automatic_size_grows_only_the_axes_it_names_and_never_below_the_size() {
    // The child reaches 70x50; `Size` is the floor, so the untouched axis
    // stays at 30.
    assert_eq!((automatic(1).width, automatic(1).height), (70.0, 30.0));
    assert_eq!((automatic(2).width, automatic(2).height), (30.0, 50.0));
    assert_eq!((automatic(3).width, automatic(3).height), (70.0, 50.0));
    assert_eq!((automatic(0).width, automatic(0).height), (30.0, 30.0));
}

#[test]
fn automatic_size_counts_a_list_layouts_run_and_the_padding_around_it() {
    let (mut dom, gui) = screen_gui();
    let parent = frame(&mut dom, gui, udim2(0.0, 0, 0.0, 0), udim2(0.0, 0, 0.0, 0));
    dom.set_property(parent, "AutomaticSize", Variant::Enum(3))
        .unwrap();
    padding(&mut dom, parent, 0.0, 5);
    let list = component(&mut dom, parent, "UIListLayout");
    dom.set_property(list, "FillDirection", Variant::Enum(1))
        .unwrap();
    // Left-aligned, so the run starts on the padded box's own corner and any
    // double-counting of the left padding would show in the width.
    dom.set_property(list, "HorizontalAlignment", Variant::Enum(1))
        .unwrap();
    dom.set_property(list, "Padding", udim(0.0, 4)).unwrap();
    for _ in 0..3 {
        frame(
            &mut dom,
            parent,
            udim2(0.0, 0, 0.0, 0),
            udim2(0.0, 20, 0.0, 20),
        );
    }

    // Three 20px items, two 4px gaps, 5px of padding on every side.
    let rect = rects(&dom)[0];
    assert_eq!((rect.width, rect.height), (30.0, 78.0));
}

#[test]
fn a_child_sized_by_scale_along_an_automatic_axis_contributes_only_its_offset() {
    let (mut dom, gui) = screen_gui();
    let parent = frame(&mut dom, gui, udim2(0.0, 0, 0.0, 0), udim2(0.0, 0, 0.0, 50));
    dom.set_property(parent, "AutomaticSize", Variant::Enum(1))
        .unwrap();
    frame(
        &mut dom,
        parent,
        udim2(0.0, 0, 0.0, 0),
        udim2(2.0, 25, 1.0, 0),
    );

    let rects = rects(&dom);
    // The parent cannot grow to fit a child measured against the parent, so
    // only the 25px offset counts; the child then resolves against the answer.
    assert_eq!(rects[0].width, 25.0);
    assert_eq!(rects[1].width, 75.0);
    // The fixed axis is unaffected: the child still fills its 50px.
    assert_eq!(rects[1].height, 50.0);
}

#[test]
fn a_text_elements_own_content_size_grows_the_box_beside_its_children() {
    let (mut dom, gui) = screen_gui();
    let parent = frame(&mut dom, gui, udim2(0.0, 0, 0.0, 0), udim2(0.0, 0, 0.0, 0));
    dom.set_property(parent, "AutomaticSize", Variant::Enum(3))
        .unwrap();
    frame(
        &mut dom,
        parent,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 10, 0.0, 90),
    );

    // The hook the text side fills in once it has measured its own glyphs.
    let mut planned = screens(&dom);
    planned[0].roots[0].content_size = Some([120.0, 30.0]);

    let rect = resolve(&planned, VIEWPORT)[0].rect;
    assert_eq!((rect.width, rect.height), (120.0, 90.0));
}
