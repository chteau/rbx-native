//! GUI scenes: each a full-screen backdrop with labels, frames and images
//! over it, so only the 2D drawing is compared.

use rbx_dom::{
    Color3Data, ColorSequence, ColorSequenceKeypoint, Font, FontStyle, NumberSequence,
    NumberSequenceKeypoint, Rect, Ref, UDim, Variant, Vector2Data,
};

use super::super::{content, float, rgb, udim2, Builder, Scene};
use super::studio;

const GRID: &str = "rbxassetid://6372755229";
const FACE: &str = "rbxasset://textures/face.png";
const ROUNDED: &str = "rbxasset://textures/ui/RoundedRect8px.png";
const BACKDROP: [u8; 3] = [46, 46, 52];

fn font(family: &str, weight: u16, italic: bool) -> Variant {
    Variant::Font(Font {
        family: format!("rbxasset://fonts/families/{family}.json"),
        weight,
        style: if italic {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        },
        cached_face_id: None,
    })
}

fn udim(scale: f32, offset: i32) -> Variant {
    Variant::UDim(UDim { scale, offset })
}

fn colour([r, g, b]: [u8; 3]) -> Color3Data {
    Color3Data {
        r: f32::from(r) / 255.0,
        g: f32::from(g) / 255.0,
        b: f32::from(b) / 255.0,
    }
}

fn colours(keys: &[(f32, [u8; 3])]) -> Variant {
    Variant::ColorSequence(ColorSequence {
        keypoints: keys
            .iter()
            .map(|&(time, c)| ColorSequenceKeypoint {
                time,
                color: colour(c),
                envelope: 0.0,
            })
            .collect(),
    })
}

fn numbers(keys: &[(f32, f32)]) -> Variant {
    Variant::NumberSequence(NumberSequence {
        keypoints: keys
            .iter()
            .map(|&(time, value)| NumberSequenceKeypoint {
                time,
                value,
                envelope: 0.0,
            })
            .collect(),
    })
}

fn vec2(x: f32, y: f32) -> Variant {
    Variant::Vector2(Vector2Data { x, y })
}

/// A screen that ignores the top bar inset, with the backdrop over the
/// whole of it.
fn backdrop(b: &mut Builder) -> Ref {
    b.floor();
    let screen = b.screen();
    b.set(screen, "IgnoreGuiInset", Variant::Bool(true));
    let back = frame(b, screen, [0.0, 0.0], [0.0, 0.0], BACKDROP);
    b.set(back, "Size", udim2([1.0, 0.0], [1.0, 0.0]));
    screen
}

/// A borderless `class` at pixel `position` and `size`.
fn object(b: &mut Builder, parent: Ref, class: &str, position: [f32; 2], size: [f32; 2]) -> Ref {
    let object = b.child(parent, class);
    b.set(
        object,
        "Position",
        udim2([0.0, position[0]], [0.0, position[1]]),
    );
    b.set(object, "Size", udim2([0.0, size[0]], [0.0, size[1]]));
    b.set(object, "BorderSizePixel", Variant::Int32(0));
    object
}

fn frame(b: &mut Builder, parent: Ref, position: [f32; 2], size: [f32; 2], fill: [u8; 3]) -> Ref {
    let frame = object(b, parent, "Frame", position, size);
    let [r, g, bl] = fill;
    b.set(frame, "BackgroundColor3", rgb(r, g, bl));
    frame
}

/// A label with no background, white text at `size`.
fn label(
    b: &mut Builder,
    parent: Ref,
    position: [f32; 2],
    size: [f32; 2],
    text: &str,
    text_size: f32,
) -> Ref {
    let label = object(b, parent, "TextLabel", position, size);
    b.set(label, "BackgroundTransparency", float(1.0));
    b.set(label, "TextColor3", rgb(255, 255, 255));
    b.set(label, "TextSize", float(text_size));
    b.set(label, "Text", Variant::String(text.to_owned()));
    label
}

const PANGRAM: &str = "The quick brown fox jumps 0123";

