//! Reads the `ScreenGui` trees a DOM carries into resolution-independent
//! nodes. Nothing here knows the viewport: a `UDim2` stays a scale plus an
//! offset until [`super::layout`] is handed a pixel rect to resolve it
//! against.

use std::collections::BTreeMap;

use rbx_assets::AssetRef;
use rbx_dom::{Instance, Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::style::Styled;
use crate::textures::asset_uri;

mod constraints;
mod corner;
mod gradient;
mod layouts;
mod props;
mod stroke;

use constraints::{automatic_size, border_mode, constraints, size_axes};
pub(super) use constraints::{global_z_index, Aspect, Border, Constraints, SizeAxes};
pub(super) use corner::Corner;
pub(super) use gradient::Gradient;
pub(crate) use gradient::{GradientKind, Tile};
pub(super) use layouts::{layout_of, Align, Flex, FlexItem, Grid, Layout, LineAlign, List, Table};
use props::{alpha, color, degrees, enum_of};
pub(super) use props::{flag, integer, span, vector2};
pub(crate) use stroke::Join;
pub(super) use stroke::{Stroke, StrokePosition};

const SCREEN_CLASS: &str = "ScreenGui";
const ELEMENT_CLASS: &str = "GuiObject";
/// The classes whose `UIStroke` outlines glyphs rather than the box.
const TEXT_CLASSES: [&str; 3] = ["TextLabel", "TextButton", "TextBox"];

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
    /// `BorderSizePixel`, in pixels; where the band sits relative to the box
    /// is [`Node::border_mode`]'s business.
    pub(super) border: f32,
    pub(super) border_color: [f32; 3],
    pub(super) border_mode: Border,
    pub(super) clips: bool,
    pub(super) z_index: i32,
    /// `AutomaticSize`, per axis: the element grows along that axis until it
    /// contains its children, its `Size` acting as a lower bound.
    pub(super) automatic_size: [bool; 2],
    /// `SizeConstraint`, which parent axis each `Size` scale is taken against.
    pub(super) size_constraint: SizeAxes,
    /// What this element's own `UIComponent` children say about its size.
    pub(super) constraints: Constraints,
    /// Content this element holds that is not a child node — a text element's
    /// own measured bounds. Counted alongside the children's extent when
    /// [`Node::automatic_size`] grows the box.
    pub(super) content_size: Option<[f32; 2]>,
    pub(super) fill: Option<Fill>,
    /// The layout among this node's children, arranging them.
    pub(super) list: Option<Layout>,
    /// A `UIFlexItem` of this node's own, flexing it inside its parent's
    /// `UIListLayout`.
    pub(super) flex: Option<FlexItem>,
    pub(super) corner: Option<Corner>,
    pub(super) stroke: Option<Stroke>,
    pub(super) gradient: Option<Gradient>,
    pub(super) children: Vec<Node>,
}

impl Node {
    /// Whether anything in this subtree puts a pixel down: a container holding
    /// only transparent text has no reason to be given a canvas.
    pub(super) fn paints(&self) -> bool {
        self.background_alpha > 0.0
            || self.fill.as_ref().is_some_and(|fill| fill.alpha > 0.0)
            || self.stroke.is_some_and(|stroke| stroke.alpha > 0.0)
            || self.children.iter().any(Node::paints)
    }
}

/// One `ScreenGui`: a screen-space overlay whose top-level children resolve
/// against the viewport itself.
#[derive(Clone)]
pub(crate) struct Screen {
    pub(super) display_order: i32,
    /// `ScreenInsets`: pixels of the viewport's top edge the canvas gives up
    /// to Roblox's top bar.
    pub(super) top_inset: f32,
    /// `ZIndexBehavior.Global`, where `ZIndex` orders every descendant of the
    /// screen against every other rather than only its own siblings.
    pub(super) global_z_index: bool,
    pub(super) list: Option<Layout>,
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
    let styles = Styled::new(dom);
    let mut screens = Vec::new();
    for &root in dom.root_refs() {
        gather(dom, database, &styles, root, &mut screens);
    }
    screens
}

fn gather(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    styles: &Styled,
    referent: Ref,
    into: &mut Vec<Screen>,
) {
    let Some(instance) = dom.get(referent) else {
        return;
    };
    if database.is_subclass_of(instance.class(), SCREEN_CLASS) {
        let properties = styles.properties_of(instance);
        if flag(properties, "Enabled", true) {
            into.push(Screen {
                display_order: integer(properties, "DisplayOrder", 0),
                top_inset: constraints::top_bar_inset(properties),
                global_z_index: constraints::global_z_index(properties),
                list: layout_of(dom, database, styles, instance.children()),
                roots: elements(dom, database, styles, instance.children()),
            });
        }
        // A `ScreenGui` never nests inside another, and its own children are
        // already read above.
        return;
    }
    for &child in instance.children() {
        gather(dom, database, styles, child, into);
    }
}

pub(super) fn elements(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    styles: &Styled,
    children: &[Ref],
) -> Vec<Node> {
    children
        .iter()
        .filter_map(|&child| element(dom, database, styles, child))
        .collect()
}

/// One `GuiObject` read into a [`Node`], or `None` where it is not drawable.
///
/// A class this viewer has no special handling for still becomes a node: an
/// unknown `GuiObject` subclass draws its background like a `Frame` rather
/// than vanishing.
fn element(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    styles: &Styled,
    referent: Ref,
) -> Option<Node> {
    let instance = dom.get(referent)?;
    let class = instance.class();
    if !database.is_subclass_of(class, ELEMENT_CLASS) {
        return None;
    }

    let properties = styles.properties_of(instance);
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
        border_mode: border_mode(properties),
        clips: flag(properties, "ClipsDescendants", false),
        z_index: integer(properties, "ZIndex", 1),
        automatic_size: automatic_size(properties),
        size_constraint: size_axes(properties),
        constraints: constraints(dom, database, instance.children()),
        content_size: None,
        fill: fill(properties),
        list: layout_of(dom, database, styles, instance.children()),
        flex: layouts::flex_item(dom, database, styles, instance.children()),
        corner: corner::read(dom, database, styles, instance.children()),
        stroke: stroke::read(
            dom,
            database,
            styles,
            instance.children(),
            is_text(database, class),
        ),
        gradient: gradient::read(dom, database, styles, instance.children()),
        children: elements(dom, database, styles, instance.children()),
    })
}

fn is_text(database: &ReflectionDatabase, class: &str) -> bool {
    TEXT_CLASSES
        .iter()
        .any(|text| database.is_subclass_of(class, text))
}

/// The children of `class` (or a subclass) among `children`, in tree order —
/// the pool every "first `UICorner`/`UIStroke`/`UIGradient`" is drawn from.
fn modifiers<'a>(
    dom: &'a WeakDom,
    database: &'a ReflectionDatabase,
    children: &'a [Ref],
    class: &'a str,
) -> impl Iterator<Item = &'a Instance> + 'a {
    children.iter().filter_map(move |&child| {
        let instance = dom.get(child)?;
        database
            .is_subclass_of(instance.class(), class)
            .then_some(instance)
    })
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
