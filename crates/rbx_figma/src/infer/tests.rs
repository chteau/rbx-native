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

fn rgb(r: f64, g: f64, b: f64, a: f64) -> Value {
    json!({ "r": r, "g": g, "b": b, "a": a })
}

fn solid(r: f64, g: f64, b: f64) -> Value {
    json!({ "type": "SOLID", "color": rgb(r, g, b, 1.0) })
}

/// Red to blue, left to right.
fn linear() -> Value {
    json!({ "type": "GRADIENT_LINEAR",
        "gradientHandlePositions": [{ "x": 0, "y": 0.5 }, { "x": 1, "y": 0.5 }, { "x": 0, "y": 1 }],
        "gradientStops": [{ "position": 0, "color": rgb(1.0, 0.0, 0.0, 1.0) },
                          { "position": 1, "color": rgb(0.0, 0.0, 1.0, 1.0) }] })
}

fn keys(g: &Node) -> (Vec<(f32, Color3Data)>, Vec<f32>) {
    let Some(Variant::ColorSequence(c)) = g.get("Color") else {
        panic!("no Color")
    };
    let Some(Variant::NumberSequence(t)) = g.get("Transparency") else {
        panic!("no Transparency")
    };
    (
        c.keypoints.iter().map(|k| (k.time, k.color)).collect(),
        t.keypoints.iter().map(|k| k.value).collect(),
    )
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-3
}

#[test]
fn a_wrapping_auto_layout_is_a_grid() {
    let mut grid = frame("Items", vec![label("A", 0.0, 0.0), label("B", 90.0, 0.0)]);
    grid["layoutMode"] = json!("HORIZONTAL");
    grid["layoutWrap"] = json!("WRAP");
    grid["itemSpacing"] = json!(8);
    grid["counterAxisSpacing"] = json!(12);
    grid["primaryAxisAlignItems"] = json!("MAX");
    let grid = infer(&grid).unwrap();
    assert!(grid.children.iter().all(|c| c.class != "UIListLayout"));
    let layout = only(&grid, "UIGridLayout");
    assert_eq!(layout.get("CellSize"), Some(&udim2(80.0, 20.0)));
    assert_eq!(layout.get("CellPadding"), Some(&udim2(8.0, 12.0)));
    assert_eq!(layout.get("FillDirection"), Some(&Variant::Enum(0)));
    assert_eq!(layout.get("SortOrder"), Some(&Variant::Enum(2)));
    assert_eq!(layout.get("HorizontalAlignment"), Some(&Variant::Enum(2)));
    assert_eq!(layout.get("VerticalAlignment"), Some(&Variant::Enum(1)));
}

#[test]
fn mixed_text_styles_become_rich_text() {
    let mut text = label("Price", 0.0, 0.0);
    // "A😀b<&": the emoji takes two UTF-16 slots, so "b<" is slots 3..5.
    text["characters"] = json!("A\u{1F600}b<&");
    text["characterStyleOverrides"] = json!([0, 0, 0, 1, 1]);
    text["styleOverrideTable"] = json!({ "1": {
        "fontWeight": 700, "italic": true, "textDecoration": "UNDERLINE",
        "fills": [solid(1.0, 0.0, 0.0)] } });
    let text = infer(&text).unwrap();
    assert_eq!(text.get("RichText"), Some(&Variant::Bool(true)));
    assert_eq!(
        text.get("Text"),
        Some(&Variant::String(
            "A\u{1F600}<font color=\"#ff0000\" weight=\"700\"><i><u>b&lt;</u></i></font>&amp;"
                .into()
        ))
    );
    // The base style stays on the label.
    let Some(Variant::Font(font)) = text.get("FontFace") else {
        panic!("no font")
    };
    assert_eq!(font.weight, 700);
    assert_eq!(
        text.get("TextColor3"),
        Some(&Variant::Color3(Color3Data {
            r: 0.0,
            g: 0.0,
            b: 0.0
        }))
    );

    let plain = infer(&label("Plain", 0.0, 0.0)).unwrap();
    assert_eq!(plain.get("RichText"), None);
    assert_eq!(escape("&<>\"'"), "&amp;&lt;&gt;&quot;&apos;");

    let mut upper = label("Upper", 0.0, 0.0);
    upper["style"]["textCase"] = json!("UPPER");
    upper["style"]["textDecoration"] = json!("STRIKETHROUGH");
    let upper = infer(&upper).unwrap();
    assert_eq!(
        upper.get("Text"),
        Some(&Variant::String("<s>PLAY</s>".into()))
    );
}

