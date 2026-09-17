//! `ScrollingFrame`: what is read, the canvas its children resolve against,
//! the window they are clipped to and the bars a still frame shows.

use rbx_assets::AssetRef;

use super::super::plan::{Inset, Scrolling, Span};
use super::*;
use crate::scene::srgb_to_linear;

const THICKNESS: i32 = 12;

/// A `ScrollingFrame` at `position`, `size` pixels, `ScrollBarThickness`
/// 12 and everything else left to the defaults.
fn scrolling_frame(dom: &mut WeakDom, parent: Ref, position: Variant, size: Variant) -> Ref {
    let referent = dom.new_instance("ScrollingFrame", "Scroll", Some(parent));
    dom.set_property(referent, "Position", position).unwrap();
    dom.set_property(referent, "Size", size).unwrap();
    dom.set_property(referent, "BorderSizePixel", Variant::Int32(0))
        .unwrap();
    dom.set_property(referent, "ScrollBarThickness", Variant::Int32(THICKNESS))
        .unwrap();
    referent
}

/// A 200 × 100 scrolling frame at the origin.
fn window(dom: &mut WeakDom, gui: Ref) -> Ref {
    scrolling_frame(dom, gui, udim2(0.0, 0, 0.0, 0), udim2(0.0, 200, 0.0, 100))
}

fn canvas(dom: &mut WeakDom, frame: Ref, width: i32, height: i32) {
    dom.set_property(frame, "CanvasSize", udim2(0.0, width, 0.0, height))
        .unwrap();
}

fn vector2(x: f32, y: f32) -> Variant {
    Variant::Vector2(Vector2Data { x, y })
}

fn scrolling_of(dom: &WeakDom) -> Scrolling {
    screens(dom)[0].roots[0]
        .scrolling
        .clone()
        .expect("a ScrollingFrame reads its scrolling")
}

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

/// The bar segments among `elements`: everything after the frame and its
/// `children` children.
fn bars(elements: &[Element], children: usize) -> &[Element] {
    &elements[1 + children..]
}

#[test]
fn the_defaults_are_the_docs_defaults() {
    let (mut dom, gui) = screen_gui();
    let frame = dom.new_instance("ScrollingFrame", "Scroll", Some(gui));
    dom.set_property(frame, "Size", udim2(0.0, 10, 0.0, 10))
        .unwrap();

    let scrolling = scrolling_of(&dom);

    assert_eq!(scrolling.canvas_size, Span::default());
    assert_eq!(scrolling.canvas_position, [0.0, 0.0]);
    assert_eq!(scrolling.automatic_canvas, [false, false]);
    assert_eq!(scrolling.direction, [true, true], "ScrollingDirection.XY");
    assert_eq!(scrolling.thickness, 12.0);
    assert_eq!(scrolling.bar_color, [0.0; 3]);
    assert_eq!(scrolling.bar_alpha, 1.0);
    assert_eq!(
        scrolling.images.top,
        AssetRef::Native("textures/ui/Scroll/scroll-top.png".to_string())
    );
    assert_eq!(
        scrolling.images.mid,
        AssetRef::Native("textures/ui/Scroll/scroll-middle.png".to_string())
    );
    assert_eq!(
        scrolling.images.bottom,
        AssetRef::Native("textures/ui/Scroll/scroll-bottom.png".to_string())
    );
    assert!(!scrolling.bar_left, "VerticalScrollBarPosition.Right");
    assert_eq!(scrolling.vertical_inset, Inset::None);
    assert_eq!(scrolling.horizontal_inset, Inset::None);
}

