//! Figma's node JSON (`GET /v1/files/:key/nodes`) as a tree of Roblox GUI
//! instances: the class each node most likely is, its properties, and the
//! `UI*` modifiers its styling maps onto.
//!
//! Class rules, strongest first: a click interaction makes a button; a
//! component named like a button or with hover/pressed variants makes one
//! too (flagged); text is a `TextLabel` (a `TextBox` when it is named like an
//! input, flagged); vectors and all-vector subtrees are rendered to one image;
//! a leaf with an image fill is an `ImageLabel`; a clipping frame whose
//! children overflow is a `ScrollingFrame`; anything else a `Frame`.
//!
//! # Compatibility
//!
//! Every Figma feature lands in one of four buckets. **Supported** maps onto a
//! native property or modifier. **Approximated** gets the closest Roblox
//! equivalent and a review note (confidence Medium). **Impossible** renders
//! the node to a PNG through Figma (`Image::Render`) with a note (confidence
//! Low); on a container that would swallow its children, the feature is
//! dropped instead, with a note (Low). Nothing vanishes without a note.
//!
//! | Figma | Handling |
//! |---|---|
//! | FRAME, GROUP, SECTION, COMPONENT, COMPONENT_SET, INSTANCE | Supported: `Frame` (or button / `ScrollingFrame` / `CanvasGroup`) |
//! | RECTANGLE, ELLIPSE | Supported: `Frame` + `UICorner` (ellipse: radius 0.5) |
//! | ELLIPSE with a partial `arcData` (arc, donut) | Impossible on leaves: rendered |
//! | TEXT | Supported: `TextLabel` (`TextBox` when named like an input: Low) |
//! | VECTOR, BOOLEAN_OPERATION, STAR, LINE, REGULAR_POLYGON, all-vector groups | Rendered to one PNG: pixel-exact, so no note |
//! | SLICE | Skipped: an export marker, Figma draws nothing for it |
//! | STICKY, SHAPE_WITH_TEXT, CONNECTOR, TABLE, TABLE_CELL, EMBED, LINK_UNFURL, MEDIA, WIDGET, STAMP, WASHI_TAPE, HIGHLIGHT, CODE_BLOCK | Impossible: rendered |
//! | any other node type | Approximated as a `Frame`, Low |
//! | solid paint | Supported |
//! | GRADIENT_LINEAR | Supported: `UIGradient` (colour, transparency, rotation from the handles) |
//! | GRADIENT_RADIAL, GRADIENT_DIAMOND | Approximated: linear from the centre handle towards handle 2 |
//! | GRADIENT_ANGULAR | Impossible on leaves (rendered); linear on containers, Low |
//! | IMAGE fill FILL / FIT / STRETCH / TILE | Supported: `ScaleType` Crop / Fit / Stretch / Tile (`TileSize` set on upload, from the image's size) |
//! | PATTERN fill | Supported on containers: its source node rendered once and tiled (`ScaleType` Tile) under the content; hexagonal tiling and spacing approximated |
//! | IMAGE with `imageTransform` (crop), rotation, filters | Approximated: shown uncropped / unfiltered |
//! | VIDEO fill | Impossible: rendered on leaves, dropped on containers |
//! | stacked fills | Rendered on leaves; on containers each fill is its own layer under the content |
//! | blend modes | Impossible: rendered on leaves, dropped on containers |
//! | DROP_SHADOW | Supported: `UIShadow` (the first; more are approximated away) |
//! | INNER_SHADOW, LAYER_BLUR, BACKGROUND_BLUR | Impossible: rendered on leaves, dropped on containers |
//! | strokes (solid, gradient) | Supported: `UIStroke` (+ `UIGradient`); `BorderStrokePosition` from `strokeAlign` |
//! | stroke on text with INSIDE / CENTER align | Approximated: Roblox strokes text outside |
//! | dashed strokes, per-side weights, stacked strokes, image strokes | Approximated (solid / thickest / top one); image strokes dropped |
//! | `cornerRadius` | Supported: `UICorner` |
//! | per-corner `rectangleCornerRadii`, `cornerSmoothing` | Approximated: the largest radius, round corners |
//! | `clipsContent` | Supported: `ClipsDescendants`; `CanvasGroup` when children reach into rounded corners (approximated) |
//! | masks (`isMask`) | Impossible: the parent holding the mask is rendered |
//! | constraints | Supported: anchor point, scale and offset per axis |
//! | auto layout | Supported: `UIListLayout` + `UIPadding`; SPACE_BETWEEN → flex |
//! | auto layout wrap | Supported: `UIGridLayout` (cells all take the first child's size: approximated when they differ) |
//! | FILL sizing / `layoutGrow` | Supported: `UIFlexItem` Fill on the main axis, scale 1 on the cross axis |
//! | HUG sizing | Supported: `AutomaticSize` |
//! | min / max width and height | Supported: `UISizeConstraint` |
//! | ABSOLUTE child of auto layout | Approximated: the layout still places it, Low |
//! | BASELINE counter alignment | Approximated: top |
//! | rotation | Supported: `Rotation`, box from `size` (or solved from the bounding box); approximated on containers, whose children are placed from their rotated bounds |
//! | mirroring (flipped transform) | Impossible: dropped, Low |
//! | opacity | Supported on leaves; on containers multiplied into each child (overlaps show through): approximated |
//! | font family | Supported for Roblox's families, else Builder Sans (approximated) |
//! | mixed text styles | Supported: `RichText` markup (`font`, `i`, `u`, `s`, `stroke`, `sc`) |
//! | `textCase` | Supported: UPPER / LOWER / TITLE rewrite the characters, SMALL_CAPS uses `<sc>` |
//! | `textDecoration` | Supported: `<u>` / `<s>` |
//! | line height | Supported: `LineHeight` (clamped to 1..3: approximated) |
//! | text size | Supported: `TextSize` is Figma's size × 1.2 (a Roblox line, not an em); `TextScaled` + `UITextSizeConstraint` capped at it |
//! | `textTruncation` ENDING | Supported: `TextTruncate` AtEnd |
//! | JUSTIFIED, letter spacing, paragraph spacing / indent, lists, `maxLines`, hyperlinks, OpenType flags | Approximated: dropped with a note |
//! | text paint other than solid or gradient | Dropped, Low |

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
    /// Layers inference splits out of a node carry the node's id plus a
    /// suffix (`1:2#fill`, `1:2#content`).
    pub id: String,
    /// How sure the class guess and the paint translation are.
    pub confidence: Confidence,
    /// A tiled picture's `TileSize`, which needs the picture's own size:
    /// [`crate::import`] sets it once it has the PNG.
    pub tile: Option<Tile>,
}