#[test]
fn rotation_is_clockwise_degrees_on_the_unrotated_box() {
    // Figma turns -30° (clockwise on screen): x axis lands at (cos, sin) y-down.
    let (s, c) = 30f64.to_radians().sin_cos();
    let (w, h) = (100.0 * c + 40.0 * s, 100.0 * s + 40.0 * c);
    let mut tilted = json!({ "id": "3:3", "type": "RECTANGLE", "name": "Tag",
        "absoluteBoundingBox": bounds(0.0, 0.0, w, h),
        "relativeTransform": [[c, -s, 20.0], [s, c, 0.0]],
        "size": { "x": 100, "y": 40 },
        "fills": [solid(1.0, 1.0, 1.0)] });
    let root = infer(&frame("Menu", vec![tilted.clone()])).unwrap();
    let tag = &root.children[0];
    let Some(Variant::Float32(rotation)) = tag.get("Rotation") else {
        panic!("no rotation")
    };
    assert!(close(*rotation, 30.0), "{rotation}");
    assert_eq!(tag.get("Size"), Some(&udim2(100.0, 40.0)));
    assert_eq!(
        tag.get("Position"),
        Some(&Variant::UDim2(UDim2 {
            x: udim(0.0, w / 2.0 - 50.0),
            y: udim(0.0, h / 2.0 - 20.0)
        }))
    );
    assert!(tag.review.as_deref().unwrap().contains("rotated"));
    assert_eq!(tag.confidence, Confidence::Medium);

    // Without `size`, the box is solved from the bounds.
    tilted.as_object_mut().unwrap().remove("size");
    let root = infer(&frame("Menu", vec![tilted])).unwrap();
    assert_eq!(root.children[0].get("Size"), Some(&udim2(100.0, 40.0)));
}

#[test]
fn fill_and_hug_sizing_in_auto_layout() {
    let mut grow = frame("Grow", vec![]);
    grow["layoutSizingHorizontal"] = json!("FILL");
    grow["layoutSizingVertical"] = json!("HUG");
    let mut stretch = frame("Stretch", vec![]);
    stretch["layoutAlign"] = json!("STRETCH");
    let mut row = frame("Row", vec![grow, stretch]);
    row["layoutMode"] = json!("HORIZONTAL");
    let row = infer(&row).unwrap();
    let grow = row.children.iter().find(|c| c.name == "Grow").unwrap();
    assert_eq!(
        only(grow, "UIFlexItem").get("FlexMode"),
        Some(&Variant::Enum(3))
    );
    assert_eq!(grow.get("AutomaticSize"), Some(&Variant::Enum(2)));
    let stretch = row.children.iter().find(|c| c.name == "Stretch").unwrap();
    assert_eq!(
        stretch.get("Size"),
        Some(&Variant::UDim2(UDim2 {
            x: udim(0.0, 400.0),
            y: udim(1.0, 0.0)
        }))
    );
    assert!(stretch.children.iter().all(|c| c.class != "UIFlexItem"));
}

#[test]
fn line_height_is_a_ratio_clamped_to_roblox_range() {
    let mut text = label("Body", 0.0, 0.0);
    text["style"]["lineHeightPx"] = json!(27);
    let text = infer(&text).unwrap();
    assert_eq!(text.get("LineHeight"), Some(&Variant::Float32(1.5)));
    assert!(text.review.is_none());

    let mut tall = label("Body", 0.0, 0.0);
    tall["style"]["lineHeightPercentFontSize"] = json!(400);
    let tall = infer(&tall).unwrap();
    assert_eq!(tall.get("LineHeight"), Some(&Variant::Float32(3.0)));
    assert!(tall.review.unwrap().contains("clamped"));
}

#[test]
fn text_scales_down_but_never_past_its_design_size() {
    let text = infer(&label("Title", 0.0, 0.0)).unwrap();
    assert_eq!(text.get("TextScaled"), Some(&Variant::Bool(true)));
    assert_eq!(text.get("TextSize"), Some(&Variant::Float32(18.0)));
    let cap = only(&text, "UITextSizeConstraint");
    assert_eq!(cap.get("MaxTextSize"), Some(&Variant::Int32(18)));
    assert_eq!(cap.get("MinTextSize"), Some(&Variant::Int32(1)));

    let mut hugging = label("Title", 0.0, 0.0);
    hugging["layoutSizingHorizontal"] = json!("HUG");
    let hugging = infer(&hugging).unwrap();
    assert_eq!(hugging.get("TextScaled"), None);
    assert_eq!(hugging.get("AutomaticSize"), Some(&Variant::Enum(1)));
}

