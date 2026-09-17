//! Resolves a planned `ScreenGui` tree into flat, screen-space rectangles in
//! the order they have to be painted.
//!
//! Coordinates are pixels with the origin at the viewport's top-left corner,
//! which is the frame `UDim2` itself is written in; the renderer is what turns
//! them into clip space.

use rbx_assets::AssetRef;

use super::plan::{Align, Fill, List, Node, Screen, Span, Tiling};
use super::space::SpaceGui;

mod sizing;

/// A screen-space box in pixels, top-left origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Rect {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
}

impl Rect {
    pub(crate) fn size(&self) -> [f32; 2] {
        [self.width, self.height]
    }

    /// The overlap of two boxes, empty (zero-sized) where they do not meet —
    /// which is what a scissor rect has to become for a child clipped away
    /// entirely.
    pub(crate) fn intersect(&self, other: &Rect) -> Rect {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = (self.x + self.width).min(other.x + other.width);
        let bottom = (self.y + self.height).min(other.y + other.height);

        Rect {
            x,
            y,
            width: (right - x).max(0.0),
            height: (bottom - y).max(0.0),
        }
    }
}

/// An `ImageLabel`'s image with its tiling already turned into a UV repeat
/// count, so the renderer never has to resolve a `UDim2` of its own.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Painted {
    pub(crate) asset: AssetRef,
    pub(crate) tint: [f32; 3],
    pub(crate) alpha: f32,
    /// How many times the image repeats across the box, 1 being a stretch.
    pub(crate) repeat: [f32; 2],
}

/// One `GuiObject` at its final pixel position, ready to be drawn on its own.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Element {
    pub(crate) rect: Rect,
    /// The scissor rect inherited from the nearest `ClipsDescendants`
    /// ancestor, if any. Already intersected down the whole chain.
    pub(crate) clip: Option<Rect>,
    /// `AbsoluteRotation`: degrees clockwise around `rect`'s own centre, this
    /// element's `Rotation` plus every ancestor's. `rect` has already been
    /// carried around those ancestors' centres, so turning it about its own
    /// is all that is left to do.
    pub(crate) rotation: f32,
    pub(crate) background: [f32; 3],
    pub(crate) background_alpha: f32,
    /// `BorderSizePixel` and `BorderColor3`, `None` for a zero-width border.
    pub(crate) border: Option<(f32, [f32; 3])>,
    /// How far inside `rect` the border's outer edge sits, per `BorderMode`.
    pub(crate) border_inset: f32,
    /// `ZIndex`, kept so a `ZIndexBehavior.Global` screen can sort its whole
    /// flattened tree by it after the fact.
    pub(crate) z_index: i32,
    pub(crate) image: Option<Painted>,
}

/// Every element of every screen, in paint order: `DisplayOrder` first, then
/// `ZIndex` among siblings, then tree order — and a child always over its
/// parent, which is what `ZIndexBehavior.Sibling` (the default) means.
pub(crate) fn resolve(screens: &[Screen], viewport: [f32; 2]) -> Vec<Element> {
    let mut order: Vec<&Screen> = screens.iter().collect();
    // Stable, so two screens sharing a `DisplayOrder` keep the order the DOM
    // holds them in rather than an arbitrary one.
    order.sort_by_key(|screen| screen.display_order);

    let mut elements = Vec::new();
    for screen in order {
        // `ScreenInsets`: the canvas starts below the top bar, and is that
        // much shorter, so a `{1, 0}` child still reaches the bottom edge.
        let frame = Rect {
            y: screen.top_inset,
            height: (viewport[1] - screen.top_inset).max(0.0),
            ..canvas(viewport)
        };
        let start = elements.len();
        children(
            &screen.roots,
            screen.list.as_ref(),
            &frame,
            Context {
                global_z_index: screen.global_z_index,
                ..Context::default()
            },
            &mut elements,
        );
        if screen.global_z_index {
            // "Sorts all descendants according to the ZIndex, then breaks ties
            // using the hierarchy order": the walk above already emitted them
            // in hierarchy order, so a stable sort is the whole of it.
            elements[start..].sort_by_key(|element| element.z_index);
        }
    }
    elements
}

/// The same resolution for a `BillboardGui`/`SurfaceGui`, against its own
/// canvas instead of the viewport: a container drawn into an offscreen texture
/// is a viewport of `canvas` pixels as far as a `UDim2` is concerned, which is
/// the whole reason this and [`resolve`] are one code path.
pub(crate) fn resolve_canvas(gui: &SpaceGui) -> Vec<Element> {
    let frame = canvas(gui.canvas);
    let mut elements = Vec::new();
    children(
        &gui.roots,
        gui.list.as_ref(),
        &frame,
        Context {
            global_z_index: gui.global_z_index,
            ..Context::default()
        },
        &mut elements,
    );
    if gui.global_z_index {
        elements.sort_by_key(|element| element.z_index);
    }
    elements
}

