//! Reads the `ScreenGui` trees a DOM carries into resolution-independent
//! nodes. Nothing here knows the viewport: a `UDim2` stays a scale plus an
//! offset until [`super::layout`] is handed a pixel rect to resolve it
//! against.

use rbx_assets::AssetRef;
use rbx_dom::{Instance, Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::style::Styled;

mod constraints;
mod corner;
mod gradient;
mod image;
mod layouts;
mod props;
mod stroke;
mod text;
mod viewport;

use constraints::{automatic_size, border_mode, constraints, size_axes};
pub(super) use constraints::{global_z_index, Aspect, Border, Constraints, SizeAxes};
pub(super) use corner::Corner;
pub(super) use gradient::Gradient;
pub(crate) use gradient::{GradientKind, Tile};
use image::fill;
pub(super) use image::{Fill, ScaleMode};
// Reaches all the way to `renderer::gui::quads::image`, unlike `Fill`/
// `ScaleMode` above — see the type's own doc comment.
pub(crate) use image::PixelRect;
pub(crate) use layouts::Align;
pub(super) use layouts::{layout_of, Flex, FlexItem, Grid, Layout, LineAlign, List, Page, Table};
use props::{alpha, color, degrees, enum_of};
pub(super) use props::{flag, float, integer, span, vector2};
pub(crate) use stroke::Join;
pub(super) use stroke::{Stroke, StrokePosition};
#[cfg(test)]
pub(crate) use text::Span as TextSpan;
pub(crate) use text::{span_face, Text};
pub(super) use viewport::each_part as each_viewport_part;
pub(crate) use viewport::{ViewCamera, Viewport};

use crate::fonts::Face;
use crate::scene::{Catalog, Part};

const SCREEN_CLASS: &str = "ScreenGui";
const ELEMENT_CLASS: &str = "GuiObject";
/// The three text classes share every text property, so one reader serves
/// all of them — and their `UIStroke` outlines glyphs rather than the box;
/// only the `TextBox` placeholder rule tells them apart.
const TEXT_CLASSES: [&str; 3] = ["TextLabel", "TextButton", "TextBox"];
const TEXT_BOX_CLASS: &str = "TextBox";

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

/// One `GuiObject` and everything under it, `Visible = false` subtrees already
/// pruned away.
#[derive(Clone)]
pub(super) struct Node {
    /// What this node was read from, so a property that names a *sibling*
    /// rather than describing the instance itself — `UIPageLayout.CurrentPage`
    /// — can be matched back to the node it points at.
    pub(super) referent: Ref,
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
    pub(super) fill: Option<Fill>,
    /// The text of a `TextLabel`/`TextButton`/`TextBox`, drawn over the
    /// background and image.
    pub(super) text: Option<Text>,
    /// The layout among this node's children, arranging them.
    pub(super) list: Option<Layout>,
    /// A `UIFlexItem` of this node's own, flexing it inside its parent's
    /// `UIListLayout`.
    pub(super) flex: Option<FlexItem>,
    pub(super) corner: Option<Corner>,
    pub(super) stroke: Option<Stroke>,
    pub(super) gradient: Option<Gradient>,
    /// A `ViewportFrame`'s 3D content, rendered into a texture the box then
    /// shows like an image.
    pub(super) viewport: Option<Viewport>,
    pub(super) children: Vec<Node>,
    /// The non-`GuiObject` containers among this node's children, and what
    /// they hold — see [`Group`].
    pub(super) groups: Vec<Group>,
}

impl Node {
    /// Whether anything in this subtree puts a pixel down: a container holding
    /// only transparent text has no reason to be given a canvas.
    pub(super) fn paints(&self) -> bool {
        self.background_alpha > 0.0
            || self.fill.as_ref().is_some_and(|fill| fill.alpha > 0.0)
            || self.viewport.as_ref().is_some_and(|viewport| {
                viewport.alpha > 0.0 && viewport.camera.is_some() && !viewport.parts.is_empty()
            })
            || self.stroke.is_some_and(|stroke| stroke.alpha > 0.0)
            || self.text.as_ref().is_some_and(Text::visible)
            || self.children.iter().any(Node::paints)
            || self.groups.iter().any(Group::paints)
    }
}

/// What one non-`GuiObject` instance inside a GUI tree — a `Folder`, or a
/// `Configuration`, a `ModuleScript`, anything — contributes to the picture.
///
/// Roblox draws a `GuiObject` whose ancestry reaches a `ScreenGui`/
/// `BillboardGui`/`SurfaceGui` however many plain instances sit in between,
/// so such an instance is walked *through* rather than being an end to the
/// tree. It has no box of its own, so its contents resolve against the
/// container above it — the nearest `GuiBase2d` — and they paint where it
/// sits among its siblings.
///
/// It is not simply transparent, though. Roblox's `Folder` page: "Each
/// `Folder` in your UI hierarchy can define its own `UILayout`
/// (`UIListLayout`, `UIGridLayout`, `UIPageLayout`, `UITableLayout`), or use
/// a default position-based layout. [...] `Folder` contents are exempt from
/// the effects of a `UILayout` sibling." So the contents are a layout scope
/// of their own: arranged by [`Group::layout`] where there is one and by
/// their own `Position`/`Size` where there is not, and never an item of the
/// container's layout.
#[derive(Clone)]
pub(super) struct Group {
    /// Where the instance sat among the container's children, as an index
    /// into its `children`: the contents paint in its place, so tree order
    /// survives a container that is not drawn.
    pub(super) at: usize,
    /// The `UILayout` hung directly off this instance, arranging its own
    /// contents against the container's rect.
    pub(super) layout: Option<Layout>,
    pub(super) children: Vec<Node>,
    /// Non-`GuiObject` containers nested inside this one — a `Folder` in a
    /// `Folder` is a layout scope inside a layout scope.
    pub(super) groups: Vec<Group>,
}

impl Group {
    pub(super) fn paints(&self) -> bool {
        self.children.iter().any(Node::paints) || self.groups.iter().any(Group::paints)
    }

    /// Whether this group holds nothing drawable at all, in which case the
    /// plan is better off without it: every `LocalScript` and value object in
    /// a GUI tree would otherwise become an empty layout scope.
    fn is_empty(&self) -> bool {
        self.children.is_empty() && self.groups.is_empty()
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
    /// `ClipToDeviceSafeArea`: whether the canvas also scissors its contents
    /// rather than only offsetting them.
    pub(super) clip_to_safe_area: bool,
    pub(super) list: Option<Layout>,
    pub(super) roots: Vec<Node>,
    pub(super) groups: Vec<Group>,
}

impl Screen {
    /// Every image the screen wants, in first-seen paint order.
    pub(crate) fn assets(&self, into: &mut Vec<AssetRef>) {
        collect_assets_of(&self.roots, &self.groups, into);
    }

    /// Every font face the screen's text wants, in first-seen paint order.
    pub(crate) fn fonts(&self, into: &mut Vec<Face>) {
        collect_fonts_of(&self.roots, &self.groups, into);
    }

    /// Every `ViewportFrame` part on the screen — see `viewport::each_part`.
    pub(crate) fn viewport_parts(&mut self, apply: &mut impl FnMut(&mut Part)) {
        for root in &mut self.roots {
            viewport::each_part(root, apply);
        }
    }
}

pub(super) fn collect_fonts_of(nodes: &[Node], groups: &[Group], into: &mut Vec<Face>) {
    for node in nodes {
        collect_fonts(node, into);
    }
    for group in groups {
        collect_fonts_of(&group.children, &group.groups, into);
    }
}

fn collect_fonts(node: &Node, into: &mut Vec<Face>) {
    if let Some(text) = &node.text {
        text.faces(into);
    }
    collect_fonts_of(&node.children, &node.groups, into);
}

pub(super) fn collect_assets_of(nodes: &[Node], groups: &[Group], into: &mut Vec<AssetRef>) {
    for node in nodes {
        collect_assets(node, into);
    }
    for group in groups {
        collect_assets_of(&group.children, &group.groups, into);
    }
}

fn collect_assets(node: &Node, into: &mut Vec<AssetRef>) {
    if let Some(fill) = &node.fill {
        if !into.contains(&fill.asset) {
            into.push(fill.asset.clone());
        }
    }
    collect_assets_of(&node.children, &node.groups, into);
}

/// Every enabled `ScreenGui` in the DOM, in the order the tree holds them.
///
/// The walk is its own recursion rather than [`crate::scene::descendants`]:
/// that iterator pops off a stack and so visits children backwards, while a
/// GUI's paint order among equal `ZIndex` siblings is exactly tree order.
///
/// `materials` is the scene's catalog, which a `ViewportFrame`'s parts take
/// their layers from (see [`viewport`]).
pub(crate) fn plan(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    materials: &mut Catalog,
) -> Vec<Screen> {
    let styles = Styled::new(dom);
    let mut screens = Vec::new();
    for &root in dom.root_refs() {
        gather(dom, database, &styles, materials, root, &mut screens);
    }
    screens
}

fn gather(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    styles: &Styled,
    materials: &mut Catalog,
    referent: Ref,
    into: &mut Vec<Screen>,
) {
    let Some(instance) = dom.get(referent) else {
        return;
    };
    if database.is_subclass_of(instance.class(), SCREEN_CLASS) {
        let properties = styles.properties_of(instance);
        if flag(properties, "Enabled", true) {
            let (roots, groups) = elements(dom, database, styles, materials, instance.children());
            into.push(Screen {
                display_order: integer(properties, "DisplayOrder", 0),
                top_inset: constraints::top_bar_inset(properties),
                global_z_index: constraints::global_z_index(properties),
                clip_to_safe_area: constraints::clip_to_safe_area(properties),
                list: layout_of(dom, database, styles, instance.children()),
                roots,
                groups,
            });
        }
        // A `ScreenGui` never nests inside another, and its own children are
        // already read above.
        return;
    }
    for &child in instance.children() {
        gather(dom, database, styles, materials, child, into);
    }
}

/// What one container holds: its own `GuiObject` children in tree order, and
/// a [`Group`] for every non-`GuiObject` child that holds elements of its own.
pub(super) fn elements(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    styles: &Styled,
    materials: &mut Catalog,
    children: &[Ref],
) -> (Vec<Node>, Vec<Group>) {
    let mut nodes = Vec::new();
    let mut groups = Vec::new();
    for &child in children {
        let Some(instance) = dom.get(child) else {
            continue;
        };
        // The class is asked here rather than left to `element`, which also
        // answers `None` for a `Visible = false` element — a subtree Roblox
        // hides whole, and that must not be walked around.
        if database.is_subclass_of(instance.class(), ELEMENT_CLASS) {
            if let Some(node) = element(dom, database, styles, materials, child) {
                nodes.push(node);
            }
            continue;
        }
        let (children, groups_of) = elements(dom, database, styles, materials, instance.children());
        let group = Group {
            at: nodes.len(),
            layout: layout_of(dom, database, styles, instance.children()),
            children,
            groups: groups_of,
        };
        if !group.is_empty() {
            groups.push(group);
        }
    }
    (nodes, groups)
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
    materials: &mut Catalog,
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

    let constraints = constraints(dom, database, styles, instance.children());
    let (children, groups) = elements(dom, database, styles, materials, instance.children());

    Some(Node {
        referent,
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
        constraints,
        fill: fill(properties),
        text: is_text(database, class).then(|| {
            text::text(
                properties,
                database.is_subclass_of(class, TEXT_BOX_CLASS),
                constraints.text_size_bounds,
            )
        }),
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
        viewport: viewport::read(dom, database, instance, properties, materials),
        children,
        groups,
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