#[test]
fn clipping_follows_clips_content_and_rounded_corners() {
    let open = infer(&frame("Open", vec![label("A", 0.0, 0.0)])).unwrap();
    assert_eq!(open.get("ClipsDescendants"), None);

    let mut card = frame("Card", vec![label("A", 0.0, 0.0)]);
    card["clipsContent"] = json!(true);
    card["cornerRadius"] = json!(16);
    let card = infer(&card).unwrap();
    assert_eq!(card.class, "CanvasGroup");
    assert_eq!(card.get("ClipsDescendants"), Some(&Variant::Bool(true)));
    assert!(card.review.unwrap().contains("CanvasGroup"));

    let mut inset = frame("Card", vec![label("A", 100.0, 100.0)]);
    inset["clipsContent"] = json!(true);
    inset["cornerRadius"] = json!(16);
    let inset = infer(&inset).unwrap();
    assert_eq!(inset.class, "Frame");
    assert_eq!(inset.get("ClipsDescendants"), Some(&Variant::Bool(true)));

    let mut button = frame("Buy", vec![label("A", 100.0, 100.0)]);
    button["clipsContent"] = json!(true);
    button["reactions"] = json!([{ "trigger": { "type": "ON_CLICK" } }]);
    let button = infer(&button).unwrap();
    assert_eq!(button.class, "TextButton");
    assert_eq!(button.get("ClipsDescendants"), Some(&Variant::Bool(true)));
}

#[test]
fn gradients_paint_backgrounds_text_and_strokes() {
    // Background: white under the gradient.
    let mut back = frame("Back", vec![]);
    back["fills"] = json!([linear()]);
    let back = infer(&back).unwrap();
    assert_eq!(back.get("BackgroundColor3"), Some(&Variant::Color3(WHITE)));
    let g = only(&back, "UIGradient");
    assert_eq!(g.get("Rotation"), Some(&Variant::Float32(0.0)));
    let (colors, _) = keys(&g);
    assert_eq!(colors.len(), 2);
    assert!(close(colors[0].0, 0.0) && colors[0].1.r == 1.0);
    assert!(close(colors[1].0, 1.0) && colors[1].1.b == 1.0);
    assert_eq!(back.confidence, Confidence::High);

    // Text: the rainbow header.
    let mut header = label("Header", 0.0, 0.0);
    header["fills"] = json!([linear()]);
    let header = infer(&header).unwrap();
    assert_eq!(header.get("TextColor3"), Some(&Variant::Color3(WHITE)));
    only(&header, "UIGradient");

    // Stroke: a gradient under the UIStroke.
    let mut ring = frame("Ring", vec![]);
    ring["strokes"] = json!([linear()]);
    ring["strokeWeight"] = json!(2);
    let ring = infer(&ring).unwrap();
    let stroke = only(&ring, "UIStroke");
    assert_eq!(stroke.get("Color"), Some(&Variant::Color3(WHITE)));
    only(&stroke, "UIGradient");
}

#[test]
fn gradient_handles_map_through_the_box_aspect() {
    // A diagonal on a 200×100 box, equal-colour lines perpendicular on
    // screen: the screen direction (200, 100) is (1, 0.25) in Roblox's
    // normalized space, about 14° clockwise.
    let fill = json!({ "type": "GRADIENT_LINEAR",
        "gradientHandlePositions": [{ "x": 0, "y": 0 }, { "x": 1, "y": 1 }],
        "gradientStops": [{ "position": 0, "color": rgb(1.0, 0.0, 0.0, 1.0) },
                          { "position": 1, "color": rgb(0.0, 0.0, 1.0, 1.0) }] });
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 200.0,
        h: 100.0,
    };
    let (g, concern) = gradient(&fill, rect);
    assert!(concern.is_none());
    let Some(Variant::Float32(rotation)) = g.get("Rotation") else {
        panic!("no rotation")
    };
    assert!(close(*rotation, 0.25f32.atan().to_degrees()), "{rotation}");
    // The corners stop at the gradient's ends: red at (0,0), blue at (1,1).
    let (colors, _) = keys(&g);
    assert_eq!(colors.first().unwrap().1.r, 1.0);
    assert_eq!(colors.last().unwrap().1.b, 1.0);

    // Vertical handles: 90°.
    let down = json!({ "type": "GRADIENT_LINEAR",
        "gradientHandlePositions": [{ "x": 0.5, "y": 0 }, { "x": 0.5, "y": 1 }, { "x": 1, "y": 0 }],
        "gradientStops": [{ "position": 0, "color": rgb(1.0, 0.0, 0.0, 1.0) },
                          { "position": 1, "color": rgb(0.0, 0.0, 1.0, 1.0) }] });
    let (g, _) = gradient(&down, rect);
    let Some(Variant::Float32(rotation)) = g.get("Rotation") else {
        panic!("no rotation")
    };
    assert!(close(*rotation, 90.0), "{rotation}");
}

