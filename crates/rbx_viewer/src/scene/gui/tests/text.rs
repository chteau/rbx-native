//! What a text object's properties are read into, and how `TextScaled` and
//! `AutomaticSize` settle its box against a measure with a known answer.

use rbx_dom::{Font, FontStyle};

use super::*;
use crate::fonts::Face;
use crate::scene::gui::{resolve_with, Align, Text, TextMeasure};

fn label(dom: &mut WeakDom, parent: Ref, class: &str, text: &str) -> Ref {
    let referent = dom.new_instance(class, class, Some(parent));
    dom.set_property(referent, "Text", Variant::String(text.to_string()))
        .unwrap();
    dom.set_property(referent, "Size", udim2(0.0, 100, 0.0, 20))
        .unwrap();
    referent
}

fn text_of(dom: &WeakDom) -> Text {
    resolve(&screens(dom), VIEWPORT)
        .remove(0)
        .text
        .expect("a text class carries text")
        .text
}

/// Every character `size / 2` wide and every line `size * LineHeight` tall,
/// wrapping as many whole characters per line as `max_width` holds.
struct Fixed;

impl TextMeasure for Fixed {
    fn measure(&mut self, text: &Text, size: f32, max_width: Option<f32>) -> [f32; 2] {
        let advance = size * 0.5;
        let characters = text
            .spans
            .iter()
            .map(|span| span.text.chars().count())
            .sum::<usize>() as f32;
        let per_line = match max_width {
            Some(width) => (width / advance).floor().max(1.0),
            None => characters,
        };
        let lines = (characters / per_line).ceil().max(1.0);
        [
            characters.min(per_line) * advance,
            lines * size * text.line_height,
        ]
    }
}

#[test]
fn a_text_label_reads_every_text_property() {
    let (mut dom, gui) = screen_gui();
    let referent = label(&mut dom, gui, "TextLabel", "Hi\tthere");
    let set = |dom: &mut WeakDom, name: &str, value: Variant| {
        dom.set_property(referent, name, value).unwrap();
    };
    set(
        &mut dom,
        "TextColor3",
        Variant::Color3(Color3Data {
            r: 1.0,
            g: 1.0,
            b: 1.0,
        }),
    );
    set(&mut dom, "TextTransparency", Variant::Float32(0.25));
    set(&mut dom, "TextSize", Variant::Float32(24.0));
    set(&mut dom, "TextScaled", Variant::Bool(true));
    set(&mut dom, "TextWrapped", Variant::Bool(true));
    set(&mut dom, "TextXAlignment", Variant::Enum(0));
    set(&mut dom, "TextYAlignment", Variant::Enum(2));
    set(
        &mut dom,
        "FontFace",
        Variant::Font(Font {
            family: "rbxasset://fonts/families/GothamSSm.json".to_string(),
            weight: 700,
            style: FontStyle::Italic,
            cached_face_id: None,
        }),
    );
    set(
        &mut dom,
        "TextStrokeColor3",
        Variant::Color3(Color3Data {
            r: 1.0,
            g: 0.0,
            b: 0.0,
        }),
    );
    set(&mut dom, "TextStrokeTransparency", Variant::Float32(0.5));
    set(&mut dom, "LineHeight", Variant::Float32(1.5));
    set(&mut dom, "TextTruncate", Variant::Enum(1));
    set(&mut dom, "MaxVisibleGraphemes", Variant::Int32(3));
    set(&mut dom, "AutomaticSize", Variant::Enum(2));

    let text = text_of(&dom);

    assert_eq!(text.spans.len(), 1);
    assert_eq!(text.spans[0].text, "Hi there", "a tab renders as a space");
    assert_eq!(text.color, [1.0; 3]);
    assert_eq!(text.alpha, 0.75);
    assert_eq!(text.size, 24.0);
    assert!(text.scaled && text.wrapped);
    assert_eq!((text.x_align, text.y_align), (Align::Start, Align::End));
    assert_eq!(text.face, Face::named("GothamSSm", 700, true));
    assert_eq!(text.stroke, Some(([1.0, 0.0, 0.0], 0.5)));
    assert_eq!(text.line_height, 1.5);
    assert!(text.truncate);
    assert_eq!(text.max_graphemes, Some(3));
    assert_eq!(text.automatic, [false, true]);
    assert_eq!(text.size_bounds, None);
}

#[test]
fn a_text_object_with_nothing_set_takes_roblox_defaults() {
    let (mut dom, gui) = screen_gui();
    label(&mut dom, gui, "TextButton", "Button");

    let text = text_of(&dom);

    assert_eq!(text.size, 14.0);
    assert_eq!(text.alpha, 1.0);
    assert_eq!((text.x_align, text.y_align), (Align::Center, Align::Center));
    assert_eq!(text.face, Face::named("SourceSansPro", 400, false));
    assert!(!text.scaled && !text.wrapped && !text.truncate);
    assert_eq!(text.stroke, None);
    assert_eq!(text.line_height, 1.0);
    assert_eq!(text.max_graphemes, None);
    assert_eq!(text.automatic, [false, false]);
    assert!(text.visible());
}

