use serde_json::{json, Value};

use super::*;

fn bounds(x: f64, y: f64, w: f64, h: f64) -> Value {
    json!({ "x": x, "y": y, "width": w, "height": h })
}

fn frame(name: &str, children: Vec<Value>) -> Value {
    json!({
        "id": "1:1", "type": "FRAME", "name": name,
        "absoluteBoundingBox": bounds(0.0, 0.0, 400.0, 300.0),
        "fills": [{ "type": "SOLID", "color": { "r": 1.0, "g": 1.0, "b": 1.0, "a": 1.0 } }],
        "children": children,
    })
}

fn label(name: &str, x: f64, y: f64) -> Value {
    json!({
        "id": "2:1", "type": "TEXT", "name": name, "characters": "Play",
        "absoluteBoundingBox": bounds(x, y, 80.0, 20.0),
        "constraints": { "horizontal": "LEFT", "vertical": "TOP" },
        "fills": [{ "type": "SOLID", "color": { "r": 0.0, "g": 0.0, "b": 0.0, "a": 1.0 } }],
        "style": { "fontFamily": "Inter", "fontWeight": 650, "fontSize": 18,
                   "textAlignHorizontal": "CENTER", "textAlignVertical": "CENTER" },
    })
}

fn only(node: &Node, class: &str) -> Node {
    let found: Vec<&Node> = node.children.iter().filter(|c| c.class == class).collect();
    assert_eq!(found.len(), 1, "one {class} under {}", node.name);
    found[0].clone()
}

#[test]
fn a_click_reaction_makes_a_text_button() {
    let mut button = frame("Play", vec![label("Label", 20.0, 10.0)]);
    button["reactions"] = json!([{ "trigger": { "type": "ON_CLICK" }, "actions": [] }]);
    button["cornerRadius"] = json!(8);
    let root = infer(&frame("Menu", vec![button])).unwrap();
    let button = &root.children[0];
    assert_eq!(button.class, "TextButton");
    assert_eq!(button.get("Text"), Some(&Variant::String(String::new())));
    assert_eq!(
        only(button, "UICorner").get("CornerRadius"),
        Some(&Variant::UDim(UDim {
            scale: 0.0,
            offset: 8
        }))
    );
    assert_eq!(
        only(button, "TextLabel").get("Text"),
        Some(&Variant::String("Play".into()))
    );
    assert!(button.review.is_none());

    let mut icon_button = frame("Close", vec![]);
    icon_button["interactions"] = json!([{ "trigger": { "type": "ON_PRESS" } }]);
    assert_eq!(infer(&icon_button).unwrap().class, "ImageButton");
}

#[test]
fn text_keeps_its_words_alignment_and_font() {
    let root = infer(&frame("Menu", vec![label("Title", 10.0, 20.0)])).unwrap();
    let text = &root.children[0];
    assert_eq!(text.class, "TextLabel");
    assert_eq!(text.get("TextSize"), Some(&Variant::Float32(18.0)));
    assert_eq!(text.get("TextXAlignment"), Some(&Variant::Enum(2)));
    assert_eq!(text.get("TextYAlignment"), Some(&Variant::Enum(1)));
    assert_eq!(
        text.get("Position"),
        Some(&Variant::UDim2(UDim2 {
            x: udim(0.0, 10.0),
            y: udim(0.0, 20.0)
        }))
    );
    let Some(Variant::Font(font)) = text.get("FontFace") else {
        panic!("no font")
    };
    assert_eq!(font.family, "rbxasset://fonts/families/BuilderSans.json");
    assert_eq!(font.weight, 700);

    let mut odd = label("Title", 0.0, 0.0);
    odd["style"]["fontFamily"] = json!("Papyrus");
    let odd = infer(&odd).unwrap();
    assert!(odd.review.unwrap().contains("Papyrus"));

    let field = infer(&frame("Email input", vec![label("Hint", 0.0, 0.0)])).unwrap();
    let field = &field.children[0];
    assert_eq!(field.class, "TextBox");
    assert_eq!(
        field.get("PlaceholderText"),
        Some(&Variant::String("Play".into()))
    );
    assert!(field.review.is_some());
}

#[test]
fn an_image_fill_is_an_image_label_to_upload() {
    let mut photo = frame("Avatar", vec![]);
    photo["fills"] = json!([{ "type": "IMAGE", "scaleMode": "FIT", "imageRef": "abc123" }]);
    let photo = infer(&photo).unwrap();
    assert_eq!(photo.class, "ImageLabel");
    assert_eq!(photo.get("ScaleType"), Some(&Variant::Enum(3)));
    assert_eq!(photo.image, Some(Image::Fill("abc123".into())));
}

