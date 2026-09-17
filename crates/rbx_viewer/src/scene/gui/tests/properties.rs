//! What is read out of a DOM, and what is deliberately not: enabled flags,
//! colours, borders, the unknown-class fallback and the text classes' boxes.

use super::*;

#[test]
fn a_disabled_screen_gui_contributes_nothing() {
    let (mut dom, gui) = screen_gui();
    dom.set_property(gui, "Enabled", Variant::Bool(false))
        .unwrap();
    frame(
        &mut dom,
        gui,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 10, 0.0, 10),
    );

    assert!(resolve(&screens(&dom), VIEWPORT).is_empty());
}

#[test]
fn an_invisible_frame_takes_its_whole_subtree_with_it() {
    let (mut dom, gui) = screen_gui();
    let zero = udim2(0.0, 0, 0.0, 0);
    let hidden = frame(&mut dom, gui, zero.clone(), zero.clone());
    frame(&mut dom, hidden, zero.clone(), zero.clone());
    dom.set_property(hidden, "Visible", Variant::Bool(false))
        .unwrap();

    assert!(resolve(&screens(&dom), VIEWPORT).is_empty());
}

#[test]
fn colour_is_linearized_and_transparency_becomes_alpha() {
    let (mut dom, gui) = screen_gui();
    let zero = udim2(0.0, 0, 0.0, 0);
    let painted = frame(&mut dom, gui, zero.clone(), zero.clone());
    dom.set_property(
        painted,
        "BackgroundColor3",
        Variant::Color3(Color3Data {
            r: 1.0,
            g: 0.5,
            b: 0.0,
        }),
    )
    .unwrap();
    dom.set_property(painted, "BackgroundTransparency", Variant::Float32(0.25))
        .unwrap();

    let elements = resolve(&screens(&dom), VIEWPORT);

    assert_eq!(elements[0].background[0], 1.0);
    assert!((elements[0].background[1] - 0.2140).abs() < 1e-3);
    assert_eq!(elements[0].background[2], 0.0);
    assert_eq!(elements[0].background_alpha, 0.75);
}

#[test]
fn a_border_is_dropped_only_when_it_has_no_width() {
    let (mut dom, gui) = screen_gui();
    let zero = udim2(0.0, 0, 0.0, 0);
    let bare = frame(&mut dom, gui, zero.clone(), zero.clone());
    let outlined = frame(&mut dom, gui, zero.clone(), zero.clone());
    dom.set_property(outlined, "BorderSizePixel", Variant::Int32(3))
        .unwrap();
    dom.set_property(
        outlined,
        "BorderColor3",
        Variant::Color3(Color3Data {
            r: 0.0,
            g: 0.0,
            b: 1.0,
        }),
    )
    .unwrap();
    let _ = bare;

    let elements = resolve(&screens(&dom), VIEWPORT);

    assert_eq!(elements[0].border, None);
    assert_eq!(elements[1].border, Some((3.0, [0.0, 0.0, 1.0])));
}

// An unknown `GuiObject` subclass must still show: an invisible element is a

// far worse failure mode than a plain coloured box.

#[test]
fn a_gui_object_class_with_no_special_handling_is_drawn_like_a_frame() {
    let (mut dom, gui) = screen_gui();
    let odd = dom.new_instance("ViewportFrame", "ViewportFrame", Some(gui));
    dom.set_property(odd, "Position", udim2(0.0, 20, 0.0, 30))
        .unwrap();
    dom.set_property(odd, "Size", udim2(0.0, 40, 0.0, 50))
        .unwrap();

    let elements = resolve(&screens(&dom), VIEWPORT);

    assert_eq!(
        elements[0].rect,
        Rect {
            x: 20.0,
            y: 30.0,
            width: 40.0,
            height: 50.0,
        }
    );
}

// A `TextLabel`'s background is a plain box like any other, and is often the
// only thing painted over a part's face; its text rides on the same element
// (see `tests::text` for what is read into it).

#[test]
fn text_classes_draw_their_background_and_carry_their_text() {
    let (mut dom, gui) = screen_gui();
    for class in ["TextLabel", "TextButton", "TextBox"] {
        let referent = dom.new_instance(class, class, Some(gui));
        dom.set_property(referent, "Size", udim2(0.0, 10, 0.0, 10))
            .unwrap();
    }

    let elements = resolve(&screens(&dom), VIEWPORT);
    assert_eq!(elements.len(), 3);
    for element in &elements {
        assert_eq!(element.rect.size(), [10.0, 10.0]);
        assert_eq!(element.background_alpha, 1.0);
        assert!(element.image.is_none());
        assert!(element.text.is_some());
    }
}

// A GUI instance outside any `ScreenGui` — `BubbleChatConfiguration`'s own

// `ImageLabel`, for one — is not part of any screen and must not be drawn.

#[test]
fn a_gui_object_outside_a_screen_gui_is_not_a_screen_of_its_own() {
    let mut dom = WeakDom::new();
    let holder = dom.new_instance("Folder", "Folder", None);
    frame(
        &mut dom,
        holder,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 10, 0.0, 10),
    );

    assert!(resolve(&screens(&dom), VIEWPORT).is_empty());
}

#[test]
fn a_stretched_image_repeats_once_and_a_tiled_one_by_its_tile_size() {
    let (mut dom, gui) = screen_gui();
    let size = udim2(0.0, 300, 0.0, 200);
    for (name, tiled) in [("stretch", false), ("tile", true)] {
        let label = dom.new_instance("ImageLabel", name, Some(gui));
        dom.set_property(label, "Size", size.clone()).unwrap();
        dom.set_property(
            label,
            "Image",
            Variant::String("rbxassetid://12345".to_string()),
        )
        .unwrap();
        if tiled {
            dom.set_property(label, "ScaleType", Variant::Enum(2))
                .unwrap();
            dom.set_property(label, "TileSize", udim2(0.0, 100, 0.5, 0))
                .unwrap();
        }
    }

    let elements = resolve(&screens(&dom), VIEWPORT);

    let stretch = elements[0].image.as_ref().unwrap();
    assert_eq!(stretch.repeat, [1.0, 1.0]);
    let tile = elements[1].image.as_ref().unwrap();
    // 300 px across 100 px tiles, and 200 px across tiles half its own height.
    assert_eq!(tile.repeat, [3.0, 2.0]);
}

#[test]
fn rotation_is_read_verbatim_in_degrees() {
    let (mut dom, gui) = screen_gui();
    let spun = frame(
        &mut dom,
        gui,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 10, 0.0, 10),
    );
    dom.set_property(spun, "Rotation", Variant::Float32(33.5))
        .unwrap();

    let elements = resolve(&screens(&dom), VIEWPORT);

    assert_eq!(elements[0].rotation, 33.5);
}

#[test]
fn an_unset_rotation_defaults_to_zero() {
    let (mut dom, gui) = screen_gui();
    frame(
        &mut dom,
        gui,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 10, 0.0, 10),
    );

    let elements = resolve(&screens(&dom), VIEWPORT);

    assert_eq!(elements[0].rotation, 0.0);
}

#[test]
fn an_empty_image_property_leaves_the_element_a_plain_box() {
    let (mut dom, gui) = screen_gui();
    let label = dom.new_instance("ImageLabel", "ImageLabel", Some(gui));
    dom.set_property(label, "Size", udim2(0.0, 10, 0.0, 10))
        .unwrap();
    dom.set_property(label, "Image", Variant::String(String::new()))
        .unwrap();

    let elements = resolve(&screens(&dom), VIEWPORT);

    assert_eq!(elements.len(), 1);
    assert!(elements[0].image.is_none());
}
