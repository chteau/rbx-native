//! Where an element lands rather than how big it is: the cumulative
//! rotation a nested element inherits, where `BorderMode` puts the border
//! bands, and the two `ScreenGui`-wide properties — `ScreenInsets` and
//! `ZIndexBehavior` — that decide the canvas and the paint order.

use super::*;

#[test]
fn a_nested_rotation_is_cumulative_and_carries_the_child_around_its_parent() {
    let (mut dom, gui) = screen_gui();
    // A 200x200 parent at the origin, turned a quarter turn about its centre
    // at (100, 100).
    let parent = frame(
        &mut dom,
        gui,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 200, 0.0, 200),
    );
    dom.set_property(parent, "Rotation", Variant::Float32(90.0))
        .unwrap();
    // A 20x20 child in the parent's own top-left corner, centre (10, 10).
    let child = frame(
        &mut dom,
        parent,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 20, 0.0, 20),
    );
    dom.set_property(child, "Rotation", Variant::Float32(30.0))
        .unwrap();

    let elements = resolve(&screens(&dom), VIEWPORT);
    assert_eq!(elements[0].rotation, 90.0);
    // Clockwise about (100, 100): (10, 10) lands on (190, 10).
    let child = elements[1].rect;
    assert!((child.x + 10.0 - 190.0).abs() < 1e-3);
    assert!((child.y + 10.0 - 10.0).abs() < 1e-3);
    // `AbsoluteRotation` is the sum, which the renderer then applies about the
    // child's own centre.
    assert_eq!(elements[1].rotation, 120.0);
}

#[test]
fn border_mode_decides_how_far_inside_the_box_the_bands_start() {
    let (mut dom, gui) = screen_gui();
    for mode in 0..3 {
        let referent = frame(
            &mut dom,
            gui,
            udim2(0.0, 0, 0.0, 0),
            udim2(0.0, 100, 0.0, 100),
        );
        dom.set_property(referent, "BorderSizePixel", Variant::Int32(4))
            .unwrap();
        dom.set_property(referent, "BorderMode", Variant::Enum(mode))
            .unwrap();
    }

    let insets: Vec<f32> = resolve(&screens(&dom), VIEWPORT)
        .iter()
        .map(|element| element.border_inset)
        .collect();
    // Outline grows outward only, Middle straddles the edge, Inset is wholly
    // inside.
    assert_eq!(insets, vec![0.0, 2.0, 4.0]);
}

#[test]
fn core_ui_safe_insets_start_the_canvas_below_the_top_bar() {
    let (mut dom, gui) = screen_gui();
    dom.set_property(gui, "ScreenInsets", Variant::Enum(2))
        .unwrap();
    frame(&mut dom, gui, udim2(0.0, 0, 0.0, 0), udim2(1.0, 0, 1.0, 0));

    let rect = rects(&dom)[0];
    assert_eq!(rect.y, 36.0);
    // And the canvas is that much shorter, so the bottom edge is unmoved.
    assert_eq!(rect.y + rect.height, VIEWPORT[1]);
}

#[test]
fn ignore_gui_inset_gives_the_whole_viewport_back() {
    let (mut dom, gui) = screen_gui();
    dom.set_property(gui, "ScreenInsets", Variant::Enum(2))
        .unwrap();
    dom.set_property(gui, "IgnoreGuiInset", Variant::Bool(true))
        .unwrap();
    frame(&mut dom, gui, udim2(0.0, 0, 0.0, 0), udim2(1.0, 0, 1.0, 0));

    let rect = rects(&dom)[0];
    assert_eq!((rect.y, rect.height), (0.0, VIEWPORT[1]));
}

/// A frame reaching above the safe area, so the clip either bites or does not.
fn spilling(dom: &mut WeakDom, gui: Ref) {
    dom.set_property(gui, "ScreenInsets", Variant::Enum(2))
        .unwrap();
    frame(dom, gui, udim2(0.0, 0, 0.0, -20), udim2(1.0, 0, 0.0, 100));
}

#[test]
fn clip_to_device_safe_area_scissors_the_screen_to_the_inset_canvas() {
    let (mut dom, gui) = screen_gui();
    spilling(&mut dom, gui);

    let clip = resolve(&screens(&dom), VIEWPORT)[0].clip.expect("clipped");
    assert_eq!(clip.y, 36.0);
    assert_eq!(clip.height, VIEWPORT[1] - 36.0);
}

#[test]
fn clip_to_device_safe_area_off_lets_the_screen_spill_past_the_inset() {
    let (mut dom, gui) = screen_gui();
    spilling(&mut dom, gui);
    dom.set_property(gui, "ClipToDeviceSafeArea", Variant::Bool(false))
        .unwrap();

    assert!(resolve(&screens(&dom), VIEWPORT)[0].clip.is_none());
}

/// "This property will be ignored if you set `ScreenInsets` to `None`."
#[test]
fn screen_insets_none_ignores_the_safe_area_clip() {
    let (mut dom, gui) = screen_gui();
    frame(
        &mut dom,
        gui,
        udim2(0.0, 0, 0.0, -20),
        udim2(1.0, 0, 0.0, 100),
    );
    dom.set_property(gui, "ClipToDeviceSafeArea", Variant::Bool(true))
        .unwrap();

    assert!(resolve(&screens(&dom), VIEWPORT)[0].clip.is_none());
}

#[test]
fn a_global_z_index_screen_sorts_descendants_against_each_other() {
    let (mut dom, gui) = screen_gui();
    dom.set_property(gui, "ZIndexBehavior", Variant::Enum(0))
        .unwrap();
    let low = frame(
        &mut dom,
        gui,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 10, 0.0, 10),
    );
    dom.set_property(low, "ZIndex", Variant::Int32(1)).unwrap();
    let under = frame(&mut dom, low, udim2(0.0, 1, 0.0, 1), udim2(0.0, 2, 0.0, 2));
    dom.set_property(under, "ZIndex", Variant::Int32(0))
        .unwrap();
    let high = frame(
        &mut dom,
        gui,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 20, 0.0, 20),
    );
    dom.set_property(high, "ZIndex", Variant::Int32(5)).unwrap();

    // A child with a lower `ZIndex` than its parent renders *under* it, which
    // `Sibling` can never do.
    let widths: Vec<f32> = rects(&dom).iter().map(|rect| rect.width).collect();
    assert_eq!(widths, vec![2.0, 10.0, 20.0]);
}

#[test]
fn a_sibling_z_index_screen_still_draws_every_child_over_its_parent() {
    let (mut dom, gui) = screen_gui();
    let parent = frame(
        &mut dom,
        gui,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 10, 0.0, 10),
    );
    dom.set_property(parent, "ZIndex", Variant::Int32(1))
        .unwrap();
    let child = frame(
        &mut dom,
        parent,
        udim2(0.0, 1, 0.0, 1),
        udim2(0.0, 2, 0.0, 2),
    );
    dom.set_property(child, "ZIndex", Variant::Int32(0))
        .unwrap();

    let widths: Vec<f32> = rects(&dom).iter().map(|rect| rect.width).collect();
    assert_eq!(widths, vec![10.0, 2.0]);
}