#[test]
fn every_scrolling_property_is_read() {
    let (mut dom, gui) = screen_gui();
    let frame = window(&mut dom, gui);
    for (name, value) in [
        ("CanvasSize", udim2(1.0, 20, 2.0, 30)),
        ("CanvasPosition", vector2(15.0, 25.0)),
        ("AutomaticCanvasSize", Variant::Enum(3)),
        ("ScrollingDirection", Variant::Enum(1)),
        ("ScrollBarThickness", Variant::Int32(8)),
        (
            "ScrollBarImageColor3",
            Variant::Color3(Color3Data {
                r: 1.0,
                g: 0.5,
                b: 0.0,
            }),
        ),
        ("ScrollBarImageTransparency", Variant::Float32(0.25)),
        ("TopImage", Variant::String("rbxassetid://1".to_string())),
        ("MidImage", Variant::String("rbxassetid://2".to_string())),
        ("BottomImage", Variant::String("rbxassetid://3".to_string())),
        ("VerticalScrollBarPosition", Variant::Enum(1)),
        ("VerticalScrollBarInset", Variant::Enum(2)),
        ("HorizontalScrollBarInset", Variant::Enum(1)),
    ] {
        dom.set_property(frame, name, value).unwrap();
    }

    let scrolling = scrolling_of(&dom);

    assert_eq!(
        scrolling.canvas_size,
        Span {
            scale: [1.0, 2.0],
            offset: [20.0, 30.0],
        }
    );
    assert_eq!(scrolling.canvas_position, [15.0, 25.0]);
    assert_eq!(scrolling.automatic_canvas, [true, true]);
    assert_eq!(scrolling.direction, [true, false], "ScrollingDirection.X");
    assert_eq!(scrolling.thickness, 8.0);
    assert_eq!(scrolling.bar_color, [1.0, srgb_to_linear(0.5), 0.0]);
    assert_eq!(scrolling.bar_alpha, 0.75);
    assert_eq!(scrolling.images.top, AssetRef::Id(1));
    assert_eq!(scrolling.images.mid, AssetRef::Id(2));
    assert_eq!(scrolling.images.bottom, AssetRef::Id(3));
    assert!(scrolling.bar_left);
    assert_eq!(scrolling.vertical_inset, Inset::Always);
    assert_eq!(scrolling.horizontal_inset, Inset::ScrollBar);
}

// "If `false`, no scroll bars will be rendered" — the same thing a zero
// thickness comes to, so that is how it is read.
#[test]
fn scrolling_disabled_reads_as_no_bar_at_all() {
    let (mut dom, gui) = screen_gui();
    let frame = window(&mut dom, gui);
    dom.set_property(frame, "ScrollingEnabled", Variant::Bool(false))
        .unwrap();

    assert_eq!(scrolling_of(&dom).thickness, 0.0);
}

// The images are wanted by the loader like any `ImageLabel`'s.
#[test]
fn the_bar_images_are_among_the_screens_assets() {
    let (mut dom, gui) = screen_gui();
    window(&mut dom, gui);

    let mut assets = Vec::new();
    screens(&dom)[0].assets(&mut assets);

    assert_eq!(assets.len(), 3);
    assert_eq!(
        assets[0],
        AssetRef::Native("textures/ui/Scroll/scroll-top.png".to_string())
    );
}

#[test]
fn children_resolve_against_the_canvas_shifted_back_by_the_position() {
    let (mut dom, gui) = screen_gui();
    let frame = window(&mut dom, gui);
    canvas(&mut dom, frame, 400, 300);
    dom.set_property(frame, "CanvasPosition", vector2(50.0, 20.0))
        .unwrap();
    super::frame(
        &mut dom,
        frame,
        udim2(0.0, 0, 0.0, 0),
        udim2(1.0, 0, 1.0, 0),
    );

    let elements = resolve(&screens(&dom), VIEWPORT);

    let child = &elements[1];
    assert_eq!(child.rect, rect(-50.0, -20.0, 400.0, 300.0));
    assert_eq!(
        child.clip,
        Some(rect(0.0, 0.0, 200.0, 100.0)),
        "clipped to the window, `ClipsDescendants` unset or not"
    );
    // Both axes overflow: two bars of three segments each.
    assert_eq!(bars(&elements, 1).len(), 6);
}