#[test]
fn gradient_keypoints_are_capped_increasing_and_carry_opacity() {
    let stops: Vec<Value> = (0..30)
        .map(|i| json!({ "position": (i / 2) as f64 / 14.0, "color": rgb(1.0, 1.0, 1.0, 0.5) }))
        .collect();
    let fill = json!({ "type": "GRADIENT_RADIAL", "opacity": 0.5,
        "gradientHandlePositions": [{ "x": 0.5, "y": 0.5 }, { "x": 1, "y": 0.5 }, { "x": 0.5, "y": 1 }],
        "gradientStops": stops });
    let (g, concern) = gradient(
        &fill,
        Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        },
    );
    assert_eq!(concern.unwrap().1, Confidence::Medium);
    let (colors, transparency) = keys(&g);
    assert!(colors.len() <= 20);
    assert!(colors.windows(2).all(|w| w[0].0 < w[1].0));
    assert!(close(colors[0].0, 0.0) && close(colors.last().unwrap().0, 1.0));
    assert!(transparency.iter().all(|t| close(*t, 0.75)));

    // Equal positions are nudged apart.
    let fill = json!({ "type": "GRADIENT_LINEAR",
        "gradientHandlePositions": [{ "x": 0, "y": 0.5 }, { "x": 1, "y": 0.5 }, { "x": 0, "y": 1 }],
        "gradientStops": [{ "position": 0, "color": rgb(1.0, 0.0, 0.0, 1.0) },
                          { "position": 0.5, "color": rgb(1.0, 0.0, 0.0, 1.0) },
                          { "position": 0.5, "color": rgb(0.0, 0.0, 1.0, 1.0) },
                          { "position": 1, "color": rgb(0.0, 0.0, 1.0, 1.0) }] });
    let (g, _) = gradient(
        &fill,
        Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        },
    );
    let (colors, _) = keys(&g);
    assert_eq!(colors.len(), 4);
    assert!(colors.windows(2).all(|w| w[0].0 < w[1].0));

    // Angular: rendered on a leaf, Low on a container.
    let mut leaf = frame("Wheel", vec![]);
    leaf["fills"] = json!([{ "type": "GRADIENT_ANGULAR", "gradientStops": [] }]);
    let leaf = infer(&leaf).unwrap();
    assert_eq!(leaf.image, Some(Image::Render("1:1".into())));
    assert_eq!(leaf.confidence, Confidence::Low);
    let mut container = frame("Wheel", vec![label("A", 0.0, 0.0)]);
    container["fills"] = json!([{ "type": "GRADIENT_ANGULAR", "gradientStops": [] }]);
    let container = infer(&container).unwrap();
    assert_eq!(container.class, "Frame");
    assert_eq!(container.confidence, Confidence::Low);
}

#[test]
fn strokes_follow_glyphs_on_text_and_the_border_elsewhere() {
    let mut text = label("Title", 0.0, 0.0);
    text["strokes"] = json!([solid(0.0, 0.0, 0.0)]);
    text["strokeWeight"] = json!(2);
    text["strokeAlign"] = json!("OUTSIDE");
    let text = infer(&text).unwrap();
    let s = only(&text, "UIStroke");
    assert_eq!(s.get("ApplyStrokeMode"), Some(&Variant::Enum(0)));
    assert_eq!(s.get("BorderStrokePosition"), None);
    assert!(text.review.is_none());

    let mut card = frame("Card", vec![]);
    card["strokes"] = json!([solid(0.0, 0.0, 0.0)]);
    card["strokeWeight"] = json!(3);
    card["strokeAlign"] = json!("INSIDE");
    card["strokeDashes"] = json!([4, 2]);
    let card = infer(&card).unwrap();
    let s = only(&card, "UIStroke");
    assert_eq!(s.get("ApplyStrokeMode"), Some(&Variant::Enum(1)));
    assert_eq!(s.get("BorderStrokePosition"), Some(&Variant::Enum(2)));
    assert_eq!(s.get("Thickness"), Some(&Variant::Float32(3.0)));
    assert!(card.review.unwrap().contains("dashed"));
}

