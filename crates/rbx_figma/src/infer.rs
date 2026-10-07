//! Figma's node JSON (`GET /v1/files/:key/nodes`) as a tree of Roblox GUI
//! instances: the class each node most likely is, its properties, and the
//! `UI*` modifiers its styling maps onto.
//!
//! Class rules, strongest first: a click interaction makes a button; a
//! component named like a button or with hover/pressed variants makes one
//! too; text is a `TextLabel` (a `TextBox` when it is named like an input,
//! flagged for review); vectors and all-vector subtrees are rendered to one
//! image; an image fill is an `ImageLabel`; a clipping frame whose children
//! overflow is a `ScrollingFrame`; anything else a `Frame`. Paint Roblox
//! can't draw (stacked fills, blend modes, radial gradients, inner shadow,
//! blur) is flattened to a rendered image on leaves, and dropped with a
//! review note on containers, which would lose their children otherwise.

use rbx_dom::{
    Color3Data, ColorSequence, ColorSequenceKeypoint, Font, FontStyle, NumberSequence,
    NumberSequenceKeypoint, UDim, UDim2, Variant, Vector2Data,
};
use serde_json::Value;

/// One instance to create.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub class: &'static str,
    pub name: String,
    pub properties: Vec<(&'static str, Variant)>,
    pub children: Vec<Node>,
    /// Why the guess is shaky, for the UI Editor's review list.
    pub review: Option<String>,
    /// The picture its `Image` needs, uploaded by [`crate::import`].
    pub image: Option<Image>,
    /// Figma's node id (`1:2`); empty on the `UI*` modifiers inference adds.
    pub id: String,
    /// How sure the class guess and the paint translation are.
    pub confidence: Confidence,
}

/// How much a node's translation can be trusted, for the review step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Confidence {
    /// Guessed from a name, or drawn differently: look at it.
    Low,
    /// Something was approximated, but the class is solid.
    Medium,
    #[default]
    High,
}

/// The classes the review step offers when a guess is wrong.
pub const CLASSES: [&str; 8] = [
    "Frame",
    "TextLabel",
    "TextButton",
    "TextBox",
    "ImageLabel",
    "ImageButton",
    "ScrollingFrame",
    "CanvasGroup",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Image {
    /// An image fill, by Figma's `imageRef`.
    Fill(String),
    /// The node rendered to a PNG, by node id.
    Render(String),
}

impl Node {
    fn new(class: &'static str, name: &str) -> Self {
        Node {
            class,
            name: name.to_string(),
            properties: Vec::new(),
            children: Vec::new(),
            review: None,
            image: None,
            id: String::new(),
            confidence: Confidence::High,
        }
    }

    /// Whether this is a GUI object from the design rather than a `UI*`
    /// modifier inference added.
    pub fn is_design(&self) -> bool {
        !self.id.is_empty()
    }

    /// Swaps the class picked in review. Properties the new class lacks are
    /// left for the caller to drop against the reflection database.
    pub fn set_class(&mut self, class: &'static str) {
        if class != self.class {
            self.note(format!("class changed in review from {}", self.class));
            self.class = class;
            self.confidence = Confidence::High;
        }
    }

    /// Replaces the node and its subtree with one picture of it, rendered by
    /// Figma: where it sits stays, everything drawn inside goes into the PNG.
    pub fn flatten(&mut self) {
        const KEEP: [&str; 6] = [
            "AnchorPoint",
            "Position",
            "Size",
            "LayoutOrder",
            "Rotation",
            "ZIndex",
        ];
        self.class = "ImageLabel";
        self.properties.retain(|(n, _)| KEEP.contains(n));
        self.set("BackgroundTransparency", Variant::Float32(1.0));
        self.set("BorderSizePixel", Variant::Int32(0));
        self.children.clear();
        self.image = Some(Image::Render(self.id.clone()));
        self.confidence = Confidence::High;
        self.note("flattened to an image in review");
    }

    fn set(&mut self, name: &'static str, value: Variant) {
        self.properties.retain(|(n, _)| *n != name);
        self.properties.push((name, value));
    }

    pub fn get(&self, name: &str) -> Option<&Variant> {
        self.properties
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v)
    }

    fn note(&mut self, why: impl Into<String>) {
        let why = why.into();
        self.review = Some(match self.review.take() {
            Some(before) => format!("{before}; {why}"),
            None => why,
        });
    }

    /// Every node, parents first.
    pub fn walk_mut(&mut self, f: &mut impl FnMut(&mut Node)) {
        f(self);
        for child in &mut self.children {
            child.walk_mut(f);
        }
    }

    /// One line per node, indented: for the live test's eyes.
    pub fn outline(&self) -> String {
        let mut out = String::new();
        fn line(node: &Node, depth: usize, out: &mut String) {
            let review = node
                .review
                .as_deref()
                .map(|r| format!("  [review: {r}]"))
                .unwrap_or_default();
            out.push_str(&format!(
                "{}{} {:?}{review}\n",
                "  ".repeat(depth),
                node.class,
                node.name
            ));
            for child in &node.children {
                line(child, depth + 1, out);
            }
        }
        line(self, 0, &mut out);
        out
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Rect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

struct Parent<'a> {
    rect: Rect,
    auto_layout: bool,
    name: &'a str,
}

/// The tree for `root`, a frame centred in the screen it lands in.
pub fn infer(root: &Value) -> Result<Node, String> {
    node(root, None, 1.0, 0).ok_or_else(|| "That frame is hidden in Figma".to_string())
}

fn node(v: &Value, parent: Option<&Parent>, inherited: f64, order: i32) -> Option<Node> {
    let mut out = design_node(v, parent, inherited, order)?;
    out.id = text(v, "id").to_string();
    Some(out)
}

fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

fn num(v: &Value, key: &str) -> Option<f64> {
    v.get(key).and_then(Value::as_f64)
}

fn visible(v: &Value) -> bool {
    v.get("visible").and_then(Value::as_bool) != Some(false)
}

fn list<'a>(v: &'a Value, key: &str) -> Vec<&'a Value> {
    v.get(key)
        .and_then(Value::as_array)
        .map(|a| a.iter().filter(|p| visible(p)).collect())
        .unwrap_or_default()
}