#[test]
fn a_legacy_font_enum_maps_to_its_family_weight_and_style() {
    let (mut dom, gui) = screen_gui();
    let referent = label(&mut dom, gui, "TextLabel", "x");
    let font_of = |dom: &mut WeakDom, font: u32| {
        dom.set_property(referent, "Font", Variant::Enum(font))
            .unwrap();
        text_of(dom).face
    };

    assert_eq!(
        font_of(&mut dom, 3),
        Face::named("SourceSansPro", 400, false)
    );
    assert_eq!(
        font_of(&mut dom, 4),
        Face::named("SourceSansPro", 700, false)
    );
    assert_eq!(
        font_of(&mut dom, 6),
        Face::named("SourceSansPro", 400, true)
    );
    assert_eq!(font_of(&mut dom, 19), Face::named("Montserrat", 700, false));
    assert_eq!(font_of(&mut dom, 26), Face::named("FredokaOne", 400, false));
    assert_eq!(font_of(&mut dom, 100), Face::default(), "Unknown");

    // `FontFace` wins over the enum wherever a place carries both.
    dom.set_property(
        referent,
        "FontFace",
        Variant::Font(Font {
            family: "rbxasset://fonts/families/Nunito.json".to_string(),
            weight: 300,
            style: FontStyle::Normal,
            cached_face_id: None,
        }),
    )
    .unwrap();
    assert_eq!(text_of(&dom).face, Face::named("Nunito", 300, false));
}

#[test]
fn a_text_box_shows_its_placeholder_only_while_empty() {
    let (mut dom, gui) = screen_gui();
    let referent = label(&mut dom, gui, "TextBox", "");
    dom.set_property(
        referent,
        "PlaceholderText",
        Variant::String("Type here".to_string()),
    )
    .unwrap();
    dom.set_property(
        referent,
        "PlaceholderColor3",
        Variant::Color3(Color3Data {
            r: 0.0,
            g: 1.0,
            b: 0.0,
        }),
    )
    .unwrap();
    dom.set_property(
        referent,
        "TextColor3",
        Variant::Color3(Color3Data {
            r: 1.0,
            g: 1.0,
            b: 1.0,
        }),
    )
    .unwrap();

    let empty = text_of(&dom);
    assert_eq!(empty.spans[0].text, "Type here");
    assert_eq!(empty.color, [0.0, 1.0, 0.0]);

    dom.set_property(referent, "Text", Variant::String("typed".to_string()))
        .unwrap();
    let typed = text_of(&dom);
    assert_eq!(typed.spans[0].text, "typed");
    assert_eq!(typed.color, [1.0; 3]);

    // A `TextLabel` has no placeholder rule: empty stays empty.
    let plain = label(&mut dom, gui, "TextLabel", "");
    dom.set_property(plain, "PlaceholderText", Variant::String("no".to_string()))
        .unwrap();
    let elements = resolve(&screens(&dom), VIEWPORT);
    let text = elements[1].text.as_ref().unwrap();
    assert_eq!(text.text.spans[0].text, "");
    assert!(!text.text.visible());
}

#[test]
fn markup_is_parsed_only_with_rich_text_on() {
    let (mut dom, gui) = screen_gui();
    let referent = label(&mut dom, gui, "TextLabel", "a <b>b</b>");

    let literal = text_of(&dom);
    assert_eq!(literal.spans.len(), 1);
    assert_eq!(literal.spans[0].text, "a <b>b</b>");

    dom.set_property(referent, "RichText", Variant::Bool(true))
        .unwrap();
    let rich = text_of(&dom);
    assert_eq!(rich.spans.len(), 2);
    assert_eq!(rich.spans[1].text, "b");
    assert!(rich.spans[1].bold);
    // The bold span is one more face to fetch.
    let mut faces = Vec::new();
    rich.faces(&mut faces);
    assert_eq!(
        faces,
        vec![Face::default(), Face::named("SourceSansPro", 700, false)]
    );
}

#[test]
fn only_a_text_class_carries_text_and_only_visible_text_paints() {
    let (mut dom, gui) = screen_gui();
    let zero = udim2(0.0, 0, 0.0, 0);
    let empty = frame(&mut dom, gui, zero.clone(), zero);
    dom.set_property(empty, "BackgroundTransparency", Variant::Float32(1.0))
        .unwrap();
    let transparent = label(&mut dom, gui, "TextLabel", "hidden");
    dom.set_property(transparent, "BackgroundTransparency", Variant::Float32(1.0))
        .unwrap();
    dom.set_property(transparent, "TextTransparency", Variant::Float32(1.0))
        .unwrap();
    let shown = label(&mut dom, gui, "TextLabel", "shown");
    dom.set_property(shown, "BackgroundTransparency", Variant::Float32(1.0))
        .unwrap();

    let screens = screens(&dom);
    let elements = resolve(&screens, VIEWPORT);
    assert!(elements[0].text.is_none());
    assert!(elements[1].text.is_some());

    // A tree whose only paint is text still needs a canvas.
    let roots = &screens[0].roots;
    assert!(!roots[0].paints());
    assert!(!roots[1].paints(), "fully transparent text paints nothing");
    assert!(roots[2].paints());
}