/// The families games use most, in a big and a small size.
const FONTS: [(&str, u16, bool); 9] = [
    ("SourceSansPro", 400, false),
    ("BuilderSans", 400, false),
    ("BuilderSans", 700, false),
    ("GothamSSm", 400, false),
    ("GothamSSm", 700, false),
    ("Arial", 400, false),
    ("Roboto", 400, false),
    ("FredokaOne", 400, false),
    ("SourceSansPro", 400, true),
];

fn fonts(b: &mut Builder) {
    let screen = backdrop(b);
    for (row, &(family, weight, italic)) in FONTS.iter().enumerate() {
        let y = 80.0 + 70.0 * row as f32;
        for (x, size, width) in [(30.0, 32.0, 760.0), (820.0, 16.0, 430.0)] {
            let text = label(b, screen, [x, y], [width, 60.0], PANGRAM, size);
            b.set(text, "FontFace", font(family, weight, italic));
            b.set(text, "TextXAlignment", Variant::Enum(0));
        }
    }
}

const PARAGRAPH: &str =
    "Wrapped text runs onto a second and a third line when the box is too narrow for it";

fn text_layout(b: &mut Builder) {
    let screen = backdrop(b);
    let builder = font("BuilderSans", 400, false);
    // TextScaled in three boxes of different shapes.
    for (i, size) in [[260.0, 50.0], [260.0, 110.0], [120.0, 110.0]]
        .into_iter()
        .enumerate()
    {
        let x = 30.0 + [0.0, 0.0, 280.0][i];
        let y = [80.0, 150.0, 150.0][i];
        let text = label(b, screen, [x, y], size, "Scaled text", 14.0);
        b.set(text, "BackgroundTransparency", float(0.0));
        b.set(text, "BackgroundColor3", rgb(70, 70, 90));
        b.set(text, "TextScaled", Variant::Bool(true));
        b.set(text, "FontFace", builder.clone());
    }
    // Wrapped paragraphs at LineHeight 1 and 1.5, top aligned.
    for (i, height) in [1.0, 1.5].into_iter().enumerate() {
        let x = 450.0 + 270.0 * i as f32;
        let text = label(b, screen, [x, 80.0], [250.0, 180.0], PARAGRAPH, 22.0);
        b.set(text, "BackgroundTransparency", float(0.0));
        b.set(text, "BackgroundColor3", rgb(70, 70, 90));
        b.set(text, "TextWrapped", Variant::Bool(true));
        b.set(text, "LineHeight", float(height));
        b.set(text, "TextXAlignment", Variant::Enum(0));
        b.set(text, "TextYAlignment", Variant::Enum(0));
        b.set(text, "FontFace", builder.clone());
    }
    // RichText.
    let rich = label(
        b,
        screen,
        [30.0, 290.0],
        [1000.0, 50.0],
        "Plain <b>bold</b> <i>italic</i> <font color=\"#ff5050\">red</font> <font size=\"40\">big</font> <u>under</u> <s>struck</s>",
        28.0,
    );
    b.set(rich, "RichText", Variant::Bool(true));
    b.set(rich, "TextXAlignment", Variant::Enum(0));
    b.set(rich, "FontFace", builder.clone());
    // Strokes on text, opaque and half transparent, and half transparent text.
    for (i, (stroke, text_transparency)) in
        [(0.0, 0.0), (0.5, 0.0), (1.0, 0.5)].into_iter().enumerate()
    {
        let y = 370.0 + 70.0 * i as f32;
        let text = label(b, screen, [30.0, y], [600.0, 60.0], "Stroked text 42", 40.0);
        b.set(text, "TextColor3", rgb(255, 220, 60));
        b.set(text, "TextStrokeColor3", rgb(0, 0, 0));
        b.set(text, "TextStrokeTransparency", float(stroke));
        b.set(text, "TextTransparency", float(text_transparency));
        b.set(text, "TextXAlignment", Variant::Enum(0));
        b.set(text, "FontFace", font("GothamSSm", 700, false));
    }
    // Vertical alignments in tall boxes.
    for (i, alignment) in [0u32, 1, 2].into_iter().enumerate() {
        let x = 700.0 + 180.0 * i as f32;
        let text = label(b, screen, [x, 370.0], [160.0, 200.0], "Aligned", 26.0);
        b.set(text, "BackgroundTransparency", float(0.0));
        b.set(text, "BackgroundColor3", rgb(70, 70, 90));
        b.set(text, "TextYAlignment", Variant::Enum(alignment));
        b.set(text, "FontFace", builder.clone());
    }
}