#[test]
fn an_automatic_canvas_grows_to_hold_the_children() {
    let (mut dom, gui) = screen_gui();
    let frame = window(&mut dom, gui);
    dom.set_property(frame, "AutomaticCanvasSize", Variant::Enum(2))
        .unwrap();
    for row in 0..5 {
        super::frame(
            &mut dom,
            frame,
            udim2(0.0, 0, 0.0, row * 40),
            udim2(0.0, 200, 0.0, 40),
        );
    }

    let elements = resolve(&screens(&dom), VIEWPORT);

    assert_eq!(elements[5].rect, rect(0.0, 160.0, 200.0, 40.0));
    // The canvas came to 200 tall inside a 100 tall window: one vertical
    // bar, half the track long, at the top.
    let bars = bars(&elements, 5);
    assert_eq!(bars.len(), 3);
    assert_eq!(bars[0].rect, rect(188.0, 0.0, 12.0, 12.0), "top cap");
    assert_eq!(bars[1].rect, rect(188.0, 12.0, 12.0, 26.0), "middle");
    assert_eq!(bars[2].rect, rect(188.0, 38.0, 12.0, 12.0), "bottom cap");
    for bar in bars {
        assert_eq!(bar.rotation, 0.0);
        assert_eq!(bar.background_alpha, 0.0);
        let image = bar.image.as_ref().expect("a bar segment is an image");
        assert_eq!(image.tint, [0.0; 3]);
        assert_eq!(image.alpha, 1.0);
    }
    assert_eq!(
        bars[1].image.as_ref().unwrap().asset,
        AssetRef::Native("textures/ui/Scroll/scroll-middle.png".to_string())
    );
}

/// A 200 × 300 scrolling frame with a zero `CanvasSize`, an automatic
/// vertical canvas and `rows` children each `{1, 0}, {0.3, 0}`, stacked by
/// scale.
fn scale_rows(dom: &mut WeakDom, gui: Ref, rows: i32) -> Ref {
    let frame = scrolling_frame(dom, gui, udim2(0.0, 0, 0.0, 0), udim2(0.0, 200, 0.0, 300));
    dom.set_property(frame, "AutomaticCanvasSize", Variant::Enum(2))
        .unwrap();
    for row in 0..rows {
        super::frame(
            dom,
            frame,
            udim2(0.0, 0, 0.3 * row as f32, 0),
            udim2(1.0, 0, 0.3, 0),
        );
    }
    frame
}

#[test]
fn scale_sized_children_of_an_automatic_canvas_resolve_against_the_window() {
    let (mut dom, gui) = screen_gui();
    scale_rows(&mut dom, gui, 3);

    let elements = resolve(&screens(&dom), VIEWPORT);

    // A zero canvas is no smaller than the window on either axis, so a row
    // is 30% of the 300px window — and the whole 200px wide, though nothing
    // is automatic along X.
    for row in 0..3 {
        assert_eq!(
            elements[1 + row].rect,
            rect(0.0, 90.0 * row as f32, 200.0, 90.0)
        );
    }
    // Three rows come to 270px: the canvas grew to that, and fits.
    assert!(bars(&elements, 3).is_empty());
}

#[test]
fn an_automatic_canvas_grown_past_the_window_leaves_the_rows_their_size() {
    let (mut dom, gui) = screen_gui();
    scale_rows(&mut dom, gui, 4);

    let elements = resolve(&screens(&dom), VIEWPORT);

    // Four rows are 360px of content in a 300px window: the canvas overflows
    // and a bar shows, but a row is still 30% of the window, not of the
    // canvas it made.
    assert_eq!(elements[4].rect, rect(0.0, 270.0, 200.0, 90.0));
    let bars = bars(&elements, 4);
    assert_eq!(bars.len(), 3);
    // The thumb is the window's share of the 360px canvas: 300 * 300 / 360.
    let length: f32 = bars.iter().map(|bar| bar.rect.height).sum();
    assert_eq!(length, 250.0);
}