#[test]
fn a_vector_icon_is_rendered_to_one_image() {
    let icon = json!({
        "id": "5:5", "type": "GROUP", "name": "Gear",
        "absoluteBoundingBox": bounds(10.0, 10.0, 24.0, 24.0),
        "children": [
            { "type": "VECTOR", "name": "Teeth", "absoluteBoundingBox": bounds(10.0, 10.0, 24.0, 24.0) },
            { "type": "ELLIPSE", "name": "Hole", "absoluteBoundingBox": bounds(18.0, 18.0, 8.0, 8.0) },
        ],
    });
    let root = infer(&frame("Bar", vec![icon])).unwrap();
    let icon = &root.children[0];
    assert_eq!(icon.class, "ImageLabel");
    assert!(icon.children.is_empty());
    assert_eq!(icon.image, Some(Image::Render("5:5".into())));
    assert_eq!(
        icon.get("BackgroundTransparency"),
        Some(&Variant::Float32(1.0))
    );
}

#[test]
fn auto_layout_becomes_a_list_layout_with_padding() {
    let mut list = frame("Buttons", vec![label("A", 0.0, 0.0), label("B", 0.0, 30.0)]);
    list["layoutMode"] = json!("VERTICAL");
    list["itemSpacing"] = json!(12);
    list["paddingLeft"] = json!(16);
    list["primaryAxisAlignItems"] = json!("SPACE_BETWEEN");
    list["counterAxisAlignItems"] = json!("CENTER");
    let list = infer(&list).unwrap();
    let layout = only(&list, "UIListLayout");
    assert_eq!(layout.get("FillDirection"), Some(&Variant::Enum(1)));
    assert_eq!(layout.get("Padding"), Some(&Variant::UDim(udim(0.0, 12.0))));
    assert_eq!(layout.get("HorizontalAlignment"), Some(&Variant::Enum(0)));
    assert_eq!(layout.get("VerticalFlex"), Some(&Variant::Enum(3)));
    assert_eq!(
        only(&list, "UIPadding").get("PaddingLeft"),
        Some(&Variant::UDim(udim(0.0, 16.0)))
    );
    let orders: Vec<_> = list
        .children
        .iter()
        .filter_map(|c| c.get("LayoutOrder"))
        .collect();
    assert_eq!(orders, [&Variant::Int32(1), &Variant::Int32(2)]);
}

#[test]
fn a_clipping_frame_that_overflows_scrolls() {
    let mut scroll = frame("Shop", vec![label("Far", 0.0, 900.0)]);
    scroll["clipsContent"] = json!(true);
    let scroll = infer(&scroll).unwrap();
    assert_eq!(scroll.class, "ScrollingFrame");
    assert_eq!(scroll.get("CanvasSize"), Some(&udim2(400.0, 920.0)));

    let mut hidden = frame("Shop", vec![label("Gone", 0.0, 0.0)]);
    hidden["children"][0]["visible"] = json!(false);
    hidden["clipsContent"] = json!(true);
    let hidden = infer(&hidden).unwrap();
    assert_eq!(hidden.class, "Frame");
    assert!(hidden.children.is_empty());
}

#[test]
fn constraints_anchor_to_the_right_edge_and_stretch() {
    let mut text = label("Corner", 300.0, 0.0);
    text["constraints"] = json!({ "horizontal": "RIGHT", "vertical": "TOP_BOTTOM" });
    let root = infer(&frame("Menu", vec![text])).unwrap();
    let text = &root.children[0];
    assert_eq!(
        text.get("AnchorPoint"),
        Some(&Variant::Vector2(Vector2Data { x: 1.0, y: 0.0 }))
    );
    assert_eq!(
        text.get("Position"),
        Some(&Variant::UDim2(UDim2 {
            x: udim(1.0, -20.0),
            y: udim(0.0, 0.0)
        }))
    );
    assert_eq!(
        text.get("Size"),
        Some(&Variant::UDim2(UDim2 {
            x: udim(0.0, 80.0),
            y: udim(1.0, -280.0)
        }))
    );
}