/// A `UIStroke` under `parent`.
fn stroke(b: &mut Builder, parent: Ref, thickness: f32, fill: [u8; 3]) -> Ref {
    let stroke = b.child(parent, "UIStroke");
    b.set(stroke, "Thickness", float(thickness));
    let [r, g, bl] = fill;
    b.set(stroke, "Color", rgb(r, g, bl));
    stroke
}

fn corner(b: &mut Builder, parent: Ref, scale: f32, offset: i32) {
    let corner = b.child(parent, "UICorner");
    b.set(corner, "CornerRadius", udim(scale, offset));
}

fn strokes(b: &mut Builder) {
    let screen = backdrop(b);
    // Border strokes 1, 3 and 8 thick, square, 8 px and 24 px corners.
    for (row, radius) in [0, 8, 24].into_iter().enumerate() {
        for (column, thickness) in [1.0, 3.0, 8.0].into_iter().enumerate() {
            let at = [40.0 + 170.0 * column as f32, 90.0 + 130.0 * row as f32];
            let frame = frame(b, screen, at, [130.0, 90.0], [60, 120, 220]);
            stroke(b, frame, thickness, [255, 255, 255]);
            if radius > 0 {
                corner(b, frame, 0.0, radius);
            }
        }
    }
    // Line joins on a thick stroke: Round, Bevel, Miter.
    for (column, join) in [0u32, 1, 2].into_iter().enumerate() {
        let at = [600.0 + 200.0 * column as f32, 100.0];
        let frame = frame(b, screen, at, [130.0, 90.0], [220, 90, 60]);
        let line = stroke(b, frame, 14.0, [240, 240, 240]);
        b.set(line, "LineJoinMode", Variant::Enum(join));
    }
    // A half transparent stroke, a circle and a stroke on a transparent frame.
    let faded = frame(b, screen, [600.0, 270.0], [130.0, 90.0], [60, 180, 90]);
    let line = stroke(b, faded, 6.0, [0, 0, 0]);
    b.set(line, "Transparency", float(0.5));
    let circle = frame(b, screen, [820.0, 255.0], [120.0, 120.0], [230, 200, 60]);
    corner(b, circle, 0.5, 0);
    stroke(b, circle, 4.0, [30, 30, 30]);
    let hollow = frame(b, screen, [1000.0, 270.0], [130.0, 90.0], [255, 255, 255]);
    b.set(hollow, "BackgroundTransparency", float(1.0));
    stroke(b, hollow, 3.0, [255, 120, 200]);
    // Contextual strokes on text, and a Border stroke on a label's box.
    for (i, mode) in [0u32, 1].into_iter().enumerate() {
        let y = 440.0 + 110.0 * i as f32;
        let text = label(b, screen, [600.0, y], [520.0, 80.0], "Outlined", 56.0);
        b.set(text, "FontFace", font("BuilderSans", 700, false));
        if mode == 1 {
            b.set(text, "BackgroundTransparency", float(0.0));
            b.set(text, "BackgroundColor3", rgb(60, 120, 220));
        }
        let line = stroke(b, text, 3.0, [200, 40, 40]);
        b.set(line, "ApplyStrokeMode", Variant::Enum(mode));
    }
}

fn gradient(b: &mut Builder, parent: Ref, keys: &[(f32, [u8; 3])]) -> Ref {
    let gradient = b.child(parent, "UIGradient");
    b.set(gradient, "Color", colours(keys));
    gradient
}

const RAINBOW: [(f32, [u8; 3]); 3] = [
    (0.0, [255, 40, 40]),
    (0.5, [40, 255, 40]),
    (1.0, [40, 40, 255]),
];
const DUSK: [(f32, [u8; 3]); 2] = [(0.0, [255, 200, 40]), (1.0, [120, 20, 160])];