#[test]
fn an_image_fill_under_children_is_a_first_child_layer() {
    let mut card = frame("Card", vec![label("Price", 100.0, 100.0)]);
    card["cornerRadius"] = json!(8);
    card["fills"] = json!([{ "type": "IMAGE", "scaleMode": "TILE", "scalingFactor": 0.5,
                             "imageRef": "checker", "opacity": 0.5 }]);
    let card = infer(&card).unwrap();
    assert_eq!(card.class, "Frame");
    let layer = &card.children[0];
    assert_eq!(layer.class, "ImageLabel");
    assert_eq!(layer.name, "CardImage");
    assert_eq!(layer.id, "1:1#fill");
    assert_eq!(layer.image, Some(Image::Fill("checker".into())));
    assert_eq!(layer.get("Size"), Some(&full()));
    assert_eq!(layer.get("ZIndex"), Some(&Variant::Int32(0)));
    assert_eq!(
        layer.get("BackgroundTransparency"),
        Some(&Variant::Float32(1.0))
    );
    assert_eq!(layer.get("ImageTransparency"), Some(&Variant::Float32(0.5)));
    assert_eq!(layer.get("ScaleType"), Some(&Variant::Enum(2)));
    assert_eq!(layer.get("TileSize"), Some(&udim2(200.0, 150.0)));
    assert!(layer.review.as_deref().unwrap().contains("TileSize"));
    only(layer, "UICorner");
    only(&card, "UICorner");
    assert!(card.children.iter().any(|c| c.class == "TextLabel"));

    for (mode, scale) in [("FILL", 4), ("FIT", 3), ("STRETCH", 0), ("CROP", 0)] {
        let mut card = frame("Card", vec![label("A", 100.0, 100.0)]);
        card["fills"] = json!([{ "type": "IMAGE", "scaleMode": mode, "imageRef": "r" }]);
        let card = infer(&card).unwrap();
        assert_eq!(
            card.children[0].get("ScaleType"),
            Some(&Variant::Enum(scale)),
            "{mode}"
        );
        assert_eq!(card.children[0].review.is_some(), mode == "CROP", "{mode}");
    }
}

#[test]
fn stacked_fills_on_a_container_are_layers_bottom_first() {
    let mut card = frame("Card", vec![label("A", 100.0, 100.0)]);
    card["fills"] = json!([
        solid(1.0, 0.0, 0.0),
        linear(),
        { "type": "IMAGE", "imageRef": "top" }
    ]);
    let card = infer(&card).unwrap();
    assert_eq!(
        card.get("BackgroundColor3"),
        Some(&Variant::Color3(Color3Data {
            r: 1.0,
            g: 0.0,
            b: 0.0
        }))
    );
    let (a, b) = (&card.children[0], &card.children[1]);
    assert_eq!((a.class, a.id.as_str()), ("Frame", "1:1#fill1"));
    assert_eq!(a.get("ZIndex"), Some(&Variant::Int32(-1)));
    only(a, "UIGradient");
    assert_eq!((b.class, b.id.as_str()), ("ImageLabel", "1:1#fill2"));
    assert_eq!(b.get("ZIndex"), Some(&Variant::Int32(0)));

    // Auto layout would lay the layers out: the content moves aside.
    let mut list = frame("List", vec![label("A", 0.0, 0.0)]);
    list["layoutMode"] = json!("VERTICAL");
    list["fills"] = json!([solid(1.0, 0.0, 0.0), solid(0.0, 0.0, 1.0)]);
    let list = infer(&list).unwrap();
    assert!(list.children.iter().all(|c| c.class != "UIListLayout"));
    let content = list
        .children
        .iter()
        .find(|c| c.id == "1:1#content")
        .unwrap();
    only(content, "UIListLayout");
    only(content, "TextLabel");

    // A leaf still flattens.
    let mut leaf = frame("Swatch", vec![]);
    leaf["fills"] = json!([solid(1.0, 0.0, 0.0), solid(0.0, 0.0, 1.0)]);
    let leaf = infer(&leaf).unwrap();
    assert_eq!(leaf.image, Some(Image::Render("1:1".into())));
    assert_eq!(leaf.confidence, Confidence::Low);
}