/// How a tiled picture's tiles are sized and placed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tile {
    /// Figma's `scalingFactor`: a tile is the picture's size times this.
    pub factor: f64,
    /// The box the tiles fill, and whether they are centred in it on each
    /// axis (Figma's CENTER alignment) rather than starting at its corner.
    pub size: (f64, f64),
    pub centred: (bool, bool),
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
            tile: None,
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

    /// Sizes a tiled picture's tiles once the picture's size (in design
    /// pixels) is known; see [`Tile`]. Roblox starts tiles at the corner,
    /// so a centred grid moves the layer out by the part of a tile that
    /// pokes past the edge.
    // ponytail: the shifted layer relies on its parent clipping (Figma's
    // pattern frames do); an unclipped one shows up to a tile outside.
    pub fn lay_tiles(&mut self, picture: (f64, f64)) {
        let Some(tile) = self.tile else { return };
        let (w, h) = (picture.0 * tile.factor, picture.1 * tile.factor);
        if w < 0.5 || h < 0.5 {
            return;
        }
        self.set("TileSize", udim2(w, h));
        let shift = |centred: bool, size: f64, t: f64| {
            let phase = ((size - t) / 2.0).rem_euclid(t);
            if centred && phase > 0.01 {
                t - phase
            } else {
                0.0
            }
        };
        let dx = shift(tile.centred.0, tile.size.0, w);
        let dy = shift(tile.centred.1, tile.size.1, h);
        if dx > 0.0 || dy > 0.0 {
            self.set("Position", udim2(-dx, -dy));
            self.set(
                "Size",
                Variant::UDim2(UDim2 {
                    x: udim(1.0, dx),
                    y: udim(1.0, dy),
                }),
            );
        }
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

    /// A note that also caps the confidence at `level`.
    fn concern(&mut self, why: impl Into<String>, level: Confidence) {
        self.note(why);
        self.confidence = self.confidence.min(level);
    }

    /// Something drawn close to, but not exactly like, Figma.
    fn approx(&mut self, why: impl Into<String>) {
        self.concern(why, Confidence::Medium);
    }

    /// A guess, a flattening, or something dropped.
    fn doubt(&mut self, why: impl Into<String>) {
        self.concern(why, Confidence::Low);
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
    horizontal: bool,
    grid: bool,
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

const WHITE: Color3Data = Color3Data {
    r: 1.0,
    g: 1.0,
    b: 1.0,
};

fn hex(c: Color3Data) -> String {
    let byte = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", byte(c.r), byte(c.g), byte(c.b))
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

fn full() -> Variant {
    Variant::UDim2(UDim2 {
        x: udim(1.0, 0.0),
        y: udim(1.0, 0.0),
    })
}

fn is_gradient(fill: &Value) -> bool {
    text(fill, "type").starts_with("GRADIENT_")
}

const VECTORS: [&str; 5] = [
    "VECTOR",
    "BOOLEAN_OPERATION",
    "STAR",
    "LINE",
    "REGULAR_POLYGON",
];

/// Node types read as a GUI object without a doubt.
const CONTAINERS: [&str; 8] = [
    "FRAME",
    "GROUP",
    "RECTANGLE",
    "ELLIPSE",
    "COMPONENT",
    "INSTANCE",
    "COMPONENT_SET",
    "SECTION",
];

/// FigJam and embed node types with no GUI equivalent: rendered.
const RENDERED: [&str; 13] = [
    "STICKY",
    "SHAPE_WITH_TEXT",
    "CONNECTOR",
    "TABLE",
    "TABLE_CELL",
    "EMBED",
    "LINK_UNFURL",
    "MEDIA",
    "WIDGET",
    "STAMP",
    "WASHI_TAPE",
    "HIGHLIGHT",
    "CODE_BLOCK",
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

fn blended(b: &str) -> bool {
    !matches!(b, "" | "NORMAL" | "PASS_THROUGH")
}

const UNDRAWABLE_EFFECTS: [&str; 3] = ["INNER_SHADOW", "LAYER_BLUR", "BACKGROUND_BLUR"];

/// Why Roblox can't draw this leaf's own paint, if it can't.
fn unsupported(v: &Value, fills: &[&Value]) -> Option<String> {
    if fills.len() > 1 {
        return Some(format!("{} stacked fills", fills.len()));
    }
    if blended(text(v, "blendMode")) || fills.iter().any(|f| blended(text(f, "blendMode"))) {
        return Some("a blend mode".into());
    }
    if let Some(kind) = fills
        .iter()
        .map(|f| text(f, "type"))
        .find(|k| matches!(*k, "GRADIENT_ANGULAR" | "VIDEO"))
    {
        return Some(format!(
            "a {} fill",
            kind.trim_start_matches("GRADIENT_").to_lowercase()
        ));
    }
    if let Some(arc) = v.get("arcData") {
        let start = num(arc, "startingAngle").unwrap_or(0.0);
        let end = num(arc, "endingAngle").unwrap_or(std::f64::consts::TAU);
        if num(arc, "innerRadius").unwrap_or(0.0) > 0.0
            || ((end - start).abs() - std::f64::consts::TAU).abs() > 1e-3
        {
            return Some("an arc".into());
        }
    }
    list(v, "effects")
        .iter()
        .map(|e| text(e, "type"))
        .find(|k| UNDRAWABLE_EFFECTS.contains(k))
        .map(|k| format!("an effect ({})", k.to_lowercase().replace('_', " ")))
}

/// Whether the node is drawn as one PNG: `Some(None)` when that is exact
/// (vectors), `Some(Some(why))` when it is a fallback.
fn render_reason(v: &Value, kind: &str, fills: &[&Value], nested: bool) -> Option<Option<String>> {
    if kind == "TEXT" {
        return None;
    }
    if all_vector(v) && (nested || VECTORS.contains(&kind)) {
        return Some(None);
    }
    if RENDERED.contains(&kind) {
        return Some(Some(format!(
            "Figma {} has no Roblox equivalent",
            kind.to_lowercase().replace('_', " ")
        )));
    }
    let children = list(v, "children");
    if children
        .iter()
        .any(|c| c.get("isMask").and_then(Value::as_bool) == Some(true))
    {
        return Some(Some("it holds a mask".into()));
    }
    if children.is_empty() {
        return unsupported(v, fills).map(Some);
    }
    None
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
        let mut size = UDim2 {
            x: udim(0.0, rect.w),
            y: udim(0.0, rect.h),
        };
        if text(v, "layoutPositioning") == "ABSOLUTE" {
            out.doubt("placed freely inside an auto-layout frame; the layout places it in Roblox");
        } else {
            let (sx, sy) = (
                text(v, "layoutSizingHorizontal"),
                text(v, "layoutSizingVertical"),
            );
            let (main, cross) = if parent.horizontal {
                (sx, sy)
            } else {
                (sy, sx)
            };
            let stretch = cross == "FILL" || text(v, "layoutAlign") == "STRETCH";
            let grow = main == "FILL" || num(v, "layoutGrow").unwrap_or(0.0) > 0.0;
            if parent.grid {
                if grow || stretch {
                    out.approx("fills its row in Figma; grid cells have one fixed size");
                }
            } else {
                if stretch {
                    if parent.horizontal {
                        size.y = udim(1.0, 0.0);
                    } else {
                        size.x = udim(1.0, 0.0);
                    }
                }
                if grow {
                    let mut flex = Node::new("UIFlexItem", "UIFlexItem");
                    flex.set("FlexMode", Variant::Enum(3));
                    out.children.push(flex);
                }
            }
        }
        out.set("Size", Variant::UDim2(size));
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

/// HUG sizing as `AutomaticSize`; whether either axis hugs.
fn hug(out: &mut Node, v: &Value) -> bool {
    let x = text(v, "layoutSizingHorizontal") == "HUG";
    let y = text(v, "layoutSizingVertical") == "HUG";
    let mode = x as u32 + 2 * y as u32;
    if mode != 0 {
        out.set("AutomaticSize", Variant::Enum(mode));
    }
    mode != 0
}

/// `minWidth` and friends as a `UISizeConstraint`.
fn limits(out: &mut Node, v: &Value) {
    let [min_w, min_h, max_w, max_h] =
        ["minWidth", "minHeight", "maxWidth", "maxHeight"].map(|k| num(v, k));
    if [min_w, min_h, max_w, max_h].iter().all(Option::is_none) {
        return;
    }
    let mut limit = Node::new("UISizeConstraint", "UISizeConstraint");
    let vec = |x: Option<f64>, y: Option<f64>, or: f32| {
        Variant::Vector2(Vector2Data {
            x: x.map_or(or, |x| x as f32),
            y: y.map_or(or, |y| y as f32),
        })
    };
    limit.set("MinSize", vec(min_w, min_h, 0.0));
    if max_w.is_some() || max_h.is_some() {
        limit.set("MaxSize", vec(max_w, max_h, f32::INFINITY));
    }
    out.children.push(limit);
}

/// The node's rotation as Roblox wants it (degrees, clockwise), and whether
/// its transform mirrors it. Figma's `relativeTransform` is
/// `[[cos θ, sin θ, x], [-sin θ, cos θ, y]]` for a counter-clockwise θ in a
/// y-down space, so the x axis lands at `atan2(m10, m00)` clockwise. Without
/// it, `rotation` (radians) already turns clockwise on screen: FigBloxUI's
/// spring (`6:3`, `rotation` -1.309) renders turned 75° counter-clockwise.
fn rotation_of(v: &Value) -> (f64, bool) {
    if let Some(m) = v.get("relativeTransform").and_then(Value::as_array) {
        let at = |r: usize, c: usize| {
            m.get(r)
                .and_then(|row| row.get(c))
                .and_then(Value::as_f64)
                .unwrap_or(if r == c { 1.0 } else { 0.0 })
        };
        let (a, b, c, d) = (at(0, 0), at(0, 1), at(1, 0), at(1, 1));
        return (c.atan2(a).to_degrees(), a * d - b * c < 0.0);
    }
    (num(v, "rotation").unwrap_or(0.0).to_degrees(), false)
}

/// The unrotated box, centred where the rotated bounding box is: from `size`
/// when Figma sent it, else solved from the bounding box (impossible near 45°).
fn unrotated(v: &Value, aabb: Rect, degrees: f64) -> Option<Rect> {
    let (w, h) = match v.get("size") {
        Some(s) => (num(s, "x")?, num(s, "y")?),
        None => {
            let (s, c) = degrees.to_radians().sin_cos();
            let (s, c) = (s.abs(), c.abs());
            let det = c * c - s * s;
            if det.abs() < 0.05 {
                return None;
            }
            (
                (aabb.w * c - aabb.h * s) / det,
                (aabb.h * c - aabb.w * s) / det,
            )
        }
    };
    Some(Rect {
        x: aabb.x + aabb.w / 2.0 - w / 2.0,
        y: aabb.y + aabb.h / 2.0 - h / 2.0,
        w,
        h,
    })
}

fn design_node(v: &Value, parent: Option<&Parent>, inherited: f64, order: i32) -> Option<Node> {
    let kind = text(v, "type");
    if !visible(v) || kind == "SLICE" {
        return None;
    }
    let name = text(v, "name");
    let id = text(v, "id");
    let opacity = inherited * num(v, "opacity").unwrap_or(1.0);
    let aabb = rect_of(v, "absoluteBoundingBox").unwrap_or_default();
    let fills = list(v, "fills");
    let children = list(v, "children");
    let lower = name.to_lowercase();

    // A picture of the node: vectors, and nodes whose paint Roblox lacks.
    if let Some(why) = render_reason(v, kind, &fills, parent.is_some()) {
        let mut out = Node::new("ImageLabel", name);
        let bounds = rect_of(v, "absoluteRenderBounds").unwrap_or(aabb);
        place(&mut out, v, bounds, parent, order);
        out.set("BackgroundTransparency", Variant::Float32(1.0));
        out.set("BorderSizePixel", Variant::Int32(0));
        out.set(
            "ImageTransparency",
            Variant::Float32((1.0 - inherited) as f32),
        );
        out.image = Some(Image::Render(id.to_string()));
        if let Some(why) = why {
            out.doubt(format!("flattened to an image: {why}"));
        }
        return Some(out);
    }

    let (degrees, mirrored) = rotation_of(v);
    let rotated = !mirrored && degrees.abs() > 0.01;
    let solved = rotated.then(|| unrotated(v, aabb, degrees)).flatten();
    let rect = solved.unwrap_or(aabb);
    let rotate = |out: &mut Node| {
        if mirrored {
            out.doubt("mirrored in Figma; Roblox can't mirror, so it isn't");
        }
        if !rotated {
            return;
        }
        out.set("Rotation", Variant::Float32(degrees as f32));
        out.approx(format!(
            "rotated {degrees:.0}°; placed by its unrotated box"
        ));
        if solved.is_none() {
            out.approx("size unknown at this angle; the rotated bounds stand in");
        }
        if !list(v, "children").is_empty() {
            out.approx("children placed from their rotated bounds");
        }
    };

    if kind == "TEXT" {
        let mut out = text_node(v, parent, rect, opacity, order, &lower);
        rotate(&mut out);
        return Some(out);
    }

    let leaf = children.is_empty();
    let image_leaf = leaf && fills.last().is_some_and(|f| text(f, "type") == "IMAGE");
    let component = matches!(kind, "COMPONENT" | "INSTANCE" | "COMPONENT_SET");
    let button_name = lower.contains("button")
        || lower
            .split(|c: char| !c.is_alphanumeric())
            .any(|w| w == "btn");
    let variants = lower.contains("hover") || lower.contains("pressed");
    let clicked = clicks(v);
    let button = clicked || (component && (button_name || variants));
    let clips = v.get("clipsContent").and_then(Value::as_bool) == Some(true);
    let child_rects: Vec<Rect> = children
        .iter()
        .filter_map(|c| rect_of(c, "absoluteBoundingBox"))
        .collect();
    let overflows = clips
        && child_rects.iter().any(|c| {
            c.x < rect.x - 0.5
                || c.y < rect.y - 0.5
                || c.x + c.w > rect.x + rect.w + 0.5
                || c.y + c.h > rect.y + rect.h + 0.5
        });
    let class = match () {
        _ if button && image_leaf => "ImageButton",
        _ if button && has_text(v) => "TextButton",
        _ if button => "ImageButton",
        _ if image_leaf => "ImageLabel",
        _ if overflows => "ScrollingFrame",
        _ => "Frame",
    };
    let mut out = Node::new(class, name);
    place(&mut out, v, rect, parent, order);
    rotate(&mut out);
    out.set("BorderSizePixel", Variant::Int32(0));
    if !CONTAINERS.contains(&kind) {
        out.doubt(format!("Figma {} read as a {class}", kind.to_lowercase()));
    }
    if button && !clicked {
        out.doubt(format!("named like a button, so made a {class}"));
    }
    if class == "TextButton" {
        out.set("Text", Variant::String(String::new()));
    }
    hug(&mut out, v);
    limits(&mut out, v);

    let corner = rounding(&mut out, v, kind, rect);

    // Paint: a leaf's image is its own; a container keeps a solid or
    // gradient bottom fill as its background and layers the rest under its
    // content.
    out.set("BackgroundTransparency", Variant::Float32(1.0));
    let mut layers = Vec::new();
    if image_leaf {
        image_fill(&mut out, fills[0], rect, opacity);
    } else {
        let mut rest = &fills[..];
        if let Some(bottom) = fills
            .first()
            .filter(|f| text(f, "type") == "SOLID" || is_gradient(f))
        {
            paint(&mut out, bottom, rect, opacity);
            rest = &fills[1..];
        }
        let count = rest.len();
        for (k, fill) in rest.iter().enumerate() {
            let suffix = if count == 1 {
                "#fill".to_string()
            } else {
                format!("#fill{}", k + 1)
            };
            let mut layer = match text(fill, "type") {
                "IMAGE" => {
                    let mut layer = Node::new("ImageLabel", &format!("{name}Image"));
                    image_fill(&mut layer, fill, rect, opacity);
                    layer
                }
                "PATTERN" => {
                    let mut layer = Node::new("ImageLabel", &format!("{name}Pattern"));
                    pattern(&mut layer, fill, rect, opacity);
                    layer
                }
                "SOLID" | "GRADIENT_LINEAR" | "GRADIENT_RADIAL" | "GRADIENT_DIAMOND"
                | "GRADIENT_ANGULAR" => {
                    let mut layer = Node::new("Frame", &format!("{name}Fill"));
                    layer.set("BackgroundTransparency", Variant::Float32(1.0));
                    paint(&mut layer, fill, rect, opacity);
                    layer
                }
                other => {
                    out.doubt(format!(
                        "a {} fill dropped; Roblox can't draw it under children",
                        other.to_lowercase()
                    ));
                    continue;
                }
            };
            layer.id = format!("{id}{suffix}");
            layer.set("Size", full());
            layer.set("BorderSizePixel", Variant::Int32(0));
            layer.set("ZIndex", Variant::Int32(k as i32 - count as i32 + 1));
            if layer.get("BackgroundTransparency").is_none() {
                layer.set("BackgroundTransparency", Variant::Float32(1.0));
            }
            if let Some((c, _)) = &corner {
                layer.children.push(c.clone());
            }
            layers.push(layer);
        }
    }
    if clips {
        out.set("ClipsDescendants", Variant::Bool(true));
        // UICorner rounds a frame's own paint but never clips its children;
        // a CanvasGroup clips to its UICorner.
        if let Some((_, radius)) = &corner {
            if child_rects
                .iter()
                .any(|c| reaches_corner(rect, *radius, *c))
            {
                if class == "Frame" {
                    out.class = "CanvasGroup";
                    out.approx(
                        "a CanvasGroup, so its rounded corners clip its children (UICorner alone doesn't)",
                    );
                } else {
                    out.approx(format!(
                        "children reach into the rounded corners, which don't clip a {class} in Roblox"
                    ));
                }
            }
        }
    }
    if let Some((c, _)) = corner {
        out.children.push(c);
    }
    stroke(&mut out, v, opacity, false, rect);
    shadow(&mut out, v);
    if !leaf {
        if blended(text(v, "blendMode")) || fills.iter().any(|f| blended(text(f, "blendMode"))) {
            out.doubt("blend mode dropped");
        }
        for effect in list(v, "effects") {
            let kind = text(effect, "type");
            if UNDRAWABLE_EFFECTS.contains(&kind) {
                out.doubt(format!("{} dropped", kind.to_lowercase().replace('_', " ")));
            }
        }
        if num(v, "opacity").unwrap_or(1.0) < 0.999 {
            out.approx("opacity multiplied into each child; overlaps show through");
        }
    }

    let mode = text(v, "layoutMode");
    let auto_layout = matches!(mode, "HORIZONTAL" | "VERTICAL");
    let horizontal = mode == "HORIZONTAL";
    let grid = auto_layout && text(v, "layoutWrap") == "WRAP";
    let modifiers = if auto_layout {
        layout(&mut out, v, horizontal, grid, &child_rects)
    } else {
        Vec::new()
    };
    if overflows {
        let (right, bottom) = child_rects.iter().fold((rect.w, rect.h), |(r, b), c| {
            (r.max(c.x + c.w - rect.x), b.max(c.y + c.h - rect.y))
        });
        out.set("CanvasSize", udim2(right, bottom));
    }

    let here = Parent {
        rect,
        auto_layout,
        horizontal,
        grid,
        name,
    };
    let kids: Vec<Node> = children
        .iter()
        .enumerate()
        .filter_map(|(i, child)| node(child, Some(&here), opacity, i as i32 + 1))
        .collect();

    let mut all = layers;
    let layered = !all.is_empty();
    all.append(&mut out.children);
    if auto_layout && layered {
        // The layout would lay the fill layers out as items: the content
        // moves into a frame of its own.
        let mut content = Node::new("Frame", &format!("{name}Content"));
        content.id = format!("{id}#content");
        content.set("Size", full());
        content.set("BackgroundTransparency", Variant::Float32(1.0));
        content.set("BorderSizePixel", Variant::Int32(0));
        content.children = modifiers.into_iter().chain(kids).collect();
        all.push(content);
        out.approx("fill layers sit beside a Content frame holding the auto layout");
    } else {
        all.extend(modifiers);
        all.extend(kids);
    }
    out.children = all;
    Some(out)
}

/// Whether `child` pokes outside `frame`'s rounded corners of `radius`.
fn reaches_corner(frame: Rect, radius: f64, child: Rect) -> bool {
    let r = radius.min(frame.w / 2.0).min(frame.h / 2.0);
    let (x0, y0) = (child.x - frame.x, child.y - frame.y);
    let (x1, y1) = (x0 + child.w, y0 + child.h);
    [
        (0.0, 0.0, 1.0, 1.0),
        (frame.w, 0.0, -1.0, 1.0),
        (0.0, frame.h, 1.0, -1.0),
        (frame.w, frame.h, -1.0, -1.0),
    ]
    .iter()
    .any(|&(cx, cy, sx, sy)| {
        // The child in a frame where this corner is the origin.
        let (u0, u1) = (
            ((x0 - cx) * sx).min((x1 - cx) * sx),
            ((x0 - cx) * sx).max((x1 - cx) * sx),
        );
        let (v0, v1) = (
            ((y0 - cy) * sy).min((y1 - cy) * sy),
            ((y0 - cy) * sy).max((y1 - cy) * sy),
        );
        if u0 >= r || v0 >= r || u1 <= 0.0 || v1 <= 0.0 {
            return false;
        }
        let (px, py) = (u0.max(0.0), v0.max(0.0));
        (px - r).hypot(py - r) > r + 0.5
    })
}

/// The node's `UICorner` and its radius in pixels.
fn rounding(out: &mut Node, v: &Value, kind: &str, rect: Rect) -> Option<(Node, f64)> {
    if kind == "ELLIPSE" {
        return Some((corner(0.5, 0.0), rect.w.min(rect.h) / 2.0));
    }
    let radii: Vec<f64> = v
        .get("rectangleCornerRadii")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_f64).collect())
        .unwrap_or_default();
    let largest = radii.iter().copied().fold(0.0, f64::max);
    let radius = if radii.iter().any(|r| (r - largest).abs() > 0.01) {
        out.approx(format!(
            "per-corner radii {radii:?}; UICorner rounds all four at {largest}"
        ));
        largest
    } else {
        num(v, "cornerRadius").unwrap_or(largest)
    };
    if radius <= 0.0 {
        return None;
    }
    if num(v, "cornerSmoothing").unwrap_or(0.0) > 0.0 {
        out.approx("smoothed corners drawn round");
    }
    Some((corner(0.0, radius), radius))
}

/// A solid or gradient paint as the node's background.
fn paint(out: &mut Node, fill: &Value, rect: Rect, opacity: f64) {
    if text(fill, "type") == "SOLID" {
        let (c, a) = color(fill.get("color").unwrap_or(&Value::Null));
        out.set("BackgroundColor3", Variant::Color3(c));
        out.set(
            "BackgroundTransparency",
            Variant::Float32((1.0 - a * num(fill, "opacity").unwrap_or(1.0) * opacity) as f32),
        );
        return;
    }
    // The gradient carries the fill's colours and opacity; white lets them
    // through untinted.
    out.set("BackgroundColor3", Variant::Color3(WHITE));
    out.set(
        "BackgroundTransparency",
        Variant::Float32((1.0 - opacity) as f32),
    );
    let (g, concern) = gradient(fill, rect);
    out.children.push(g);
    if let Some((why, level)) = concern {
        out.concern(why, level);
    }
}

/// A PATTERN fill: its source node, rendered, tiled over the box. Figma
/// scales the source by `scalingFactor` and centres or starts the tiles per
/// axis; the tile size waits for the render (see [`Tile`]).
fn pattern(out: &mut Node, fill: &Value, rect: Rect, opacity: f64) {
    out.set(
        "ImageTransparency",
        Variant::Float32((1.0 - num(fill, "opacity").unwrap_or(1.0) * opacity) as f32),
    );
    out.set("ScaleType", Variant::Enum(2));
    let factor = num(fill, "scalingFactor").unwrap_or(1.0);
    out.set("TileSize", udim2(rect.w * factor, rect.h * factor));
    out.tile = Some(Tile {
        factor,
        size: (rect.w, rect.h),
        centred: (
            text(fill, "horizontalAlignment") == "CENTER",
            text(fill, "verticalAlignment") == "CENTER",
        ),
    });
    if text(fill, "tileType") != "RECTANGULAR" {
        out.approx("hexagonal pattern tiled as a grid");
    }
    let spacing = fill.get("spacing").unwrap_or(&Value::Null);
    if num(spacing, "x").unwrap_or(0.0).abs() > 0.01
        || num(spacing, "y").unwrap_or(0.0).abs() > 0.01
    {
        out.approx("pattern spacing dropped; tiles touch");
    }
    if text(fill, "horizontalAlignment") == "END" || text(fill, "verticalAlignment") == "END" {
        out.approx("pattern aligned to its start, not its end");
    }
    out.image = Some(Image::Render(text(fill, "sourceNodeId").to_string()));
}

/// An image fill on an `ImageLabel` / `ImageButton`.
///
/// TILE's `TileSize` is Figma's `scalingFactor` times the image's pixel
/// size, which only the importer knows once it has downloaded the image
/// (see [`Tile`]): until then the node's size stands in.
fn image_fill(out: &mut Node, fill: &Value, rect: Rect, opacity: f64) {
    out.set(
        "ImageTransparency",
        Variant::Float32((1.0 - num(fill, "opacity").unwrap_or(1.0) * opacity) as f32),
    );
    let mode = text(fill, "scaleMode");
    let scale = match mode {
        "FIT" => 3,
        "TILE" => 2,
        "STRETCH" | "CROP" => 0,
        _ => 4,
    };
    out.set("ScaleType", Variant::Enum(scale));
    let cropped = fill.get("imageTransform").is_some();
    if mode == "TILE" {
        let factor = num(fill, "scalingFactor").unwrap_or(1.0);
        out.set("TileSize", udim2(rect.w * factor, rect.h * factor));
        out.tile = Some(Tile {
            factor,
            size: (rect.w, rect.h),
            centred: (false, false),
        });
    } else if cropped || mode == "CROP" {
        out.approx("cropped image shown whole; its crop needs the image's pixel size");
    }
    if num(fill, "rotation").unwrap_or(0.0).abs() > 0.01 {
        out.approx("image fill rotation dropped");
    }
    if fill.get("filters").is_some_and(|f| {
        f.as_object()
            .is_some_and(|m| m.values().any(|x| x.as_f64().unwrap_or(0.0) != 0.0))
    }) {
        out.approx("image filters (exposure, contrast…) dropped");
    }
    out.image = Some(Image::Fill(text(fill, "imageRef").to_string()));
}

const INPUT_HINTS: [&str; 5] = ["input", "textbox", "textfield", "text field", "placeholder"];

/// Roblox's `TextSize` per Figma `fontSize`: Figma sizes an em, Roblox a
/// line ("the line height equal to the `TextSize` property", Roblox's docs on
/// `Font`), which Studio captures put at 1.2 ems for every family measured
/// (the viewer's `LINE_EM`). Without it every label draws a sixth small.
const LINE_EM: f64 = 1.2;

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

/// The Roblox family for a Figma one, if Roblox has it.
fn family(name: &str) -> Option<&'static str> {
    let key: String = name.chars().filter(|c| !c.is_whitespace()).collect();
    FONTS
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(&key))
        .map(|(_, f)| *f)
}

fn weight(style: &Value) -> u16 {
    let w = num(style, "fontWeight").unwrap_or(400.0);
    ((w / 100.0).round() * 100.0).clamp(100.0, 900.0) as u16
}

fn font(style: &Value) -> (Font, Option<String>) {
    let name = text(style, "fontFamily");
    let found = family(name);
    let font = Font {
        family: format!(
            "rbxasset://fonts/families/{}.json",
            found.unwrap_or("BuilderSans")
        ),
        weight: weight(style),
        style: if style.get("italic").and_then(Value::as_bool) == Some(true) {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        },
        cached_face_id: None,
    };
    let note = match found {
        None if !name.is_empty() => Some(format!(
            "font {name} isn\u{2019}t on Roblox; Builder Sans stands in"
        )),
        _ => None,
    };
    (font, note)
}

/// Text sizing: Roblox's `TextScaled` ignores `TextSize` and fits the text
/// to its box, so text shrinks with its box; the `UITextSizeConstraint`
/// caps it at Figma's size, so at the design's resolution it matches Figma
/// exactly and never grows past it. `TextSize` is still the design size for
/// whoever turns scaling off. Hugging text instead sizes its box to the
/// text (`AutomaticSize`), which scaling would fight, so it keeps a fixed
/// `TextSize`.
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
    let em = num(style, "fontSize").unwrap_or(14.0);
    let size = em * LINE_EM;
    if input {
        out.set(
            "PlaceholderText",
            Variant::String(text(v, "characters").to_string()),
        );
        out.set("Text", Variant::String(String::new()));
        out.set("ClearTextOnFocus", Variant::Bool(false));
        out.doubt("named like an input, so made a TextBox");
    } else {
        let (body, rich) = rich_text(&mut out, v, style);
        out.set("Text", Variant::String(body));
        if rich {
            out.set("RichText", Variant::Bool(true));
        }
    }
    out.set("TextSize", Variant::Float32(size.round() as f32));
    text_paint(&mut out, v, rect, opacity, input);
    let x = match text(style, "textAlignHorizontal") {
        "RIGHT" => 1,
        "CENTER" => 2,
        "JUSTIFIED" => {
            out.approx("justified text drawn left-aligned");
            0
        }
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
        out.approx(note);
    }
    if text(style, "lineHeightUnit") != "INTRINSIC_%" && size > 0.0 {
        // Figma's line pitch over Roblox's line (`TextSize`).
        let ratio = num(style, "lineHeightPercentFontSize")
            .map(|p| p / 100.0 / LINE_EM)
            .or_else(|| num(style, "lineHeightPx").map(|px| px / size));
        if let Some(ratio) = ratio {
            let kept = ratio.clamp(1.0, 3.0);
            out.set("LineHeight", Variant::Float32(kept as f32));
            if (kept - ratio).abs() > 0.01 {
                out.approx(format!("line height {ratio:.2}× clamped to {kept}×"));
            }
        }
    }
    if !hug(&mut out, v) {
        out.set("TextScaled", Variant::Bool(true));
        let mut cap = Node::new("UITextSizeConstraint", "UITextSizeConstraint");
        cap.set("MaxTextSize", Variant::Int32(size.round().max(1.0) as i32));
        cap.set("MinTextSize", Variant::Int32(1));
        out.children.push(cap);
    }
    limits(&mut out, v);
    if text(style, "textTruncation") == "ENDING" {
        out.set("TextTruncate", Variant::Enum(1));
    }
    if num(style, "maxLines").is_some_and(|n| n > 0.0) {
        out.approx("max lines dropped; the box height limits the lines");
    }
    if num(style, "letterSpacing").unwrap_or(0.0).abs() > 0.01 {
        out.approx("letter spacing dropped");
    }
    if num(style, "paragraphSpacing").unwrap_or(0.0) > 0.0
        || num(style, "paragraphIndent").unwrap_or(0.0) > 0.0
    {
        out.approx("paragraph spacing / indent dropped");
    }
    if style.get("hyperlink").is_some_and(|h| !h.is_null()) {
        out.approx("hyperlink dropped; plain text");
    }
    if style
        .get("opentypeFlags")
        .and_then(Value::as_object)
        .is_some_and(|m| !m.is_empty())
    {
        out.approx("OpenType features dropped");
    }
    if v.get("lineTypes")
        .and_then(Value::as_array)
        .is_some_and(|t| t.iter().any(|t| t.as_str().is_some_and(|t| t != "NONE")))
    {
        out.approx("list bullets / numbers dropped");
    }
    stroke(&mut out, v, opacity, true, rect);
    out
}