fn gradients(b: &mut Builder) {
    let screen = backdrop(b);
    let cells: [(f32, [f32; 2], bool); 8] = [
        (0.0, [0.0, 0.0], false),
        (45.0, [0.0, 0.0], false),
        (90.0, [0.0, 0.0], false),
        (0.0, [0.25, 0.0], false),
        (0.0, [0.0, 0.0], true),
        (135.0, [0.0, 0.0], true),
        (-30.0, [0.0, 0.3], false),
        (180.0, [0.0, 0.0], true),
    ];
    for (i, &(rotation, offset, faded)) in cells.iter().enumerate() {
        let at = [40.0 + 300.0 * (i % 4) as f32, 90.0 + 190.0 * (i / 4) as f32];
        let frame = frame(b, screen, at, [260.0, 160.0], [255, 255, 255]);
        let keys: &[(f32, [u8; 3])] = if i % 2 == 0 { &RAINBOW } else { &DUSK };
        let g = gradient(b, frame, keys);
        b.set(g, "Rotation", float(rotation));
        b.set(g, "Offset", vec2(offset[0], offset[1]));
        if faded {
            b.set(
                g,
                "Transparency",
                numbers(&[(0.0, 0.0), (0.6, 0.5), (1.0, 1.0)]),
            );
        }
        if i == 6 {
            corner(b, frame, 0.0, 30);
        }
    }
    // On text, and on a tinted frame (the gradient multiplies its colour).
    let text = label(b, screen, [40.0, 480.0], [700.0, 120.0], "GRADIENT", 96.0);
    b.set(text, "FontFace", font("GothamSSm", 900, false));
    gradient(b, text, &RAINBOW);
    let tinted = frame(b, screen, [800.0, 480.0], [400.0, 120.0], [120, 200, 255]);
    gradient(b, tinted, &DUSK);
}

fn image(b: &mut Builder, parent: Ref, at: [f32; 2], size: [f32; 2], uri: &str) -> Ref {
    let image = object(b, parent, "ImageLabel", at, size);
    b.set(image, "BackgroundColor3", rgb(20, 20, 24));
    b.set(image, "Image", content(uri));
    image
}

fn images(b: &mut Builder) {
    let screen = backdrop(b);
    // Stretch, Tile, Fit and Crop of a square grid into a wide box.
    for (i, scale_type) in [0u32, 2, 3, 4].into_iter().enumerate() {
        let at = [40.0 + 300.0 * i as f32, 90.0];
        let img = image(b, screen, at, [260.0, 140.0], GRID);
        b.set(img, "ScaleType", Variant::Enum(scale_type));
        if scale_type == 2 {
            b.set(img, "TileSize", udim2([0.0, 64.0], [0.0, 48.0]));
        }
    }
    // Nine-slice of the rounded rectangle at three sizes and a SliceScale.
    let slices: [([f32; 2], f32); 4] = [
        ([120.0, 60.0], 1.0),
        ([260.0, 140.0], 1.0),
        ([60.0, 140.0], 1.0),
        ([260.0, 140.0], 3.0),
    ];
    let mut x = 40.0;
    for (size, scale) in slices {
        let img = image(b, screen, [x, 270.0], size, ROUNDED);
        b.set(img, "BackgroundTransparency", float(1.0));
        b.set(img, "ScaleType", Variant::Enum(1));
        b.set(
            img,
            "SliceCenter",
            Variant::Rect(Rect {
                min: Vector2Data { x: 8.0, y: 8.0 },
                max: Vector2Data { x: 24.0, y: 24.0 },
            }),
        );
        b.set(img, "SliceScale", float(scale));
        b.set(img, "ImageColor3", rgb(90, 170, 255));
        x += size[0] + 40.0;
    }
    // The face: tinted, half transparent, and a quarter of it by ImageRect.
    let tinted = image(b, screen, [40.0, 450.0], [200.0, 200.0], FACE);
    b.set(tinted, "BackgroundColor3", rgb(245, 205, 48));
    b.set(tinted, "ImageColor3", rgb(255, 80, 80));
    let faded = image(b, screen, [280.0, 450.0], [200.0, 200.0], FACE);
    b.set(faded, "BackgroundColor3", rgb(245, 205, 48));
    b.set(faded, "ImageTransparency", float(0.5));
    let cropped = image(b, screen, [520.0, 450.0], [200.0, 200.0], GRID);
    b.set(cropped, "ImageRectSize", vec2(256.0, 256.0));
    b.set(cropped, "ImageRectOffset", vec2(128.0, 128.0));
    // A half transparent image over a half transparent background.
    let both = image(b, screen, [760.0, 450.0], [200.0, 200.0], GRID);
    b.set(both, "BackgroundColor3", rgb(255, 255, 255));
    b.set(both, "BackgroundTransparency", float(0.5));
    b.set(both, "ImageTransparency", float(0.5));
    let button = object(b, screen, "ImageButton", [1000.0, 450.0], [200.0, 200.0]);
    b.set(button, "BackgroundColor3", rgb(20, 20, 24));
    b.set(button, "Image", content(GRID));
    b.set(button, "ScaleType", Variant::Enum(3));
    corner(b, button, 0.0, 40);
}

