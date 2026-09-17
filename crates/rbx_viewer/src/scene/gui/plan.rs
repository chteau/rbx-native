//! Reads the `ScreenGui` trees a DOM carries into resolution-independent
//! nodes. Nothing here knows the viewport: a `UDim2` stays a scale plus an
//! offset until [`super::layout`] is handed a pixel rect to resolve it
//! against.

use rbx_dom::{Instance, Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::style::Styled;

mod constraints;
mod corner;
mod gradient;
mod group;
mod image;
mod layouts;
mod node;
mod props;
mod scrolling;
mod stroke;
mod text;
mod viewport;

use constraints::{automatic_size, border_mode, constraints, size_axes};
pub(super) use constraints::{global_z_index, Aspect, Border, Constraints, SizeAxes};
pub(super) use corner::Corner;
pub(super) use gradient::Gradient;
pub(crate) use gradient::{GradientKind, Tile};
pub(crate) use group::GroupTint;
use image::fill;
pub(super) use image::{Fill, ScaleMode};
pub(crate) use node::Screen;
pub(in crate::scene::gui) use node::{collect_assets_of, collect_fonts_of, Group, Node, Span};
// Reaches all the way to `renderer::gui::quads::image`, unlike `Fill`/
// `ScaleMode` above — see the type's own doc comment.
pub(crate) use image::PixelRect;
pub(crate) use layouts::Align;
pub(super) use layouts::{layout_of, Flex, FlexItem, Grid, Layout, LineAlign, List, Page, Table};
use props::{alpha, color, degrees, enum_of};
pub(super) use props::{flag, float, integer, span, vector2};
#[cfg(test)]
pub(super) use scrolling::Inset;
pub(super) use scrolling::Scrolling;
pub(crate) use stroke::Join;
pub(super) use stroke::{Stroke, StrokePosition};
#[cfg(test)]
pub(crate) use text::Span as TextSpan;
pub(crate) use text::{span_face, Text};
pub(super) use viewport::each_part as each_viewport_part;
pub(crate) use viewport::{ViewCamera, Viewport};

use crate::scene::Catalog;

const SCREEN_CLASS: &str = "ScreenGui";
const ELEMENT_CLASS: &str = "GuiObject";
/// The three text classes share every text property, so one reader serves
/// all of them — and their `UIStroke` outlines glyphs rather than the box;
/// only the `TextBox` placeholder rule tells them apart.
const TEXT_CLASSES: [&str; 3] = ["TextLabel", "TextButton", "TextBox"];
const TEXT_BOX_CLASS: &str = "TextBox";
const SCROLLING_CLASS: &str = "ScrollingFrame";
/// "`CanvasGroup` always has `ClipsDescendants` set to `true`" (docs), so
/// the property is not even read for one.
const GROUP_CLASS: &str = "CanvasGroup";

/// Roblox's own default `BorderColor3`, `Color3.fromRGB(27, 42, 53)`. Only
/// ever seen on a tree built in code: a place file serializes the property.
const DEFAULT_BORDER: [f32; 3] = [27.0 / 255.0, 42.0 / 255.0, 53.0 / 255.0];

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
    let group = database
        .is_subclass_of(class, GROUP_CLASS)
        .then(|| group::group(properties));

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
        clips: group.is_some() || flag(properties, "ClipsDescendants", false),
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
        strokes: stroke::read(
            dom,
            database,
            styles,
            instance.children(),
            is_text(database, class),
        ),
        gradient: gradient::read(dom, database, styles, instance.children()),
        viewport: viewport::read(dom, database, instance, properties, materials),
        scrolling: database
            .is_subclass_of(class, SCROLLING_CLASS)
            .then(|| scrolling::scrolling(properties)),
        group,
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