#[test]
fn the_thumb_sits_at_the_positions_share_of_the_slack() {
    let (mut dom, gui) = screen_gui();
    let frame = window(&mut dom, gui);
    canvas(&mut dom, frame, 100, 400);
    // Half of the 300 px the canvas can scroll by.
    dom.set_property(frame, "CanvasPosition", vector2(0.0, 150.0))
        .unwrap();

    let elements = resolve(&screens(&dom), VIEWPORT);

    // A quarter of the canvas shows, so the thumb is a quarter of the 100 px
    // track, and it has moved half of the 75 px left over.
    let bars = bars(&elements, 0);
    assert_eq!(bars.len(), 3);
    assert_eq!(bars[0].rect, rect(188.0, 37.5, 12.0, 12.0));
    assert_eq!(bars[1].rect, rect(188.0, 49.5, 12.0, 1.0));
    assert_eq!(bars[2].rect, rect(188.0, 50.5, 12.0, 12.0));
}

#[test]
fn a_horizontal_bar_lies_along_the_bottom_with_its_images_turned_a_quarter_back() {
    let (mut dom, gui) = screen_gui();
    let frame = window(&mut dom, gui);
    canvas(&mut dom, frame, 400, 50);

    let elements = resolve(&screens(&dom), VIEWPORT);

    let bars = bars(&elements, 0);
    assert_eq!(bars.len(), 3);
    // Half the canvas shows, so the thumb is half the 200 px track, at the
    // left. The middle covers x 12..88 along the bottom edge, handed over as
    // the standing box the quarter turn lays flat there.
    assert_eq!(bars[0].rect, rect(0.0, 88.0, 12.0, 12.0), "left cap");
    assert_eq!(bars[1].rect, rect(44.0, 56.0, 12.0, 76.0), "middle");
    assert_eq!(bars[2].rect, rect(88.0, 88.0, 12.0, 12.0), "right cap");
    for bar in bars {
        assert_eq!(bar.rotation, -90.0);
    }
}

#[test]
fn a_left_bar_with_an_always_inset_pushes_the_window_over() {
    let (mut dom, gui) = screen_gui();
    let frame = window(&mut dom, gui);
    dom.set_property(frame, "CanvasSize", udim2(1.0, 0, 0.0, 300))
        .unwrap();
    dom.set_property(frame, "VerticalScrollBarPosition", Variant::Enum(1))
        .unwrap();
    dom.set_property(frame, "VerticalScrollBarInset", Variant::Enum(2))
        .unwrap();
    super::frame(
        &mut dom,
        frame,
        udim2(0.0, 0, 0.0, 0),
        udim2(1.0, 0, 1.0, 0),
    );

    let elements = resolve(&screens(&dom), VIEWPORT);

    // The canvas' 100% is the window's width, which the bar has taken 12 px
    // of on the left.
    assert_eq!(elements[1].rect, rect(12.0, 0.0, 188.0, 300.0));
    assert_eq!(elements[1].clip, Some(rect(12.0, 0.0, 188.0, 100.0)));
    let bars = bars(&elements, 1);
    assert_eq!(bars.len(), 3, "the canvas no longer overflows sideways");
    assert_eq!(bars[0].rect, rect(0.0, 0.0, 12.0, 12.0));
}

#[test]
fn a_scroll_bar_inset_only_applies_while_that_bar_shows() {
    let (mut dom, gui) = screen_gui();
    let frame = window(&mut dom, gui);
    dom.set_property(frame, "HorizontalScrollBarInset", Variant::Enum(1))
        .unwrap();
    super::frame(
        &mut dom,
        frame,
        udim2(0.0, 0, 0.0, 0),
        udim2(1.0, 0, 1.0, 0),
    );

    canvas(&mut dom, frame, 100, 300);
    let fits = resolve(&screens(&dom), VIEWPORT);
    assert_eq!(fits[1].clip, Some(rect(0.0, 0.0, 200.0, 100.0)));

    canvas(&mut dom, frame, 400, 300);
    let overflows = resolve(&screens(&dom), VIEWPORT);
    assert_eq!(
        overflows[1].clip,
        Some(rect(0.0, 0.0, 200.0, 88.0)),
        "the horizontal bar's 12 px come off the window's height"
    );
}