fn rect_of(v: &Value, key: &str) -> Option<Rect> {
    let r = v.get(key)?;
    Some(Rect {
        x: num(r, "x")?,
        y: num(r, "y")?,
        w: num(r, "width")?,
        h: num(r, "height")?,
    })
}

fn color(c: &Value) -> (Color3Data, f64) {
    let part = |k| num(c, k).unwrap_or(0.0) as f32;
    (
        Color3Data {
            r: part("r"),
            g: part("g"),
            b: part("b"),
        },
        num(c, "a").unwrap_or(1.0),
    )
}

fn offset(n: f64) -> i32 {
    n.round() as i32
}

fn udim(scale: f32, off: f64) -> UDim {
    UDim {
        scale,
        offset: offset(off),
    }
}

fn udim2(x: f64, y: f64) -> Variant {
    Variant::UDim2(UDim2 {
        x: udim(0.0, x),
        y: udim(0.0, y),
    })
}

const VECTORS: [&str; 5] = [
    "VECTOR",
    "BOOLEAN_OPERATION",
    "STAR",
    "LINE",
    "REGULAR_POLYGON",
];

fn clicks(v: &Value) -> bool {
    ["interactions", "reactions"].iter().any(|key| {
        list(v, key).iter().any(|r| {
            matches!(
                r.pointer("/trigger/type").and_then(Value::as_str),
                Some("ON_CLICK" | "ON_PRESS")
            )
        })
    })
}

fn has_text(v: &Value) -> bool {
    list(v, "children")
        .iter()
        .any(|c| text(c, "type") == "TEXT" || has_text(c))
}

/// A leaf of shapes with at least one true vector: an icon.
fn all_vector(v: &Value) -> bool {
    fn shapes(v: &Value) -> (bool, bool) {
        let kind = text(v, "type");
        if VECTORS.contains(&kind) {
            return (true, true);
        }
        if matches!(kind, "ELLIPSE" | "RECTANGLE") {
            return (true, false);
        }
        let children = list(v, "children");
        if children.is_empty() || !matches!(kind, "FRAME" | "GROUP" | "INSTANCE" | "COMPONENT") {
            return (false, false);
        }
        children.iter().fold((true, false), |(all, any), c| {
            let (a, b) = shapes(c);
            (all && a, any || b)
        })
    }
    let (all, any) = shapes(v);
    all && any
}

