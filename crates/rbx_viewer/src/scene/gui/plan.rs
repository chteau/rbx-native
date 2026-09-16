//! Reads the `ScreenGui` trees a DOM carries into resolution-independent
//! nodes. Nothing here knows the viewport: a `UDim2` stays a scale plus an
//! offset until [`super::layout`] is handed a pixel rect to resolve it
//! against.

use std::collections::BTreeMap;

use rbx_assets::AssetRef;
use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::scene::srgb_to_linear;
use crate::textures::asset_uri;

const SCREEN_CLASS: &str = "ScreenGui";
const ELEMENT_CLASS: &str = "GuiObject";
const LIST_LAYOUT_CLASS: &str = "UIListLayout";

/// Roblox's own default `BorderColor3`, `Color3.fromRGB(27, 42, 53)`. Only
/// ever seen on a tree built in code: a place file serializes the property.
const DEFAULT_BORDER: [f32; 3] = [27.0 / 255.0, 42.0 / 255.0, 53.0 / 255.0];

/// One `UDim2`: a fraction of the parent box plus a pixel offset, per axis.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct Span {
    pub(super) scale: [f32; 2],
    pub(super) offset: [f32; 2],
}

impl Span {
    /// The pixel extent this span comes to inside a parent of `size` pixels.
    pub(super) fn against(&self, size: [f32; 2]) -> [f32; 2] {
        [
            self.scale[0] * size[0] + self.offset[0],
            self.scale[1] * size[1] + self.offset[1],
        ]
    }
}

/// How an `ImageLabel`'s image covers its box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Tiling {
    Stretch,
    /// `ScaleType.Tile`: the image repeats every `size`, itself a `UDim2`
    /// resolved against the element's own box rather than its parent's.
    Tile {
        size: Span,
    },
}

/// An `ImageLabel`'s image, before it is known whether it downloaded.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Fill {
    pub(super) asset: AssetRef,
    pub(super) tint: [f32; 3],
    pub(super) alpha: f32,
    pub(super) tiling: Tiling,
}

/// Where a `UIListLayout` puts its stack along one axis, or each item across
/// the other. `Start` is Left/Top, `End` Right/Bottom; the two Roblox enums
/// share ordinals (Center = 0, Left/Top = 1, Right/Bottom = 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Align {
    Center,
    Start,
    End,
}

/// A `UIListLayout` read off its siblings: they are stacked along one axis in
/// a sort order of their own, their `Position` ignored and their `Size` kept.
///
/// TODO: the flex family (`HorizontalFlex`/`VerticalFlex`, `Wraps`,
/// `ItemLineAlignment`) and `UIGridLayout`/`UIPageLayout`/`UITableLayout`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct List {
    pub(super) vertical: bool,
    /// `Padding`, a `UDim` resolved against the parent's extent along the
    /// fill direction.
    pub(super) padding: (f32, f32),
    pub(super) horizontal: Align,
    pub(super) vertical_align: Align,
    /// `SortOrder.Name`; otherwise `LayoutOrder`, ties in tree order.
    pub(super) by_name: bool,
}

/// One `GuiObject` and everything under it, `Visible = false` subtrees already
/// pruned away.
///
/// Text (`TextLabel`/`TextButton`/`TextBox`) is read like any other box: its
/// background and border draw, the glyphs do not.
///
/// TODO: render text once a font stack is chosen.
#[derive(Clone)]
pub(super) struct Node {
    /// Only read for `SortOrder.Name` under a [`List`].
    pub(super) name: String,
    pub(super) layout_order: i32,
    pub(super) position: Span,
    pub(super) size: Span,
    pub(super) anchor: [f32; 2],
    /// `Rotation`, in degrees around the element's own centre — Roblox gives
    /// no way to move the pivot, so `AnchorPoint` plays no part in this.
    pub(super) rotation: f32,
    pub(super) background: [f32; 3],
    pub(super) background_alpha: f32,
    /// `BorderSizePixel`, drawn just outside the box: `BorderMode.Outline` is
    /// the default and the only mode reproduced.
    ///
    /// TODO: `BorderMode.Middle`/`Inset` place the same outline differently.
    pub(super) border: f32,
    pub(super) border_color: [f32; 3],
    pub(super) clips: bool,
    pub(super) z_index: i32,
    pub(super) fill: Option<Fill>,
    /// The `UIListLayout` among this node's children, arranging them.
    pub(super) list: Option<List>,
    pub(super) children: Vec<Node>,
}

impl Node {
    /// Whether anything in this subtree puts a pixel down: a container holding
    /// only transparent text has no reason to be given a canvas.
    pub(super) fn paints(&self) -> bool {
        self.background_alpha > 0.0
            || self.fill.as_ref().is_some_and(|fill| fill.alpha > 0.0)
            || self.children.iter().any(Node::paints)
    }
}