/// The box top-level children resolve against: the whole target, whatever it is.
fn canvas(size: [f32; 2]) -> Rect {
    Rect {
        x: 0.0,
        y: 0.0,
        width: size[0],
        height: size[1],
    }
}

/// What an element hands down to the subtree under it.
#[derive(Debug, Clone, Copy, Default)]
struct Context {
    /// The scissor rect inherited from the nearest `ClipsDescendants`
    /// ancestor, already intersected down the whole chain.
    clip: Option<Rect>,
    /// Whether this element or any ancestor carries a non-zero `Rotation`,
    /// which is what turns `ClipsDescendants` off.
    rotated: bool,
    /// The ancestors' cumulative `AbsoluteRotation`, and the screen point it
    /// turns about — the nearest rotated ancestor's own centre.
    angle: f32,
    pivot: [f32; 2],
    /// `ZIndexBehavior.Global`, where siblings are emitted in tree order and
    /// [`resolve`] sorts the whole screen by `ZIndex` afterwards.
    global_z_index: bool,
}

impl Context {
    /// Where a child's axis-aligned box actually lands: an ancestor's rotation
    /// carries the whole box around that ancestor's centre, and only then does
    /// the child turn about its own.
    fn carried(&self, rect: Rect) -> Rect {
        if self.angle == 0.0 {
            return rect;
        }
        // Same clockwise convention as the renderer's own rotation, y running
        // down the screen.
        let (sin, cos) = self.angle.to_radians().sin_cos();
        let dx = rect.x + rect.width * 0.5 - self.pivot[0];
        let dy = rect.y + rect.height * 0.5 - self.pivot[1];
        Rect {
            x: self.pivot[0] + dx * cos - dy * sin - rect.width * 0.5,
            y: self.pivot[1] + dx * sin + dy * cos - rect.height * 0.5,
            ..rect
        }
    }
}

/// Places every sibling inside `parent`, then emits them in paint order.
///
/// The two orders are distinct: a `UIListLayout` decides where a sibling
/// sits, `ZIndex` decides which one is drawn over the other.
fn children(
    nodes: &[Node],
    list: Option<&List>,
    parent: &Rect,
    context: Context,
    into: &mut Vec<Element>,
) {
    let rects = arrange(nodes, list, parent);
    for index in sorted(nodes, context.global_z_index) {
        emit(&nodes[index], context.carried(rects[index]), context, into);
    }
}

/// One rect per node in `nodes`'s own order, sized and placed inside `parent`
/// but not yet carried around any rotated ancestor.
fn arrange(nodes: &[Node], list: Option<&List>, parent: &Rect) -> Vec<Rect> {
    match list {
        Some(list) => stacked(nodes, list, parent),
        None => nodes
            .iter()
            .map(|node| {
                place(
                    node.position,
                    sizing::extent(node, parent.size()),
                    node.anchor,
                    parent,
                )
            })
            .collect(),
    }
}

/// Sibling indices in paint order. Stable for the same reason screens are.
///
/// Under `ZIndexBehavior.Global` the siblings are left in tree order: that is
/// the hierarchy order the screen-wide sort breaks ties with, and reordering
/// them here would interleave their subtrees wrongly.
fn sorted(nodes: &[Node], global_z_index: bool) -> Vec<usize> {
    let mut order: Vec<usize> = (0..nodes.len()).collect();
    if !global_z_index {
        order.sort_by_key(|&index| nodes[index].z_index);
    }
    order
}