/// Why Roblox can't draw this node's own paint, if it can't.
fn unsupported(v: &Value, fills: &[&Value]) -> Option<String> {
    if fills.len() > 1 {
        return Some(format!("{} stacked fills", fills.len()));
    }
    let blend = |b: &str| !matches!(b, "" | "NORMAL" | "PASS_THROUGH");
    if blend(text(v, "blendMode")) || fills.iter().any(|f| blend(text(f, "blendMode"))) {
        return Some("a blend mode".into());
    }
    if let Some(kind) = fills
        .iter()
        .map(|f| text(f, "type"))
        .find(|k| k.starts_with("GRADIENT_") && *k != "GRADIENT_LINEAR")
    {
        return Some(format!(
            "a {} gradient",
            kind.trim_start_matches("GRADIENT_").to_lowercase()
        ));
    }
    list(v, "effects")
        .iter()
        .map(|e| text(e, "type"))
        .find(|k| matches!(*k, "INNER_SHADOW" | "LAYER_BLUR" | "BACKGROUND_BLUR"))
        .map(|k| format!("an effect ({})", k.to_lowercase().replace('_', " ")))
}

/// Position, size and anchor along one axis, from the constraint.
fn axis(constraint: &str, at: f64, size: f64, parent: f64) -> (f32, UDim, UDim) {
    match constraint {
        "RIGHT" | "BOTTOM" => (1.0, udim(1.0, -(parent - at - size)), udim(0.0, size)),
        "CENTER" => (
            0.5,
            udim(0.5, at + size / 2.0 - parent / 2.0),
            udim(0.0, size),
        ),
        "LEFT_RIGHT" | "TOP_BOTTOM" => (0.0, udim(0.0, at), udim(1.0, size - parent)),
        "SCALE" if parent > 0.0 => (
            0.0,
            udim((at / parent) as f32, 0.0),
            udim((size / parent) as f32, 0.0),
        ),
        _ => (0.0, udim(0.0, at), udim(0.0, size)),
    }
}

fn place(out: &mut Node, v: &Value, rect: Rect, parent: Option<&Parent>, order: i32) {
    let Some(parent) = parent else {
        out.set(
            "AnchorPoint",
            Variant::Vector2(Vector2Data { x: 0.5, y: 0.5 }),
        );
        out.set(
            "Position",
            Variant::UDim2(UDim2 {
                x: udim(0.5, 0.0),
                y: udim(0.5, 0.0),
            }),
        );
        out.set("Size", udim2(rect.w, rect.h));
        return;
    };
    if parent.auto_layout {
        out.set("LayoutOrder", Variant::Int32(order));
        out.set("Size", udim2(rect.w, rect.h));
        if text(v, "layoutPositioning") == "ABSOLUTE" {
            out.note("placed freely inside an auto-layout frame; the UIListLayout lays it out");
        }
        return;
    }
    let constraints = v.get("constraints").unwrap_or(&Value::Null);
    let (ax, px, sx) = axis(
        text(constraints, "horizontal"),
        rect.x - parent.rect.x,
        rect.w,
        parent.rect.w,
    );
    let (ay, py, sy) = axis(
        text(constraints, "vertical"),
        rect.y - parent.rect.y,
        rect.h,
        parent.rect.h,
    );
    if ax != 0.0 || ay != 0.0 {
        out.set(
            "AnchorPoint",
            Variant::Vector2(Vector2Data { x: ax, y: ay }),
        );
    }
    out.set("Position", Variant::UDim2(UDim2 { x: px, y: py }));
    out.set("Size", Variant::UDim2(UDim2 { x: sx, y: sy }));
}