/// One `ScreenGui`: a screen-space overlay whose top-level children resolve
/// against the viewport itself.
#[derive(Clone)]
pub(crate) struct Screen {
    pub(super) display_order: i32,
    pub(super) list: Option<List>,
    pub(super) roots: Vec<Node>,
}

impl Screen {
    /// Every image the screen wants, in first-seen paint order.
    pub(crate) fn assets(&self, into: &mut Vec<AssetRef>) {
        for root in &self.roots {
            collect_assets(root, into);
        }
    }
}

pub(super) fn collect_assets(node: &Node, into: &mut Vec<AssetRef>) {
    if let Some(fill) = &node.fill {
        if !into.contains(&fill.asset) {
            into.push(fill.asset.clone());
        }
    }
    for child in &node.children {
        collect_assets(child, into);
    }
}

/// Every enabled `ScreenGui` in the DOM, in the order the tree holds them.
///
/// The walk is its own recursion rather than [`crate::scene::descendants`]:
/// that iterator pops off a stack and so visits children backwards, while a
/// GUI's paint order among equal `ZIndex` siblings is exactly tree order.
pub(crate) fn plan(dom: &WeakDom, database: &ReflectionDatabase) -> Vec<Screen> {
    let mut screens = Vec::new();
    for &root in dom.root_refs() {
        gather(dom, database, root, &mut screens);
    }
    screens
}

fn gather(dom: &WeakDom, database: &ReflectionDatabase, referent: Ref, into: &mut Vec<Screen>) {
    let Some(instance) = dom.get(referent) else {
        return;
    };
    if database.is_subclass_of(instance.class(), SCREEN_CLASS) {
        let properties = instance.properties();
        if flag(properties, "Enabled", true) {
            into.push(Screen {
                display_order: integer(properties, "DisplayOrder", 0),
                list: list_layout(dom, database, instance.children()),
                roots: elements(dom, database, instance.children()),
            });
        }
        // A `ScreenGui` never nests inside another, and its own children are
        // already read above.
        return;
    }
    for &child in instance.children() {
        gather(dom, database, child, into);
    }
}

pub(super) fn elements(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    children: &[Ref],
) -> Vec<Node> {
    children
        .iter()
        .filter_map(|&child| element(dom, database, child))
        .collect()
}

/// One `GuiObject` read into a [`Node`], or `None` where it is not drawable.
///
/// A class this viewer has no special handling for still becomes a node: an
/// unknown `GuiObject` subclass draws its background like a `Frame` rather
/// than vanishing.
fn element(dom: &WeakDom, database: &ReflectionDatabase, referent: Ref) -> Option<Node> {
    let instance = dom.get(referent)?;
    let class = instance.class();
    if !database.is_subclass_of(class, ELEMENT_CLASS) {
        return None;
    }

    let properties = instance.properties();
    // Roblox hides a whole subtree behind an invisible ancestor, so the
    // children never have to be read at all.
    if !flag(properties, "Visible", true) {
        return None;
    }

    Some(Node {
        name: instance.name().to_string(),
        layout_order: integer(properties, "LayoutOrder", 0),
        position: span(properties, "Position"),
        size: span(properties, "Size"),
        anchor: vector2(properties, "AnchorPoint"),
        rotation: degrees(properties, "Rotation"),
        background: color(properties, "BackgroundColor3", [1.0, 1.0, 1.0]),
        background_alpha: alpha(properties, "BackgroundTransparency"),
        border: integer(properties, "BorderSizePixel", 1).max(0) as f32,
        border_color: color(properties, "BorderColor3", DEFAULT_BORDER),
        clips: flag(properties, "ClipsDescendants", false),
        z_index: integer(properties, "ZIndex", 1),
        fill: fill(properties),
        list: list_layout(dom, database, instance.children()),
        children: elements(dom, database, instance.children()),
    })
}

/// The first `UIListLayout` among `children`, which is the one Roblox honours
/// when several are present.
pub(super) fn list_layout(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    children: &[Ref],
) -> Option<List> {
    let layout = children.iter().find_map(|&child| {
        let instance = dom.get(child)?;
        database
            .is_subclass_of(instance.class(), LIST_LAYOUT_CLASS)
            .then_some(instance)
    })?;
    let properties = layout.properties();
    let padding = match properties.get("Padding") {
        Some(Variant::UDim(value)) => (value.scale, value.offset as f32),
        _ => (0.0, 0.0),
    };
    Some(List {
        vertical: enum_of(properties, "FillDirection", FILL_VERTICAL) == FILL_VERTICAL,
        padding,
        horizontal: align(properties, "HorizontalAlignment"),
        vertical_align: align(properties, "VerticalAlignment"),
        by_name: enum_of(properties, "SortOrder", SORT_LAYOUT_ORDER) == SORT_NAME,
    })
}