fn stacking(b: &mut Builder) {
    let screen = backdrop(b);
    // Three half transparent squares, overlapping in sibling order.
    for (i, fill) in [[255, 0, 0], [0, 255, 0], [0, 0, 255]]
        .into_iter()
        .enumerate()
    {
        let at = [40.0 + 60.0 * i as f32, 90.0 + 50.0 * i as f32];
        let square = frame(b, screen, at, [140.0, 140.0], fill);
        b.set(square, "BackgroundTransparency", float(0.5));
    }
    // ZIndex beats sibling order: the first one is drawn on top.
    for (i, (fill, z)) in [
        ([250, 250, 250], 3),
        ([250, 150, 40], 2),
        ([140, 60, 200], 1),
    ]
    .into_iter()
    .enumerate()
    {
        let at = [340.0 + 60.0 * i as f32, 90.0 + 50.0 * i as f32];
        let square = frame(b, screen, at, [140.0, 140.0], fill);
        b.set(square, "ZIndex", Variant::Int32(z));
    }
    // A half transparent parent does not fade its child; a CanvasGroup does.
    let parent = frame(b, screen, [640.0, 90.0], [240.0, 240.0], [255, 255, 255]);
    b.set(parent, "BackgroundTransparency", float(0.5));
    let child = frame(b, parent, [40.0, 40.0], [160.0, 160.0], [200, 40, 40]);
    b.set(child, "BackgroundTransparency", float(0.0));
    let group = object(b, screen, "CanvasGroup", [920.0, 90.0], [240.0, 240.0]);
    b.set(group, "BackgroundColor3", rgb(255, 255, 255));
    b.set(group, "GroupTransparency", float(0.5));
    frame(b, group, [40.0, 40.0], [160.0, 160.0], [200, 40, 40]);
    // UIListLayout across, Padding 12, in a box with UIPadding.
    let row = frame(b, screen, [40.0, 400.0], [560.0, 120.0], [80, 80, 96]);
    let padding = b.child(row, "UIPadding");
    for side in ["PaddingLeft", "PaddingTop", "PaddingRight", "PaddingBottom"] {
        b.set(padding, side, udim(0.0, 16));
    }
    let list = b.child(row, "UIListLayout");
    b.set(list, "FillDirection", Variant::Enum(0));
    b.set(list, "Padding", udim(0.0, 12));
    b.set(list, "SortOrder", Variant::Enum(2));
    b.set(list, "VerticalAlignment", Variant::Enum(0));
    for (i, width) in [60.0, 100.0, 140.0, 80.0].into_iter().enumerate() {
        let item = frame(b, row, [0.0, 0.0], [width, 70.0], [240, 180, 60]);
        b.set(item, "LayoutOrder", Variant::Int32(4 - i as i32));
    }
    // UIListLayout down, centred, with scale sizes.
    let column = frame(b, screen, [660.0, 380.0], [500.0, 280.0], [80, 80, 96]);
    let list = b.child(column, "UIListLayout");
    b.set(list, "Padding", udim(0.0, 8));
    b.set(list, "SortOrder", Variant::Enum(2));
    b.set(list, "HorizontalAlignment", Variant::Enum(0));
    for (i, width) in [0.9, 0.6, 0.3].into_iter().enumerate() {
        let item = frame(b, column, [0.0, 0.0], [0.0, 60.0], [60, 200, 160]);
        b.set(item, "Size", udim2([width, 0.0], [0.0, 60.0]));
        b.set(item, "LayoutOrder", Variant::Int32(i as i32));
        let text = label(b, item, [0.0, 0.0], [0.0, 0.0], "Item", 22.0);
        b.set(text, "Size", udim2([1.0, 0.0], [1.0, 0.0]));
        b.set(text, "TextColor3", rgb(20, 20, 20));
    }
}

