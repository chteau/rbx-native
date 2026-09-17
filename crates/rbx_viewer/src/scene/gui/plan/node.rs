//! What a `ScreenGui` tree is read into: resolution-independent nodes, the
//! layout scopes any plain instances among them open, and the screen that
//! holds them.

use rbx_assets::AssetRef;
use rbx_dom::Ref;

use super::viewport;
use super::{Border, Fill, SizeAxes};
use super::{
    Constraints, Corner, FlexItem, Gradient, GroupTint, Layout, Scrolling, Stroke, Text, Viewport,
};
use crate::fonts::Face;
use crate::scene::Part;

/// One `UDim2`: a fraction of the parent box plus a pixel offset, per axis.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(in crate::scene::gui) struct Span {
    pub(in crate::scene::gui) scale: [f32; 2],
    pub(in crate::scene::gui) offset: [f32; 2],
}

impl Span {
    /// The pixel extent this span comes to inside a parent of `size` pixels.
    pub(in crate::scene::gui) fn against(&self, size: [f32; 2]) -> [f32; 2] {
        [
            self.scale[0] * size[0] + self.offset[0],
            self.scale[1] * size[1] + self.offset[1],
        ]
    }
}

/// One `GuiObject` and everything under it, `Visible = false` subtrees already
/// pruned away.
#[derive(Clone)]
pub(in crate::scene::gui) struct Node {
    /// What this node was read from, so a property that names a *sibling*
    /// rather than describing the instance itself — `UIPageLayout.CurrentPage`
    /// — can be matched back to the node it points at.
    pub(in crate::scene::gui) referent: Ref,
    /// Only read for `SortOrder.Name` under a [`List`].
    pub(in crate::scene::gui) name: String,
    pub(in crate::scene::gui) layout_order: i32,
    pub(in crate::scene::gui) position: Span,
    pub(in crate::scene::gui) size: Span,
    pub(in crate::scene::gui) anchor: [f32; 2],
    /// `Rotation`, in degrees around the element's own centre — Roblox gives
    /// no way to move the pivot, so `AnchorPoint` plays no part in this.
    pub(in crate::scene::gui) rotation: f32,
    pub(in crate::scene::gui) background: [f32; 3],
    pub(in crate::scene::gui) background_alpha: f32,
    /// `BorderSizePixel`, in pixels; where the band sits relative to the box
    /// is [`Node::border_mode`]'s business.
    pub(in crate::scene::gui) border: f32,
    pub(in crate::scene::gui) border_color: [f32; 3],
    pub(in crate::scene::gui) border_mode: Border,
    pub(in crate::scene::gui) clips: bool,
    pub(in crate::scene::gui) z_index: i32,
    /// `AutomaticSize`, per axis: the element grows along that axis until it
    /// contains its children, its `Size` acting as a lower bound.
    pub(in crate::scene::gui) automatic_size: [bool; 2],
    /// `SizeConstraint`, which parent axis each `Size` scale is taken against.
    pub(in crate::scene::gui) size_constraint: SizeAxes,
    /// What this element's own `UIComponent` children say about its size.
    pub(in crate::scene::gui) constraints: Constraints,
    pub(in crate::scene::gui) fill: Option<Fill>,
    /// The text of a `TextLabel`/`TextButton`/`TextBox`, drawn over the
    /// background and image.
    pub(in crate::scene::gui) text: Option<Text>,
    /// The layout among this node's children, arranging them.
    pub(in crate::scene::gui) list: Option<Layout>,
    /// A `UIFlexItem` of this node's own, flexing it inside its parent's
    /// `UIListLayout`.
    pub(in crate::scene::gui) flex: Option<FlexItem>,
    pub(in crate::scene::gui) corner: Option<Corner>,
    pub(in crate::scene::gui) stroke: Option<Stroke>,
    pub(in crate::scene::gui) gradient: Option<Gradient>,
    /// A `ViewportFrame`'s 3D content, rendered into a texture the box then
    /// shows like an image.
    pub(in crate::scene::gui) viewport: Option<Viewport>,
    /// What makes a `ScrollingFrame` more than a `Frame`.
    pub(in crate::scene::gui) scrolling: Option<Scrolling>,
    /// A `CanvasGroup`'s tint over its flattened subtree.
    pub(in crate::scene::gui) group: Option<GroupTint>,
    pub(in crate::scene::gui) children: Vec<Node>,
    /// The non-`GuiObject` containers among this node's children, and what
    /// they hold — see [`Group`].
    pub(in crate::scene::gui) groups: Vec<Group>,
}