fn design_node(v: &Value, parent: Option<&Parent>, inherited: f64, order: i32) -> Option<Node> {
    if !visible(v) {
        return None;
    }
    let kind = text(v, "type");
    let name = text(v, "name");
    let opacity = inherited * num(v, "opacity").unwrap_or(1.0);
    let rect = rect_of(v, "absoluteBoundingBox").unwrap_or_default();
    let fills = list(v, "fills");
    let children = list(v, "children");
    let lower = name.to_lowercase();
    let flatten = unsupported(v, &fills);

    // A picture of the node: vectors, and leaves whose paint Roblox lacks.
    if kind != "TEXT"
        && ((all_vector(v) && (parent.is_some() || VECTORS.contains(&kind)))
            || (flatten.is_some() && children.is_empty()))
    {
        let mut out = Node::new("ImageLabel", name);
        let bounds = rect_of(v, "absoluteRenderBounds").unwrap_or(rect);
        place(&mut out, v, bounds, parent, order);
        out.set("BackgroundTransparency", Variant::Float32(1.0));
        out.set("BorderSizePixel", Variant::Int32(0));
        out.set(
            "ImageTransparency",
            Variant::Float32((1.0 - inherited) as f32),
        );
        out.image = Some(Image::Render(text(v, "id").to_string()));
        if let Some(why) = flatten {
            out.note(format!("flattened to an image: {why}"));
        }
        return Some(out);
    }

    if kind == "TEXT" {
        return Some(text_node(v, parent, rect, opacity, order, &lower));
    }

    let image_fill = fills.last().filter(|f| text(f, "type") == "IMAGE").copied();
    let component = matches!(kind, "COMPONENT" | "INSTANCE" | "COMPONENT_SET");
    let button_name = lower.contains("button")
        || lower
            .split(|c: char| !c.is_alphanumeric())
            .any(|w| w == "btn");
    let variants = lower.contains("hover") || lower.contains("pressed");
    let button = clicks(v) || (component && (button_name || variants));
    let overflows = v.get("clipsContent").and_then(Value::as_bool) == Some(true)
        && children
            .iter()
            .filter_map(|c| rect_of(c, "absoluteBoundingBox"))
            .any(|c| {
                c.x < rect.x - 0.5
                    || c.y < rect.y - 0.5
                    || c.x + c.w > rect.x + rect.w + 0.5
                    || c.y + c.h > rect.y + rect.h + 0.5
            });
    let class = match () {
        _ if button && image_fill.is_none() && has_text(v) => "TextButton",
        _ if button => "ImageButton",
        _ if image_fill.is_some() => "ImageLabel",
        _ if overflows => "ScrollingFrame",
        _ => "Frame",
    };
    let mut out = Node::new(class, name);
    place(&mut out, v, rect, parent, order);
    out.set("BorderSizePixel", Variant::Int32(0));
    if !matches!(
        kind,
        "FRAME"
            | "GROUP"
            | "RECTANGLE"
            | "ELLIPSE"
            | "COMPONENT"
            | "INSTANCE"
            | "COMPONENT_SET"
            | "SECTION"
    ) {
        out.note(format!("Figma {} read as a {class}", kind.to_lowercase()));
    }
    if class == "TextButton" {
        out.set("Text", Variant::String(String::new()));
    }
    if let Some(why) = flatten {
        out.note(format!("{why} dropped; only the top fill kept"));
    }

    // The top fill paints the background, or the image.
    out.set("BackgroundTransparency", Variant::Float32(1.0));
    if let Some(fill) = fills.last() {
        let alpha = num(fill, "opacity").unwrap_or(1.0) * opacity;
        match text(fill, "type") {
            "SOLID" => {
                let (c, a) = color(fill.get("color").unwrap_or(&Value::Null));
                out.set("BackgroundColor3", Variant::Color3(c));
                out.set(
                    "BackgroundTransparency",
                    Variant::Float32((1.0 - a * alpha) as f32),
                );
            }
            "GRADIENT_LINEAR" => {
                out.set(
                    "BackgroundColor3",
                    Variant::Color3(Color3Data {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                    }),
                );
                out.set(
                    "BackgroundTransparency",
                    Variant::Float32((1.0 - alpha) as f32),
                );
                out.children.push(gradient(fill, rect));
            }
            "IMAGE" => {
                out.set("ImageTransparency", Variant::Float32((1.0 - alpha) as f32));
                let scale = match text(fill, "scaleMode") {
                    "FIT" => 3,
                    "TILE" => 2,
                    "STRETCH" => 0,
                    _ => 4,
                };
                out.set("ScaleType", Variant::Enum(scale));
                out.image = Some(Image::Fill(text(fill, "imageRef").to_string()));
            }
            _ => {}
        }
    }
    if v.get("clipsContent").and_then(Value::as_bool) == Some(true) {
        out.set("ClipsDescendants", Variant::Bool(true));
    }

    if kind == "ELLIPSE" {
        out.children.push(corner(0.5, 0.0));
    } else if let Some(r) = num(v, "cornerRadius").filter(|r| *r > 0.0) {
        out.children.push(corner(0.0, r));
    }
    out.children.extend(stroke(v, opacity, false));
    out.children.extend(shadow(v));

    let mode = text(v, "layoutMode");
    let auto_layout = matches!(mode, "HORIZONTAL" | "VERTICAL");
    if auto_layout {
        out.children.extend(list_layout(v, mode == "HORIZONTAL"));
    }
    if overflows {
        let (right, bottom) = children
            .iter()
            .filter_map(|c| rect_of(c, "absoluteBoundingBox"))
            .fold((rect.w, rect.h), |(r, b), c| {
                (r.max(c.x + c.w - rect.x), b.max(c.y + c.h - rect.y))
            });
        out.set("CanvasSize", udim2(right, bottom));
    }

    let here = Parent {
        rect,
        auto_layout,
        name,
    };
    let mut order = 0;
    for child in children {
        order += 1;
        out.children
            .extend(node(child, Some(&here), opacity, order));
    }
    Some(out)
}