/// `CanvasGroup`s at `GroupTransparency` 0.5 with a white background and
/// three kinds of child, with no background of their own, at 0.25 tinted by
/// `GroupColor3`, at 0 over a half transparent child, and one inside a
/// plain frame.
fn canvas_groups(b: &mut Builder) {
    let screen = backdrop(b);
    let group = |b: &mut Builder, parent, at: [f32; 2], fade: f32| {
        let group = object(b, parent, "CanvasGroup", at, [240.0, 240.0]);
        b.set(group, "BackgroundColor3", rgb(255, 255, 255));
        b.set(group, "GroupTransparency", float(fade));
        group
    };
    let full = group(b, screen, [40.0, 90.0], 0.5);
    frame(b, full, [20.0, 20.0], [100.0, 100.0], [200, 40, 40]);
    let text = label(b, full, [20.0, 130.0], [200.0, 40.0], "Group", 32.0);
    b.set(text, "TextColor3", rgb(0, 0, 0));
    image(b, full, [130.0, 20.0], [90.0, 90.0], GRID);
    let bare = group(b, screen, [320.0, 90.0], 0.5);
    b.set(bare, "BackgroundTransparency", float(1.0));
    frame(b, bare, [40.0, 40.0], [160.0, 160.0], [200, 40, 40]);
    let tinted = group(b, screen, [600.0, 90.0], 0.25);
    b.set(tinted, "GroupColor3", rgb(0, 255, 255));
    frame(b, tinted, [40.0, 40.0], [160.0, 160.0], [200, 40, 40]);
    let clear = group(b, screen, [880.0, 90.0], 0.0);
    let child = frame(b, clear, [40.0, 40.0], [160.0, 160.0], [200, 40, 40]);
    b.set(child, "BackgroundTransparency", float(0.5));
    let holder = frame(b, screen, [40.0, 380.0], [300.0, 300.0], [40, 120, 220]);
    let inner = group(b, holder, [30.0, 30.0], 0.5);
    frame(b, inner, [40.0, 40.0], [160.0, 160.0], [200, 40, 40]);
}

pub(super) fn scenes() -> Vec<Scene> {
    let gui = |name, about, build| Scene {
        name,
        about,
        eye: [0.0, 30.0, 0.1],
        look_at: [0.0, 0.0, 0.0],
        fov: 70.0,
        lighting: studio(),
        probes: Vec::new(),
        build,
    };
    vec![
        gui(
            "gui_fonts",
            "SourceSans, Builder Sans, Gotham, Arial, Roboto, Fredoka One and an italic, at 32 and 16",
            fonts,
        ),
        gui(
            "gui_text_layout",
            "TextScaled, wrapped lines at LineHeight 1 and 1.5, RichText, TextStroke, TextYAlignment",
            text_layout,
        ),
        gui(
            "gui_strokes",
            "UIStroke Border thicknesses with UICorner radii, line joins, transparency, Contextual on text",
            strokes,
        ),
        gui(
            "gui_gradients",
            "UIGradient colour and transparency sequences, Rotation, Offset, on text and a tinted frame",
            gradients,
        ),
        gui(
            "gui_images",
            "ImageLabel ScaleType Stretch, Tile, Fit, Crop, Slice with SliceScale, tints and ImageRect",
            images,
        ),
        gui(
            "gui_stacking",
            "Stacked half transparent frames, ZIndex, CanvasGroup, UIListLayout with UIPadding",
            stacking,
        ),
        gui(
            "gui_canvas_groups",
            "CanvasGroup GroupTransparency 0.5 over a frame, text and an image, bare, tinted at 0.25, at 0, nested",
            canvas_groups,
        ),
    ]
}