impl Node {
    /// Whether anything in this subtree puts a pixel down: a container holding
    /// only transparent text has no reason to be given a canvas.
    pub(in crate::scene::gui) fn paints(&self) -> bool {
        self.background_alpha > 0.0
            || self.fill.as_ref().is_some_and(|fill| fill.alpha > 0.0)
            || self.viewport.as_ref().is_some_and(|viewport| {
                viewport.alpha > 0.0 && viewport.camera.is_some() && !viewport.parts.is_empty()
            })
            || self.stroke.is_some_and(|stroke| stroke.alpha > 0.0)
            || self.text.as_ref().is_some_and(Text::visible)
            // A fixed `CanvasSize` can overflow an empty frame, and the bar
            // that shows for it is paint of its own.
            || self
                .scrolling
                .as_ref()
                .is_some_and(|scrolling| scrolling.thickness > 0.0 && scrolling.bar_alpha > 0.0)
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
pub(in crate::scene::gui) struct Group {
    /// Where the instance sat among the container's children, as an index
    /// into its `children`: the contents paint in its place, so tree order
    /// survives a container that is not drawn.
    pub(in crate::scene::gui) at: usize,
    /// The `UILayout` hung directly off this instance, arranging its own
    /// contents against the container's rect.
    pub(in crate::scene::gui) layout: Option<Layout>,
    pub(in crate::scene::gui) children: Vec<Node>,
    /// Non-`GuiObject` containers nested inside this one — a `Folder` in a
    /// `Folder` is a layout scope inside a layout scope.
    pub(in crate::scene::gui) groups: Vec<Group>,
}

impl Group {
    pub(in crate::scene::gui) fn paints(&self) -> bool {
        self.children.iter().any(Node::paints) || self.groups.iter().any(Group::paints)
    }

    /// Whether this group holds nothing drawable at all, in which case the
    /// plan is better off without it: every `LocalScript` and value object in
    /// a GUI tree would otherwise become an empty layout scope.
    pub(in crate::scene::gui) fn is_empty(&self) -> bool {
        self.children.is_empty() && self.groups.is_empty()
    }
}

/// One `ScreenGui`: a screen-space overlay whose top-level children resolve
/// against the viewport itself.
#[derive(Clone)]
pub(crate) struct Screen {
    pub(in crate::scene::gui) display_order: i32,
    /// `ScreenInsets`: pixels of the viewport's top edge the canvas gives up
    /// to Roblox's top bar.
    pub(in crate::scene::gui) top_inset: f32,
    /// `ZIndexBehavior.Global`, where `ZIndex` orders every descendant of the
    /// screen against every other rather than only its own siblings.
    pub(in crate::scene::gui) global_z_index: bool,
    /// `ClipToDeviceSafeArea`: whether the canvas also scissors its contents
    /// rather than only offsetting them.
    pub(in crate::scene::gui) clip_to_safe_area: bool,
    pub(in crate::scene::gui) list: Option<Layout>,
    pub(in crate::scene::gui) roots: Vec<Node>,
    pub(in crate::scene::gui) groups: Vec<Group>,
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

pub(in crate::scene::gui) fn collect_fonts_of(
    nodes: &[Node],
    groups: &[Group],
    into: &mut Vec<Face>,
) {
    for node in nodes {
        collect_fonts(node, into);
    }
    for group in groups {
        collect_fonts_of(&group.children, &group.groups, into);
    }
}

pub(in crate::scene::gui) fn collect_fonts(node: &Node, into: &mut Vec<Face>) {
    if let Some(text) = &node.text {
        text.faces(into);
    }
    collect_fonts_of(&node.children, &node.groups, into);
}

pub(in crate::scene::gui) fn collect_assets_of(
    nodes: &[Node],
    groups: &[Group],
    into: &mut Vec<AssetRef>,
) {
    for node in nodes {
        collect_assets(node, into);
    }
    for group in groups {
        collect_assets_of(&group.children, &group.groups, into);
    }
}

pub(in crate::scene::gui) fn collect_assets(node: &Node, into: &mut Vec<AssetRef>) {
    if let Some(fill) = &node.fill {
        if !into.contains(&fill.asset) {
            into.push(fill.asset.clone());
        }
    }
    if let Some(scrolling) = &node.scrolling {
        let images = &scrolling.images;
        for asset in [&images.top, &images.mid, &images.bottom] {
            if !into.contains(asset) {
                into.push(asset.clone());
            }
        }
    }
    collect_assets_of(&node.children, &node.groups, into);
}