const INPUT_HINTS: [&str; 5] = ["input", "textbox", "textfield", "text field", "placeholder"];

/// Figma family names with spaces dropped, as the Roblox family they map to.
const FONTS: [(&str, &str); 22] = [
    ("Inter", "BuilderSans"),
    ("BuilderSans", "BuilderSans"),
    ("Roboto", "Roboto"),
    ("RobotoMono", "RobotoMono"),
    ("Montserrat", "Montserrat"),
    ("Arial", "Arimo"),
    ("Helvetica", "Arimo"),
    ("Arimo", "Arimo"),
    ("Nunito", "Nunito"),
    ("Oswald", "Oswald"),
    ("Ubuntu", "Ubuntu"),
    ("Merriweather", "Merriweather"),
    ("SourceSansPro", "SourceSansPro"),
    ("SourceSans3", "SourceSansPro"),
    ("JosefinSans", "JosefinSans"),
    ("TitilliumWeb", "TitilliumWeb"),
    ("FredokaOne", "FredokaOne"),
    ("Bangers", "Bangers"),
    ("PermanentMarker", "PermanentMarker"),
    ("IndieFlower", "IndieFlower"),
    ("ComicNeue", "ComicNeueAngular"),
    ("Inconsolata", "Inconsolata"),
];

fn font(style: &Value) -> (Font, Option<String>) {
    let family = text(style, "fontFamily");
    let key: String = family.chars().filter(|c| !c.is_whitespace()).collect();
    let found = FONTS
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(&key))
        .map(|(_, f)| *f);
    let weight = num(style, "fontWeight").unwrap_or(400.0);
    let font = Font {
        family: format!(
            "rbxasset://fonts/families/{}.json",
            found.unwrap_or("BuilderSans")
        ),
        weight: ((weight / 100.0).round() * 100.0).clamp(100.0, 900.0) as u16,
        style: if style.get("italic").and_then(Value::as_bool) == Some(true) {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        },
        cached_face_id: None,
    };
    let note = match found {
        None if !family.is_empty() => Some(format!(
            "font {family} isn\u{2019}t on Roblox; Builder Sans stands in"
        )),
        _ => None,
    };
    (font, note)
}