/// Every property the importer can write exists on its class, with a value
/// of the kind the API dump declares, and every enum ordinal names an item.
#[test]
fn every_property_written_is_in_the_api_dump() {
    let db = rbx_reflection::ReflectionDatabase::embedded();
    let mut everything = frame("Root", vec![label("Email input", 0.0, 0.0)]);
    everything["cornerRadius"] = json!(4);
    everything["clipsContent"] = json!(true);
    everything["strokeWeight"] = json!(2);
    everything["strokes"] =
        json!([{ "type": "SOLID", "color": { "r": 0.0, "g": 0.0, "b": 0.0, "a": 1.0 } }]);
    everything["effects"] = json!([{ "type": "DROP_SHADOW", "color": { "r": 0.0, "g": 0.0, "b": 0.0, "a": 0.5 },
                                     "offset": { "x": 0, "y": 4 }, "radius": 8, "spread": 2 }]);
    everything["layoutMode"] = json!("HORIZONTAL");
    everything["layoutWrap"] = json!("WRAP");
    everything["primaryAxisAlignItems"] = json!("SPACE_BETWEEN");
    everything["paddingTop"] = json!(4);
    everything["fills"] = json!([{ "type": "GRADIENT_LINEAR",
        "gradientHandlePositions": [{ "x": 0, "y": 0 }, { "x": 1, "y": 1 }],
        "gradientStops": [{ "position": 0.2, "color": { "r": 1, "g": 0, "b": 0, "a": 1 } },
                          { "position": 0.8, "color": { "r": 0, "g": 0, "b": 1, "a": 0.5 } }] }]);
    let mut image = frame("Photo", vec![]);
    image["fills"] = json!([{ "type": "IMAGE", "scaleMode": "TILE", "imageRef": "r" }]);
    image["reactions"] = json!([{ "trigger": { "type": "ON_CLICK" } }]);
    let mut photo = frame("Photo", vec![]);
    photo["fills"] = json!([{ "type": "IMAGE", "imageRef": "r" }]);
    let mut button = frame("Play", vec![label("Label", 0.0, 0.0)]);
    button["reactions"] = json!([{ "trigger": { "type": "ON_CLICK" } }]);
    let mut overflow = frame("List", vec![label("Far", 0.0, 900.0)]);
    overflow["clipsContent"] = json!(true);
    let vector = json!({ "id": "9:9", "type": "VECTOR", "name": "V", "absoluteBoundingBox": bounds(0.0, 0.0, 4.0, 4.0) });
    let ellipse = json!({ "type": "ELLIPSE", "name": "Dot", "absoluteBoundingBox": bounds(0.0, 0.0, 4.0, 4.0),
                          "fills": [{ "type": "SOLID", "color": { "r": 1, "g": 1, "b": 1, "a": 1 } }],
                          "children": [] });
    everything["children"] = json!([
        label("Title", 0.0, 0.0),
        label("Email input", 0.0, 0.0),
        image,
        photo,
        button,
        overflow,
        vector,
        ellipse
    ]);

    let mut root = infer(&everything).unwrap();
    let mut classes = std::collections::BTreeSet::new();
    root.walk_mut(&mut |node| {
        classes.insert(node.class);
        for (name, value) in &node.properties {
            let property = db
                .resolve_property(node.class, name)
                .unwrap_or_else(|| panic!("{}.{name} isn't in the dump", node.class));
            let kind = property.value_type.as_str();
            let ok = match value {
                Variant::String(_) => matches!(kind, "string" | "ContentId"),
                Variant::Bool(_) => kind == "bool",
                Variant::Float32(_) => kind == "float",
                Variant::Int32(_) => kind == "int",
                Variant::Color3(_) => kind == "Color3",
                Variant::UDim(_) => kind == "UDim",
                Variant::UDim2(_) => kind == "UDim2",
                Variant::Vector2(_) => kind == "Vector2",
                Variant::Font(_) => kind == "Font",
                Variant::ColorSequence(_) => kind == "ColorSequence",
                Variant::NumberSequence(_) => kind == "NumberSequence",
                Variant::Enum(ordinal) => db.enum_name(kind, *ordinal).is_some(),
                other => panic!("{}.{name} has an unchecked kind {other:?}", node.class),
            };
            assert!(
                ok,
                "{}.{name} is a {kind} in the dump, not {value:?}",
                node.class
            );
        }
    });
    for class in [
        "Frame",
        "TextLabel",
        "TextBox",
        "TextButton",
        "ImageButton",
        "ImageLabel",
        "ScrollingFrame",
        "UICorner",
        "UIStroke",
        "UIShadow",
        "UIGradient",
        "UIListLayout",
        "UIPadding",
    ] {
        assert!(classes.contains(class), "the fixture never made a {class}");
    }
}