/// `Enum.FillDirection.Vertical`; `Horizontal` is 0.
const FILL_VERTICAL: u32 = 1;
/// `Enum.SortOrder.Name` and `Enum.SortOrder.LayoutOrder` (the default).
const SORT_NAME: u32 = 0;
const SORT_LAYOUT_ORDER: u32 = 2;

/// `HorizontalAlignment`/`VerticalAlignment`, both defaulting to Center.
fn align(properties: &BTreeMap<String, Variant>, name: &str) -> Align {
    match enum_of(properties, name, 0) {
        1 => Align::Start,
        2 => Align::End,
        _ => Align::Center,
    }
}

fn enum_of(properties: &BTreeMap<String, Variant>, name: &str, default: u32) -> u32 {
    match properties.get(name) {
        Some(&Variant::Enum(value)) => value,
        _ => default,
    }
}

/// The image of anything carrying one, told apart by the property rather than
/// by class name so `ImageButton` lands here beside `ImageLabel`.
///
/// TODO: `ScaleType.Slice`/`Fit`/`Crop` and `ImageRectOffset`/`ImageRectSize`
/// are all read as a plain stretch. `GuiObject.Rotation` (shared with the
/// element's background and border) is handled in `super::layout`.
fn fill(properties: &BTreeMap<String, Variant>) -> Option<Fill> {
    let asset = AssetRef::parse(asset_uri(properties.get("Image")?)?).ok()?;
    if asset == AssetRef::Empty {
        return None;
    }

    Some(Fill {
        asset,
        tint: color(properties, "ImageColor3", [1.0, 1.0, 1.0]),
        alpha: alpha(properties, "ImageTransparency"),
        tiling: match properties.get("ScaleType") {
            Some(&Variant::Enum(TILE_SCALE_TYPE)) => Tiling::Tile {
                size: span(properties, "TileSize"),
            },
            _ => Tiling::Stretch,
        },
    })
}

/// `Enum.ScaleType.Tile`'s ordinal.
const TILE_SCALE_TYPE: u32 = 2;

pub(super) fn span(properties: &BTreeMap<String, Variant>, name: &str) -> Span {
    match properties.get(name) {
        Some(Variant::UDim2(value)) => Span {
            scale: [value.x.scale, value.y.scale],
            offset: [value.x.offset as f32, value.y.offset as f32],
        },
        _ => Span::default(),
    }
}

pub(super) fn vector2(properties: &BTreeMap<String, Variant>, name: &str) -> [f32; 2] {
    match properties.get(name) {
        Some(Variant::Vector2(value)) => [value.x, value.y],
        _ => [0.0, 0.0],
    }
}

/// A `Color3` linearized, since the display target re-encodes on write — same
/// reasoning as [`crate::scene::srgb_to_linear`]'s own callers.
fn color(properties: &BTreeMap<String, Variant>, name: &str, default: [f32; 3]) -> [f32; 3] {
    let raw = match properties.get(name) {
        Some(Variant::Color3(value)) => [value.r, value.g, value.b],
        Some(&Variant::Color3uint8 { r, g, b }) => {
            [r, g, b].map(|channel| f32::from(channel) / 255.0)
        }
        _ => default,
    };
    raw.map(srgb_to_linear)
}

/// `Rotation`, 0 degrees (unrotated) where the property is missing.
fn degrees(properties: &BTreeMap<String, Variant>, name: &str) -> f32 {
    match properties.get(name) {
        Some(Variant::Float32(value)) => *value,
        Some(Variant::Float64(value)) => *value as f32,
        _ => 0.0,
    }
}

/// `1 - Transparency`, 1 (fully opaque) where the property is missing.
fn alpha(properties: &BTreeMap<String, Variant>, name: &str) -> f32 {
    let transparency = match properties.get(name) {
        Some(Variant::Float32(value)) => *value,
        Some(Variant::Float64(value)) => *value as f32,
        _ => 0.0,
    };
    if transparency.is_finite() {
        1.0 - transparency.clamp(0.0, 1.0)
    } else {
        1.0
    }
}

pub(super) fn flag(properties: &BTreeMap<String, Variant>, name: &str, default: bool) -> bool {
    match properties.get(name) {
        Some(&Variant::Bool(value)) => value,
        _ => default,
    }
}

pub(super) fn integer(properties: &BTreeMap<String, Variant>, name: &str, default: i32) -> i32 {
    match properties.get(name) {
        Some(&Variant::Int32(value)) => value,
        Some(&Variant::Float32(value)) => value as i32,
        _ => default,
    }
}