fn text_node(
    v: &Value,
    parent: Option<&Parent>,
    rect: Rect,
    opacity: f64,
    order: i32,
    lower: &str,
) -> Node {
    let parent_name = parent.map_or(String::new(), |p| p.name.to_lowercase());
    let input = INPUT_HINTS
        .iter()
        .any(|h| lower.contains(h) || parent_name.contains(h));
    let mut out = Node::new(if input { "TextBox" } else { "TextLabel" }, text(v, "name"));
    place(&mut out, v, rect, parent, order);
    out.set("BackgroundTransparency", Variant::Float32(1.0));
    out.set("BorderSizePixel", Variant::Int32(0));
    let style = v.get("style").unwrap_or(&Value::Null);
    let characters = text(v, "characters").to_string();
    if input {
        out.set("PlaceholderText", Variant::String(characters));
        out.set("Text", Variant::String(String::new()));
        out.set("ClearTextOnFocus", Variant::Bool(false));
        out.note("named like an input, so made a TextBox");
    } else {
        out.set("Text", Variant::String(characters));
    }
    out.set(
        "TextSize",
        Variant::Float32(num(style, "fontSize").unwrap_or(14.0) as f32),
    );
    let fills = list(v, "fills");
    if let Some(fill) = fills.last() {
        let (c, a) = color(fill.get("color").unwrap_or(&Value::Null));
        let alpha = a * num(fill, "opacity").unwrap_or(1.0) * opacity;
        out.set(
            if input {
                "PlaceholderColor3"
            } else {
                "TextColor3"
            },
            Variant::Color3(c),
        );
        out.set("TextColor3", Variant::Color3(c));
        out.set("TextTransparency", Variant::Float32((1.0 - alpha) as f32));
        if text(fill, "type") != "SOLID" || fills.len() > 1 {
            out.note("text paint other than one solid colour; its first colour kept");
        }
    }
    let x = match text(style, "textAlignHorizontal") {
        "RIGHT" => 1,
        "CENTER" => 2,
        _ => 0,
    };
    let y = match text(style, "textAlignVertical") {
        "CENTER" => 1,
        "BOTTOM" => 2,
        _ => 0,
    };
    out.set("TextXAlignment", Variant::Enum(x));
    out.set("TextYAlignment", Variant::Enum(y));
    out.set(
        "TextWrapped",
        Variant::Bool(text(style, "textAutoResize") != "WIDTH_AND_HEIGHT"),
    );
    let (face, note) = font(style);
    out.set("FontFace", Variant::Font(face));
    if let Some(note) = note {
        out.note(note);
    }
    let mixed = v
        .get("characterStyleOverrides")
        .and_then(Value::as_array)
        .is_some_and(|o| o.iter().any(|s| s.as_u64().unwrap_or(0) != 0));
    if mixed {
        out.note("mixed text styles; only the base style kept");
    }
    out.children.extend(stroke(v, opacity, true));
    out
}

fn corner(scale: f32, radius: f64) -> Node {
    let mut out = Node::new("UICorner", "UICorner");
    out.set("CornerRadius", Variant::UDim(udim(scale, radius)));
    out
}

fn stroke(v: &Value, opacity: f64, text_stroke: bool) -> Option<Node> {
    let weight = num(v, "strokeWeight").filter(|w| *w > 0.0)?;
    let paint = list(v, "strokes")
        .into_iter()
        .rev()
        .find(|s| text(s, "type") == "SOLID")?;
    let (c, a) = color(paint.get("color").unwrap_or(&Value::Null));
    let mut out = Node::new("UIStroke", "UIStroke");
    out.set("Color", Variant::Color3(c));
    out.set("Thickness", Variant::Float32(weight as f32));
    out.set(
        "Transparency",
        Variant::Float32((1.0 - a * num(paint, "opacity").unwrap_or(1.0) * opacity) as f32),
    );
    out.set(
        "ApplyStrokeMode",
        Variant::Enum(if text_stroke { 0 } else { 1 }),
    );
    Some(out)
}

fn shadow(v: &Value) -> Option<Node> {
    let effect = list(v, "effects")
        .into_iter()
        .find(|e| text(e, "type") == "DROP_SHADOW")?;
    let (c, a) = color(effect.get("color").unwrap_or(&Value::Null));
    let at = effect.get("offset").unwrap_or(&Value::Null);
    let spread = num(effect, "spread").unwrap_or(0.0);
    let mut out = Node::new("UIShadow", "UIShadow");
    out.set("Color", Variant::Color3(c));
    out.set("Transparency", Variant::Float32((1.0 - a) as f32));
    out.set(
        "Offset",
        udim2(num(at, "x").unwrap_or(0.0), num(at, "y").unwrap_or(0.0)),
    );
    out.set("Spread", udim2(spread, spread));
    out.set(
        "BlurRadius",
        Variant::UDim(udim(0.0, num(effect, "radius").unwrap_or(0.0))),
    );
    Some(out)
}