fn emit(node: &Node, rect: Rect, context: Context, into: &mut Vec<Element>) {
    into.push(Element {
        rect,
        clip: context.clip,
        rotation: context.angle + node.rotation,
        background: node.background,
        background_alpha: node.background_alpha,
        border: (node.border > 0.0).then_some((node.border, node.border_color)),
        border_inset: node.border_mode.inset(node.border),
        z_index: node.z_index,
        image: node.fill.as_ref().map(|fill| painted(fill, &rect)),
    });

    // Roblox's own docs describe two modes here, gated on the (NotScriptable,
    // RolloutState) `StarterGui.ClipsDescendantsSupportsRotation`: enabled,
    // clipping works correctly against rotated shapes; not enabled — the
    // mode this models, since there is no scriptable way to read the flag at
    // all — a non-zero `Rotation` on this element or any ancestor makes
    // `ClipsDescendants` a no-op rather than clipping to a box that no longer
    // matches what is actually drawn on screen.
    let rotated = context.rotated || node.rotation != 0.0;
    let angle = context.angle + node.rotation;
    children(
        &node.children,
        node.list.as_ref(),
        &sizing::padded(node, &rect),
        Context {
            clip: match node.clips && !rotated {
                true => Some(context.clip.map_or(rect, |outer| outer.intersect(&rect))),
                false => context.clip,
            },
            rotated,
            angle,
            // Once this element turns, its children turn about *its* centre.
            pivot: match angle == 0.0 {
                true => context.pivot,
                false => [rect.x + rect.width * 0.5, rect.y + rect.height * 0.5],
            },
            ..context
        },
        into,
    );
}

/// `UIListLayout` placement, one rect per node in `nodes`'s own order: the
/// siblings keep their `Size`, are sorted, and are laid end to end along the
/// fill axis with `Padding` between them. `Position` and `AnchorPoint` are
/// ignored, as Roblox ignores them.
fn stacked(nodes: &[Node], list: &List, parent: &Rect) -> Vec<Rect> {
    let extent = parent.size();
    let sizes: Vec<[f32; 2]> = nodes
        .iter()
        .map(|node| sizing::extent(node, extent))
        .collect();
    let along = usize::from(list.vertical);
    let across = 1 - along;

    let mut order: Vec<usize> = (0..nodes.len()).collect();
    match list.by_name {
        true => order.sort_by(|&a, &b| nodes[a].name.cmp(&nodes[b].name)),
        // Stable: equal `LayoutOrder`s keep tree order, which is what "added
        // sooner to the parent" comes to in a saved place.
        false => order.sort_by_key(|&index| nodes[index].layout_order),
    }

    let padding = list.padding.0 * extent[along] + list.padding.1;
    let total = sizes.iter().map(|size| size[along]).sum::<f32>()
        + padding * nodes.len().saturating_sub(1) as f32;
    let (stack, item) = match list.vertical {
        true => (list.vertical_align, list.horizontal),
        false => (list.horizontal, list.vertical_align),
    };

    let mut rects = vec![
        Rect {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0
        };
        nodes.len()
    ];
    let mut cursor = offset(stack, extent[along], total);
    for index in order {
        let size = sizes[index];
        let mut origin = [0.0; 2];
        origin[along] = cursor;
        origin[across] = offset(item, extent[across], size[across]);
        rects[index] = Rect {
            x: parent.x + origin[0],
            y: parent.y + origin[1],
            width: size[0],
            height: size[1],
        };
        cursor += size[along] + padding;
    }
    rects
}

/// Where a run of `length` starts inside `extent` for one alignment.
fn offset(align: Align, extent: f32, length: f32) -> f32 {
    match align {
        Align::Start => 0.0,
        Align::Center => (extent - length) * 0.5,
        Align::End => extent - length,
    }
}

/// Standard `UDim2` resolution: `extent` is the size every size modifier has
/// already had its say on, the position is a fraction of the parent plus a
/// pixel offset against the parent's own corner, and `AnchorPoint` then slides
/// the box back by that fraction of its own size — the default `(0, 0)`
/// putting the element's top-left corner on `Position`.
fn place(position: Span, extent: [f32; 2], anchor: [f32; 2], parent: &Rect) -> Rect {
    let origin = position.against(parent.size());

    Rect {
        x: parent.x + origin[0] - anchor[0] * extent[0],
        y: parent.y + origin[1] - anchor[1] * extent[1],
        width: extent[0],
        height: extent[1],
    }
}

fn painted(fill: &Fill, rect: &Rect) -> Painted {
    Painted {
        asset: fill.asset.clone(),
        tint: fill.tint,
        alpha: fill.alpha,
        repeat: match fill.tiling {
            Tiling::Stretch => [1.0, 1.0],
            // A tile bigger than the box repeats less than once, which is
            // Roblox's own behaviour: `TileSize` is a size, not a count.
            Tiling::Tile { size } => {
                let tile = size.against(rect.size());
                [ratio(rect.width, tile[0]), ratio(rect.height, tile[1])]
            }
        },
    }
}

/// How many tiles of `tile` pixels fit across `extent`. A non-positive tile
/// would repeat infinitely often, so it falls back to a single stretch.
fn ratio(extent: f32, tile: f32) -> f32 {
    if tile > 0.0 {
        extent / tile
    } else {
        1.0
    }
}