#[test]
fn nothing_vanishes_without_a_note() {
    let slice = json!({ "id": "7:1", "type": "SLICE", "name": "Export",
                        "absoluteBoundingBox": bounds(0.0, 0.0, 10.0, 10.0) });
    let sticky = json!({ "id": "7:2", "type": "STICKY", "name": "Note",
                         "absoluteBoundingBox": bounds(0.0, 0.0, 10.0, 10.0) });
    let odd = json!({ "id": "7:3", "type": "SOMETHING_NEW", "name": "Odd",
                      "absoluteBoundingBox": bounds(0.0, 0.0, 10.0, 10.0) });
    let mask = json!({ "id": "7:4", "type": "RECTANGLE", "name": "Mask", "isMask": true,
                       "absoluteBoundingBox": bounds(0.0, 0.0, 10.0, 10.0) });
    let masked = frame("Masked", vec![mask, label("A", 0.0, 0.0)]);
    let root = infer(&frame("Root", vec![slice, sticky, odd, masked])).unwrap();
    let names: Vec<&str> = root.children.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["Note", "Odd", "Masked"]);
    for child in &root.children {
        assert_eq!(child.confidence, Confidence::Low, "{}", child.name);
        assert!(child.review.is_some(), "{}", child.name);
    }
    assert_eq!(root.children[0].image, Some(Image::Render("7:2".into())));
    assert_eq!(root.children[2].image, Some(Image::Render("1:1".into())));

    let mut free = label("Badge", 0.0, 0.0);
    free["layoutPositioning"] = json!("ABSOLUTE");
    let mut row = frame("Row", vec![free]);
    row["layoutMode"] = json!("HORIZONTAL");
    let row = infer(&row).unwrap();
    assert_eq!(only(&row, "TextLabel").confidence, Confidence::Low);
}

/// Checks every property against the API dump: it exists on its class, its
/// value has the kind the dump declares, and enum ordinals name an item.
/// Returns the classes it saw.
fn assert_in_dump(root: &mut Node) -> std::collections::BTreeSet<&'static str> {
    let db = rbx_reflection::ReflectionDatabase::embedded();
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
    classes
}

#[test]
fn every_property_written_is_in_the_api_dump() {
    let mut everything = frame("Root", vec![]);
    everything["cornerRadius"] = json!(4);
    everything["clipsContent"] = json!(true);
    everything["strokeWeight"] = json!(2);
    everything["strokeAlign"] = json!("CENTER");
    everything["strokes"] = json!([linear()]);
    everything["effects"] = json!([{ "type": "DROP_SHADOW", "color": rgb(0.0, 0.0, 0.0, 0.5),
                                     "offset": { "x": 0, "y": 4 }, "radius": 8, "spread": 2 }]);
    everything["layoutMode"] = json!("HORIZONTAL");
    everything["layoutWrap"] = json!("WRAP");
    everything["paddingTop"] = json!(4);
    everything["minWidth"] = json!(100);
    everything["maxHeight"] = json!(600);
    everything["fills"] =
        json!([linear(), { "type": "IMAGE", "scaleMode": "TILE", "imageRef": "r" }]);
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
                          "fills": [solid(1.0, 1.0, 1.0)], "children": [] });
    let mut grow = frame("Grow", vec![]);
    grow["layoutSizingHorizontal"] = json!("FILL");
    grow["layoutSizingVertical"] = json!("HUG");
    let mut rich = label("Rich", 0.0, 0.0);
    rich["characterStyleOverrides"] = json!([1]);
    rich["styleOverrideTable"] = json!({ "1": { "fontSize": 24 } });
    rich["style"]["lineHeightPx"] = json!(27);
    rich["style"]["textTruncation"] = json!("ENDING");
    rich["fills"] = json!([linear()]);
    rich["strokes"] = json!([solid(0.0, 0.0, 0.0)]);
    let mut row = frame("Row", vec![grow, rich]);
    row["layoutMode"] = json!("HORIZONTAL");
    row["primaryAxisAlignItems"] = json!("SPACE_BETWEEN");
    everything["children"] = json!([
        label("Title", 0.0, 0.0),
        label("Email input", 0.0, 0.0),
        image,
        photo,
        button,
        overflow,
        vector,
        ellipse,
        row
    ]);

    let classes = assert_in_dump(&mut infer(&everything).unwrap());
    for class in [
        "Frame",
        "CanvasGroup",
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
        "UIGridLayout",
        "UIPadding",
        "UIFlexItem",
        "UITextSizeConstraint",
        "UISizeConstraint",
    ] {
        assert!(classes.contains(class), "the fixture never made a {class}");
    }
}