fn gradient(fill: &Value, rect: Rect) -> Node {
    let stops = list(fill, "gradientStops");
    let alpha = num(fill, "opacity").unwrap_or(1.0);
    let mut colors: Vec<ColorSequenceKeypoint> = Vec::new();
    let mut alphas: Vec<NumberSequenceKeypoint> = Vec::new();
    for stop in stops.iter().take(20) {
        let time = num(stop, "position").unwrap_or(0.0).clamp(0.0, 1.0) as f32;
        let (c, a) = color(stop.get("color").unwrap_or(&Value::Null));
        colors.push(ColorSequenceKeypoint {
            time,
            color: c,
            envelope: 0.0,
        });
        alphas.push(NumberSequenceKeypoint {
            time,
            value: (1.0 - a * alpha) as f32,
            envelope: 0.0,
        });
    }
    // Roblox wants keypoints at exactly 0 and 1.
    if let (Some(first), Some(last)) = (colors.first().cloned(), colors.last().cloned()) {
        if first.time > 0.0 {
            colors.insert(0, ColorSequenceKeypoint { time: 0.0, ..first });
            alphas.insert(
                0,
                NumberSequenceKeypoint {
                    time: 0.0,
                    ..alphas[0]
                },
            );
        }
        if last.time < 1.0 {
            colors.push(ColorSequenceKeypoint { time: 1.0, ..last });
            let end = alphas.last().cloned().expect("as many as colours");
            alphas.push(NumberSequenceKeypoint { time: 1.0, ..end });
        }
    }
    let handles = list(fill, "gradientHandlePositions");
    let rotation = match (handles.first(), handles.get(1)) {
        (Some(a), Some(b)) => {
            let dx = (num(b, "x").unwrap_or(1.0) - num(a, "x").unwrap_or(0.0)) * rect.w;
            let dy = (num(b, "y").unwrap_or(0.0) - num(a, "y").unwrap_or(0.0)) * rect.h;
            dy.atan2(dx).to_degrees()
        }
        _ => 0.0,
    };
    let mut out = Node::new("UIGradient", "UIGradient");
    out.set(
        "Color",
        Variant::ColorSequence(ColorSequence { keypoints: colors }),
    );
    out.set(
        "Transparency",
        Variant::NumberSequence(NumberSequence { keypoints: alphas }),
    );
    out.set("Rotation", Variant::Float32(rotation as f32));
    out
}

fn list_layout(v: &Value, horizontal: bool) -> Vec<Node> {
    let mut layout = Node::new("UIListLayout", "UIListLayout");
    layout.set(
        "FillDirection",
        Variant::Enum(if horizontal { 0 } else { 1 }),
    );
    layout.set("SortOrder", Variant::Enum(2));
    layout.set(
        "Padding",
        Variant::UDim(udim(0.0, num(v, "itemSpacing").unwrap_or(0.0))),
    );
    let primary = text(v, "primaryAxisAlignItems");
    let counter = text(v, "counterAxisAlignItems");
    let h_of = |a: &str| match a {
        "CENTER" => 0,
        "MAX" => 2,
        _ => 1,
    };
    let v_of = |a: &str| match a {
        "CENTER" => 0,
        "MAX" => 2,
        _ => 1,
    };
    let (h, vert) = if horizontal {
        (h_of(primary), v_of(counter))
    } else {
        (h_of(counter), v_of(primary))
    };
    layout.set("HorizontalAlignment", Variant::Enum(h));
    layout.set("VerticalAlignment", Variant::Enum(vert));
    if primary == "SPACE_BETWEEN" {
        layout.set(
            if horizontal {
                "HorizontalFlex"
            } else {
                "VerticalFlex"
            },
            Variant::Enum(3),
        );
    }
    if text(v, "layoutWrap") == "WRAP" {
        layout.set("Wraps", Variant::Bool(true));
    }
    let mut out = vec![layout];
    let pads = [
        ("PaddingLeft", "paddingLeft"),
        ("PaddingRight", "paddingRight"),
        ("PaddingTop", "paddingTop"),
        ("PaddingBottom", "paddingBottom"),
    ];
    if pads.iter().any(|(_, k)| num(v, k).unwrap_or(0.0) != 0.0) {
        let mut padding = Node::new("UIPadding", "UIPadding");
        for (property, key) in pads {
            padding.set(
                property,
                Variant::UDim(udim(0.0, num(v, key).unwrap_or(0.0))),
            );
        }
        out.push(padding);
    }
    out
}

#[cfg(test)]
#[path = "infer/tests.rs"]
mod tests;