#[test]
fn text_scaled_picks_the_largest_whole_size_that_fits() {
    let (mut dom, gui) = screen_gui();
    let referent = label(&mut dom, gui, "TextLabel", "abcd");
    dom.set_property(referent, "TextScaled", Variant::Bool(true))
        .unwrap();

    // 100 by 20: four characters are 2s wide (fits up to 50) and s tall.
    let elements = resolve_with(&screens(&dom), VIEWPORT, &mut Fixed);
    assert_eq!(elements[0].text.as_ref().unwrap().size, 20.0);

    // Twenty characters in 100 by 40 wrap: ten per line at 20, two lines
    // of 20 fit exactly; at 21 only nine fit per line and three lines do not.
    dom.set_property(
        referent,
        "Text",
        Variant::String("abcdefghijklmnopqrst".to_string()),
    )
    .unwrap();
    dom.set_property(referent, "Size", udim2(0.0, 100, 0.0, 40))
        .unwrap();
    let elements = resolve_with(&screens(&dom), VIEWPORT, &mut Fixed);
    assert_eq!(elements[0].text.as_ref().unwrap().size, 20.0);
}

#[test]
fn size_bounds_clamp_both_a_scaled_and_a_plain_size() {
    let (mut dom, gui) = screen_gui();
    let referent = label(&mut dom, gui, "TextLabel", "abcd");
    dom.set_property(referent, "TextScaled", Variant::Bool(true))
        .unwrap();
    let mut screens = screens(&dom);
    let text = screens[0].roots[0].text.as_mut().unwrap();
    text.size_bounds = Some((5.0, 10.0));

    let scaled = resolve_with(&screens, VIEWPORT, &mut Fixed);
    assert_eq!(scaled[0].text.as_ref().unwrap().size, 10.0);

    let text = screens[0].roots[0].text.as_mut().unwrap();
    text.scaled = false;
    let plain = resolve_with(&screens, VIEWPORT, &mut Fixed);
    assert_eq!(plain[0].text.as_ref().unwrap().size, 10.0);
}

#[test]
fn automatic_size_grows_the_box_to_the_text_along_its_axes() {
    let (mut dom, gui) = screen_gui();
    let referent = label(&mut dom, gui, "TextLabel", "hello");
    dom.set_property(referent, "TextSize", Variant::Float32(10.0))
        .unwrap();
    dom.set_property(referent, "Size", udim2(0.0, 4, 0.0, 4))
        .unwrap();
    let grown = |dom: &mut WeakDom, automatic: u32| {
        dom.set_property(referent, "AutomaticSize", Variant::Enum(automatic))
            .unwrap();
        resolve_with(&screens(dom), VIEWPORT, &mut Fixed)[0].rect
    };

    // Five characters at 10: 25 wide, one line of 10 tall.
    assert_eq!(grown(&mut dom, 3).size(), [25.0, 10.0]);
    assert_eq!(grown(&mut dom, 1).size(), [25.0, 4.0]);
    assert_eq!(grown(&mut dom, 2).size(), [4.0, 10.0]);
    assert_eq!(grown(&mut dom, 0).size(), [4.0, 4.0]);

    // A box already bigger than its text keeps its size.
    dom.set_property(referent, "Size", udim2(0.0, 80, 0.0, 30))
        .unwrap();
    assert_eq!(grown(&mut dom, 3).size(), [80.0, 30.0]);
}

#[test]
fn an_unmeasured_resolve_keeps_text_size_as_written() {
    let (mut dom, gui) = screen_gui();
    let referent = label(&mut dom, gui, "TextLabel", "abcd");
    dom.set_property(referent, "TextSize", Variant::Float32(18.0))
        .unwrap();

    let elements = resolve(&screens(&dom), VIEWPORT);

    let typeset = elements[0].text.as_ref().unwrap();
    assert_eq!(typeset.size, 18.0);
    assert_eq!(
        elements[0].rect,
        Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 20.0
        }
    );
}

#[test]
fn a_ui_text_size_constraint_bounds_the_text_beside_it() {
    let (mut dom, gui) = screen_gui();
    let referent = label(&mut dom, gui, "TextLabel", "abcd");
    dom.set_property(referent, "TextScaled", Variant::Bool(true))
        .unwrap();
    let constraint = dom.new_instance(
        "UITextSizeConstraint",
        "UITextSizeConstraint",
        Some(referent),
    );
    dom.set_property(constraint, "MinTextSize", Variant::Int32(5))
        .unwrap();
    dom.set_property(constraint, "MaxTextSize", Variant::Int32(10))
        .unwrap();

    // `Fixed` would fit "abcd" at 40px in a 100x20 box; the constraint caps it.
    let scaled = resolve_with(&screens(&dom), VIEWPORT, &mut Fixed);
    assert_eq!(scaled[0].text.as_ref().unwrap().size, 10.0);
}