#[test]
fn no_bar_shows_where_the_canvas_fits_and_the_position_then_does_nothing() {
    let (mut dom, gui) = screen_gui();
    let frame = window(&mut dom, gui);
    canvas(&mut dom, frame, 100, 50);
    dom.set_property(frame, "CanvasPosition", vector2(30.0, 30.0))
        .unwrap();
    super::frame(
        &mut dom,
        frame,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 10, 0.0, 10),
    );

    let elements = resolve(&screens(&dom), VIEWPORT);

    assert_eq!(elements.len(), 2);
    assert_eq!(elements[1].rect, rect(0.0, 0.0, 10.0, 10.0));
}

#[test]
fn scrolling_direction_keeps_the_other_axis_bar_away() {
    let (mut dom, gui) = screen_gui();
    let frame = window(&mut dom, gui);
    canvas(&mut dom, frame, 400, 300);

    dom.set_property(frame, "ScrollingDirection", Variant::Enum(1))
        .unwrap();
    let x_only = resolve(&screens(&dom), VIEWPORT);
    assert_eq!(bars(&x_only, 0).len(), 3);
    assert_eq!(bars(&x_only, 0)[0].rotation, -90.0, "the horizontal one");

    dom.set_property(frame, "ScrollingDirection", Variant::Enum(2))
        .unwrap();
    let y_only = resolve(&screens(&dom), VIEWPORT);
    assert_eq!(bars(&y_only, 0).len(), 3);
    assert_eq!(bars(&y_only, 0)[0].rotation, 0.0, "the vertical one");

    dom.set_property(frame, "ScrollingEnabled", Variant::Bool(false))
        .unwrap();
    assert!(bars(&resolve(&screens(&dom), VIEWPORT), 0).is_empty());
}

// What a host's wheel hit-tests against: the window (not the frame), and the
// reach along each axis the frame may actually scroll.
#[test]
fn the_frame_leaves_its_window_and_reach_behind_for_the_wheel() {
    let (mut dom, gui) = screen_gui();
    let frame = window(&mut dom, gui);
    canvas(&mut dom, frame, 400, 300);
    dom.set_property(frame, "VerticalScrollBarInset", Variant::Enum(2))
        .unwrap();

    let elements = resolve(&screens(&dom), VIEWPORT);
    let scroll = elements[0].scroll.expect("a ScrollingFrame leaves a window");
    assert_eq!(scroll.referent, frame);
    assert_eq!(scroll.rect, rect(0.0, 0.0, 200.0 - THICKNESS as f32, 100.0));
    assert_eq!(scroll.clip, None);
    assert_eq!(scroll.range, [400.0 - 188.0, 200.0]);
    assert!(bars(&elements, 0).iter().all(|bar| bar.scroll.is_none()));

    dom.set_property(frame, "ScrollingDirection", Variant::Enum(2))
        .unwrap();
    let y_only = resolve(&screens(&dom), VIEWPORT)[0].scroll.unwrap();
    assert_eq!(y_only.range, [0.0, 200.0]);

    dom.set_property(frame, "ScrollingEnabled", Variant::Bool(false))
        .unwrap();
    let disabled = resolve(&screens(&dom), VIEWPORT)[0].scroll.unwrap();
    assert_eq!(disabled.range, [0.0, 0.0], "off, however big the canvas");
}

// A list inside a list is clipped to the outer window, and its own window
// carries that clip so a wheel over where it *would* be finds nothing.
#[test]
fn a_nested_frame_carries_its_parents_window_as_its_clip() {
    let (mut dom, gui) = screen_gui();
    let outer = window(&mut dom, gui);
    canvas(&mut dom, outer, 200, 300);
    let inner = scrolling_frame(
        &mut dom,
        outer,
        udim2(0.0, 0, 0.0, 150),
        udim2(0.0, 100, 0.0, 100),
    );
    canvas(&mut dom, inner, 100, 500);

    let elements = resolve(&screens(&dom), VIEWPORT);
    let nested = elements[1].scroll.unwrap();
    assert_eq!(nested.referent, inner);
    assert_eq!(nested.rect, rect(0.0, 150.0, 100.0, 100.0));
    assert_eq!(nested.clip, Some(rect(0.0, 0.0, 200.0, 100.0)));
    assert_eq!(nested.range, [0.0, 400.0]);
}