/// The top text fill as the label's colour; a gradient tints white text.
fn text_paint(out: &mut Node, v: &Value, rect: Rect, opacity: f64, input: bool) {
    let fills = list(v, "fills");
    let Some(fill) = fills.last() else {
        return;
    };
    if fills.len() > 1 {
        out.approx("stacked text fills; only the top one kept");
    }
    let tint = |out: &mut Node, c: Color3Data| {
        if input {
            out.set("PlaceholderColor3", Variant::Color3(c));
        }
        out.set("TextColor3", Variant::Color3(c));
    };
    match text(fill, "type") {
        "SOLID" => {
            let (c, a) = color(fill.get("color").unwrap_or(&Value::Null));
            tint(out, c);
            let alpha = a * num(fill, "opacity").unwrap_or(1.0) * opacity;
            out.set("TextTransparency", Variant::Float32((1.0 - alpha) as f32));
        }
        _ if is_gradient(fill) => {
            tint(out, WHITE);
            out.set("TextTransparency", Variant::Float32((1.0 - opacity) as f32));
            let (g, concern) = gradient(fill, rect);
            out.children.push(g);
            if let Some((why, level)) = concern {
                out.concern(why, level);
            }
        }
        other => out.doubt(format!(
            "{} text fill can't be drawn; the default colour stands in",
            other.to_lowercase()
        )),
    }
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// `ch` in Figma's `textCase`, `prev` being the character before it.
fn cased(ch: char, prev: Option<char>, case: &str) -> String {
    match case {
        "UPPER" => ch.to_uppercase().collect(),
        "LOWER" | "SMALL_CAPS_FORCED" => ch.to_lowercase().collect(),
        "TITLE" if prev.is_none_or(|p| !p.is_alphanumeric()) => ch.to_uppercase().collect(),
        _ => ch.to_string(),
    }
}

/// The override style `id` points at, if it changes anything.
fn lookup(table: Option<&Value>, id: u64) -> Option<&Value> {
    if id == 0 {
        return None;
    }
    table?
        .get(id.to_string())
        .filter(|o| o.as_object().is_some_and(|m| !m.is_empty()))
}

/// The label's text and whether it needs `RichText`: Figma's characters
/// cased, with markup around each run whose style overrides the base one
/// (which stays on the label's own properties). `characterStyleOverrides`
/// holds one style id per UTF-16 code unit, so a character outside the BMP
/// takes two slots; ids past the end of the list mean the base style.
fn rich_text(out: &mut Node, v: &Value, base: &Value) -> (String, bool) {
    let overrides: Vec<u64> = v
        .get("characterStyleOverrides")
        .and_then(Value::as_array)
        .map(|a| a.iter().map(|x| x.as_u64().unwrap_or(0)).collect())
        .unwrap_or_default();
    let table = v.get("styleOverrideTable");
    let base_case = text(base, "textCase");
    let mut runs: Vec<(u64, String)> = Vec::new();
    let (mut unit, mut prev) = (0, None);
    for ch in text(v, "characters").chars() {
        let id = overrides.get(unit).copied().unwrap_or(0);
        unit += ch.len_utf16();
        let case = lookup(table, id)
            .map(|o| text(o, "textCase"))
            .filter(|c| !c.is_empty())
            .unwrap_or(base_case);
        let piece = cased(ch, prev, case);
        prev = Some(ch);
        match runs.last_mut() {
            Some((last, s)) if *last == id => s.push_str(&piece),
            _ => runs.push((id, piece)),
        }
    }
    let decoration = text(base, "textDecoration");
    let small_caps = base_case.starts_with("SMALL_CAPS");
    let styled = runs.iter().any(|(id, _)| lookup(table, *id).is_some());
    if !styled && !small_caps && !matches!(decoration, "UNDERLINE" | "STRIKETHROUGH") {
        return (runs.into_iter().map(|(_, s)| s).collect(), false);
    }
    let mut body = String::new();
    for (id, s) in &runs {
        let escaped = escape(s);
        match lookup(table, *id) {
            Some(o) => body.push_str(&styled_run(out, o, &escaped)),
            None => body.push_str(&escaped),
        }
    }
    let wrap = |tag: &str, s: String| format!("<{tag}>{s}</{tag}>");
    if small_caps {
        body = wrap("sc", body);
    }
    match decoration {
        "UNDERLINE" => body = wrap("u", body),
        "STRIKETHROUGH" => body = wrap("s", body),
        _ => {}
    }
    (body, true)
}

/// `inner` (escaped) in the markup for override style `o`.
fn styled_run(out: &mut Node, o: &Value, inner: &str) -> String {
    let mut attrs = String::new();
    if let Some(fill) = list(o, "fills").last() {
        let paint = match text(fill, "type") {
            "SOLID" => Some(color(fill.get("color").unwrap_or(&Value::Null))),
            _ if is_gradient(fill) => {
                out.approx("a gradient text run drawn in its first colour");
                list(fill, "gradientStops")
                    .first()
                    .map(|s| color(s.get("color").unwrap_or(&Value::Null)))
            }
            other => {
                out.doubt(format!(
                    "{} paint on a text run dropped",
                    other.to_lowercase()
                ));
                None
            }
        };
        if let Some((c, a)) = paint {
            attrs.push_str(&format!(" color=\"{}\"", hex(c)));
            let t = 1.0 - a * num(fill, "opacity").unwrap_or(1.0);
            if t > 0.001 {
                attrs.push_str(&format!(" transparency=\"{t:.3}\""));
            }
        }
    }
    if let Some(size) = num(o, "fontSize") {
        attrs.push_str(&format!(" size=\"{}\"", (size * LINE_EM).round()));
    }
    if o.get("fontWeight").is_some() {
        attrs.push_str(&format!(" weight=\"{}\"", weight(o)));
    }
    let name = text(o, "fontFamily");
    if !name.is_empty() {
        let face = family(name).unwrap_or_else(|| {
            out.approx(format!(
                "font {name} isn\u{2019}t on Roblox; Builder Sans stands in"
            ));
            "BuilderSans"
        });
        attrs.push_str(&format!(" face=\"{face}\""));
    }
    let mut s = inner.to_string();
    if let Some(paint) = list(o, "strokes")
        .last()
        .filter(|p| text(p, "type") == "SOLID")
    {
        let (c, a) = color(paint.get("color").unwrap_or(&Value::Null));
        let t = 1.0 - a * num(paint, "opacity").unwrap_or(1.0);
        s = format!(
            "<stroke color=\"{}\" thickness=\"{}\" transparency=\"{t:.3}\">{s}</stroke>",
            hex(c),
            num(o, "strokeWeight").unwrap_or(1.0)
        );
    }
    if text(o, "textCase").starts_with("SMALL_CAPS") {
        s = format!("<sc>{s}</sc>");
    }
    match text(o, "textDecoration") {
        "UNDERLINE" => s = format!("<u>{s}</u>"),
        "STRIKETHROUGH" => s = format!("<s>{s}</s>"),
        _ => {}
    }
    if o.get("italic").and_then(Value::as_bool) == Some(true) {
        s = format!("<i>{s}</i>");
    }
    if num(o, "letterSpacing").is_some_and(|l| l.abs() > 0.01) {
        out.approx("letter spacing on a text run dropped");
    }
    if o.get("hyperlink").is_some_and(|h| !h.is_null()) {
        out.approx("hyperlink on a text run dropped");
    }
    if attrs.is_empty() {
        s
    } else {
        format!("<font{attrs}>{s}</font>")
    }
}

fn corner(scale: f32, radius: f64) -> Node {
    let mut out = Node::new("UICorner", "UICorner");
    out.set("CornerRadius", Variant::UDim(udim(scale, radius)));
    out
}

/// The top stroke as a `UIStroke`: around the glyphs on text
/// (`Contextual`), around the box elsewhere (`Border`, aligned by
/// `BorderStrokePosition`). A gradient stroke gets a `UIGradient` child.
fn stroke(out: &mut Node, v: &Value, opacity: f64, text_stroke: bool, rect: Rect) {
    let strokes = list(v, "strokes");
    let Some(paint) = strokes.last() else {
        return;
    };
    let weight = match v.get("individualStrokeWeights") {
        Some(sides) => {
            let w = ["top", "right", "bottom", "left"].map(|k| num(sides, k).unwrap_or(0.0));
            let most = w.iter().copied().fold(0.0, f64::max);
            if w.iter().any(|x| (x - most).abs() > 0.01) {
                out.approx(format!(
                    "per-side stroke weights; UIStroke draws {most}px all round"
                ));
            }
            most
        }
        None => num(v, "strokeWeight").unwrap_or(1.0),
    };
    if weight <= 0.0 {
        return;
    }
    if strokes.len() > 1 {
        out.approx("stacked strokes; only the top one kept");
    }
    let mut s = Node::new("UIStroke", "UIStroke");
    match text(paint, "type") {
        "SOLID" => {
            let (c, a) = color(paint.get("color").unwrap_or(&Value::Null));
            s.set("Color", Variant::Color3(c));
            s.set(
                "Transparency",
                Variant::Float32((1.0 - a * num(paint, "opacity").unwrap_or(1.0) * opacity) as f32),
            );
        }
        _ if is_gradient(paint) => {
            s.set("Color", Variant::Color3(WHITE));
            s.set("Transparency", Variant::Float32((1.0 - opacity) as f32));
            let (g, concern) = gradient(paint, rect);
            s.children.push(g);
            if let Some((why, level)) = concern {
                out.concern(format!("stroke: {why}"), level);
            }
        }
        other => {
            out.doubt(format!("{} stroke dropped", other.to_lowercase()));
            return;
        }
    }
    s.set("Thickness", Variant::Float32(weight as f32));
    s.set(
        "ApplyStrokeMode",
        Variant::Enum(if text_stroke { 0 } else { 1 }),
    );
    let align = text(v, "strokeAlign");
    if text_stroke {
        if !matches!(align, "" | "OUTSIDE") {
            out.approx(format!(
                "text stroke aligned {} in Figma; Roblox strokes text outside",
                align.to_lowercase()
            ));
        }
    } else if let Some(position) = match align {
        "OUTSIDE" => Some(0),
        "CENTER" => Some(1),
        "INSIDE" => Some(2),
        _ => None,
    } {
        s.set("BorderStrokePosition", Variant::Enum(position));
    }
    if v.get("strokeDashes")
        .and_then(Value::as_array)
        .is_some_and(|d| !d.is_empty())
    {
        out.approx("dashed stroke drawn solid");
    }
    out.children.push(s);
}

fn shadow(out: &mut Node, v: &Value) {
    let shadows: Vec<&Value> = list(v, "effects")
        .into_iter()
        .filter(|e| text(e, "type") == "DROP_SHADOW")
        .collect();
    let Some(effect) = shadows.first() else {
        return;
    };
    if shadows.len() > 1 {
        out.approx(format!(
            "{} drop shadows; only the first kept",
            shadows.len()
        ));
    }
    let (c, a) = color(effect.get("color").unwrap_or(&Value::Null));
    let at = effect.get("offset").unwrap_or(&Value::Null);
    let spread = num(effect, "spread").unwrap_or(0.0);
    let mut s = Node::new("UIShadow", "UIShadow");
    s.set("Color", Variant::Color3(c));
    s.set("Transparency", Variant::Float32((1.0 - a) as f32));
    s.set(
        "Offset",
        udim2(num(at, "x").unwrap_or(0.0), num(at, "y").unwrap_or(0.0)),
    );
    s.set("Spread", udim2(spread, spread));
    s.set(
        "BlurRadius",
        Variant::UDim(udim(0.0, num(effect, "radius").unwrap_or(0.0))),
    );
    out.children.push(s);
}

/// A gradient paint as a `UIGradient`, and the concern if it isn't linear.
///
/// Figma's `gradientHandlePositions` sit in the node's normalized (0..1)
/// space: the gradient runs from handle 0 to handle 1, with lines of equal
/// colour parallel to handle 0 → handle 2. Roblox's `UIGradient` also works
/// in normalized space: `Rotation` turns it clockwise, and it spans the
/// box's projection on that direction, corner to corner. So the Roblox
/// direction is the normal of Figma's equal-colour lines in normalized
/// space, and each stop's position is remapped onto Roblox's span; the ends
/// take the colour Figma has at the box's extreme corners. Without handle 2
/// the equal-colour lines are taken perpendicular on screen, which needs
/// the box's aspect (`rect`).
fn gradient(fill: &Value, rect: Rect) -> (Node, Option<(String, Confidence)>) {
    let kind = text(fill, "type");
    let concern = match kind {
        "GRADIENT_LINEAR" => None,
        "GRADIENT_RADIAL" | "GRADIENT_DIAMOND" => Some((
            format!(
                "{} gradient drawn as a linear one from its centre outwards",
                kind.trim_start_matches("GRADIENT_").to_lowercase()
            ),
            Confidence::Medium,
        )),
        _ => Some((
            "angular gradient drawn as a linear one".to_string(),
            Confidence::Low,
        )),
    };
    let alpha = num(fill, "opacity").unwrap_or(1.0);
    let mut stops: Vec<(f64, Color3Data, f64)> = list(fill, "gradientStops")
        .iter()
        .map(|s| {
            let (c, a) = color(s.get("color").unwrap_or(&Value::Null));
            (
                num(s, "position").unwrap_or(0.0).clamp(0.0, 1.0),
                c,
                a * alpha,
            )
        })
        .collect();
    stops.sort_by(|a, b| a.0.total_cmp(&b.0));
    if stops.is_empty() {
        stops.push((0.0, WHITE, alpha));
    }

    let handles = list(fill, "gradientHandlePositions");
    let point = |i: usize, x: f64, y: f64| {
        handles.get(i).map_or((x, y), |h| {
            (num(h, "x").unwrap_or(x), num(h, "y").unwrap_or(y))
        })
    };
    let h0 = point(0, 0.0, 0.5);
    let h1 = point(1, 1.0, 0.5);
    let d = (h1.0 - h0.0, h1.1 - h0.1);
    let (w, h) = (
        if rect.w > 0.0 { rect.w } else { 1.0 },
        if rect.h > 0.0 { rect.h } else { 1.0 },
    );
    let e = if kind == "GRADIENT_LINEAR" && handles.len() > 2 {
        let h2 = point(2, 0.0, 1.0);
        (h2.0 - h0.0, h2.1 - h0.1)
    } else {
        (-d.1 * h / w, d.0 * w / h)
    };
    let mut n = (e.1, -e.0);
    if n.0 * d.0 + n.1 * d.1 < 0.0 {
        n = (-n.0, -n.1);
    }
    let mut along = n.0 * d.0 + n.1 * d.1;
    if n.0.hypot(n.1) < 1e-9 || along.abs() < 1e-9 {
        n = (1.0, 0.0);
        along = 1.0;
    }
    let len = n.0.hypot(n.1);
    let u = (n.0 / len, n.1 / len);
    let rotation = u.1.atan2(u.0).to_degrees();
    let half = (u.0.abs() + u.1.abs()) / 2.0;
    // Figma's position at Roblox's time r.
    let figma_at = |r: f64| {
        let p = (
            0.5 + (r - 0.5) * 2.0 * half * u.0,
            0.5 + (r - 0.5) * 2.0 * half * u.1,
        );
        ((p.0 - h0.0) * n.0 + (p.1 - h0.1) * n.1) / along
    };
    let (t0, t1) = (figma_at(0.0), figma_at(1.0));
    let sample = |t: f64| -> (Color3Data, f64) {
        let first = stops[0];
        let last = stops[stops.len() - 1];
        if t <= first.0 {
            return (first.1, first.2);
        }
        if t >= last.0 {
            return (last.1, last.2);
        }
        let i = stops.iter().rposition(|s| s.0 <= t).unwrap_or(0);
        let (a, b) = (stops[i], stops[(i + 1).min(stops.len() - 1)]);
        let k = if b.0 > a.0 {
            (t - a.0) / (b.0 - a.0)
        } else {
            0.0
        };
        let mix = |x: f32, y: f32| x + (y - x) * k as f32;
        (
            Color3Data {
                r: mix(a.1.r, b.1.r),
                g: mix(a.1.g, b.1.g),
                b: mix(a.1.b, b.1.b),
            },
            a.2 + (b.2 - a.2) * k,
        )
    };
    let key = |r: f64, (c, a): (Color3Data, f64)| (r, c, a);
    let mut keys = vec![key(0.0, sample(t0))];
    for s in &stops {
        let r = (s.0 - t0) / (t1 - t0);
        if r > 0.0 && r < 1.0 {
            keys.push((r, s.1, s.2));
        }
    }
    keys.push(key(1.0, sample(t1)));
    if keys.len() > 20 {
        keys = (0..20)
            .map(|i| {
                let r = i as f64 / 19.0;
                key(r, sample(t0 + (t1 - t0) * r))
            })
            .collect();
    }
    // Strictly increasing times, ending at exactly 1.
    for i in 1..keys.len() {
        if keys[i].0 <= keys[i - 1].0 {
            keys[i].0 = keys[i - 1].0 + 0.001;
        }
    }
    let count = keys.len();
    for (i, k) in keys.iter_mut().enumerate() {
        k.0 = k.0.min(1.0 - 0.001 * (count - 1 - i) as f64);
    }
    for i in (1..count).rev() {
        if keys[i - 1].0 >= keys[i].0 {
            keys[i - 1].0 = keys[i].0 - 0.001;
        }
    }

    let mut out = Node::new("UIGradient", "UIGradient");
    out.set(
        "Color",
        Variant::ColorSequence(ColorSequence {
            keypoints: keys
                .iter()
                .map(|k| ColorSequenceKeypoint {
                    time: k.0 as f32,
                    color: k.1,
                    envelope: 0.0,
                })
                .collect(),
        }),
    );
    out.set(
        "Transparency",
        Variant::NumberSequence(NumberSequence {
            keypoints: keys
                .iter()
                .map(|k| NumberSequenceKeypoint {
                    time: k.0 as f32,
                    value: (1.0 - k.2).clamp(0.0, 1.0) as f32,
                    envelope: 0.0,
                })
                .collect(),
        }),
    );
    out.set("Rotation", Variant::Float32(rotation as f32));
    (out, concern)
}

/// Auto layout as a `UIListLayout` (a `UIGridLayout` when it wraps) and a
/// `UIPadding`.
fn layout(out: &mut Node, v: &Value, horizontal: bool, grid: bool, children: &[Rect]) -> Vec<Node> {
    let primary = text(v, "primaryAxisAlignItems");
    let counter = text(v, "counterAxisAlignItems");
    if counter == "BASELINE" {
        out.approx("baseline alignment drawn top-aligned");
    }
    // Center, Left/Top, Right/Bottom.
    let align = |a: &str| match a {
        "CENTER" => 0,
        "MAX" => 2,
        _ => 1,
    };
    let (h, vert) = if horizontal {
        (align(primary), align(counter))
    } else {
        (align(counter), align(primary))
    };
    let spacing = num(v, "itemSpacing").unwrap_or(0.0);
    let mut layout = if grid {
        let mut g = Node::new("UIGridLayout", "UIGridLayout");
        let cell = children.first().copied().unwrap_or_default();
        g.set("CellSize", udim2(cell.w, cell.h));
        let cross = num(v, "counterAxisSpacing").unwrap_or(0.0);
        g.set(
            "CellPadding",
            if horizontal {
                udim2(spacing, cross)
            } else {
                udim2(cross, spacing)
            },
        );
        if children
            .iter()
            .any(|c| (c.w - cell.w).abs() > 0.5 || (c.h - cell.h).abs() > 0.5)
        {
            out.approx("wrapping auto layout as a grid: every cell takes the first child's size");
        }
        if primary == "SPACE_BETWEEN" {
            out.approx("space-between wrap drawn with fixed gaps");
        }
        g
    } else {
        let mut l = Node::new("UIListLayout", "UIListLayout");
        l.set("Padding", Variant::UDim(udim(0.0, spacing)));
        if primary == "SPACE_BETWEEN" {
            l.set(
                if horizontal {
                    "HorizontalFlex"
                } else {
                    "VerticalFlex"
                },
                Variant::Enum(3),
            );
        }
        l
    };
    layout.set(
        "FillDirection",
        Variant::Enum(if horizontal { 0 } else { 1 }),
    );
    layout.set("SortOrder", Variant::Enum(2));
    layout.set("HorizontalAlignment", Variant::Enum(h));
    layout.set("VerticalAlignment", Variant::Enum(vert));
    let mut modifiers = vec![layout];
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
        modifiers.push(padding);
    }
    modifiers
}

#[cfg(test)]
#[path = "infer/tests.rs"]
mod tests;
