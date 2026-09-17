//! Resolves a planned `ScreenGui` tree into flat, screen-space rectangles in
//! the order they have to be painted.
//!
//! Coordinates are pixels with the origin at the viewport's top-left corner,
//! which is the frame `UDim2` itself is written in; the renderer is what turns
//! them into clip space.

use rbx_assets::AssetRef;

use super::plan::{Align, Fill, Layout, Node, ScaleMode, Screen, Span};
use super::space::SpaceGui;
// Reaches all the way to `renderer::gui::quads::image`, unlike everything
// else `plan` hands this module — see the type's own doc comment.
pub(crate) use super::plan::PixelRect;

mod grid;
mod list;
mod modifiers;
mod sizing;
mod table;

pub(crate) use modifiers::{GradientPx, StrokePx};

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

/// An `ImageLabel`/`ImageButton`'s `ScaleType`, resolved as far as this layer
/// can: a `Tile`'s `UDim2` is already turned into a repeat count (in
/// [`Painted::repeat`]), but `Fit`/`Crop`'s letterbox and crop math need the
/// source image's own pixel size, which only the renderer's atlas knows — see
/// `renderer::gui::quads::image`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ImageScale {
    Stretch,
    Tile,
    Slice {
        center: Option<PixelRect>,
        scale: f32,
    },
    Fit,
    Crop,
}

/// An `ImageLabel`/`ImageButton`'s image, resolved as far as a pixel-space
/// `Rect` allows.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Painted {
    pub(crate) asset: AssetRef,
    pub(crate) tint: [f32; 3],
    pub(crate) alpha: f32,
    /// How many times the image repeats across the box under `Stretch`/
    /// `Tile`, 1 being a stretch; meaningless for the other scale types.
    pub(crate) repeat: [f32; 2],
    pub(crate) scale: ImageScale,
    /// `ImageRectOffset`/`ImageRectSize`, still in the source image's pixels.
    pub(crate) rect_offset: [f32; 2],
    pub(crate) rect_size: [f32; 2],
    pub(crate) pixelated: bool,
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
    /// `BorderSizePixel` and `BorderColor3`, `None` for a zero-width border
    /// — or for any rounded box, since a square outline around one is not
    /// what a `UICorner` shows; the docs say nothing about the two together.
    pub(crate) border: Option<(f32, [f32; 3])>,
    /// How far inside `rect` the border's outer edge sits, per `BorderMode`.
    pub(crate) border_inset: f32,
    /// `ZIndex`, kept so a `ZIndexBehavior.Global` screen can sort its whole
    /// flattened tree by it after the fact.
    pub(crate) z_index: i32,
    pub(crate) image: Option<Painted>,
    /// `UICorner` in pixels, top-left first then clockwise; all zero without.
    pub(crate) corner_radii: [f32; 4],
    pub(crate) stroke: Option<StrokePx>,
    pub(crate) gradient: Option<GradientPx>,
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
            None,
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
        None,
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

/// What a layout comes to: one rect per sibling in the tree's own order and
/// the extent the laid-out content covers, which is
/// `UIGridStyleLayout.AbsoluteContentSize`.
pub(crate) struct Arranged {
    pub(crate) rects: Vec<Rect>,
    /// `AbsoluteContentSize`. Nothing in this module needs it — it exists for
    /// `AutomaticSize`, which sizes a container to the content its layout
    /// came to.
    #[allow(dead_code)]
    pub(crate) size: [f32; 2],
    /// `UITableLayout` only: where each sibling's own children go, since a
    /// table lays out its cells rather than leaving them to their row.
    cells: Option<Vec<Vec<Rect>>>,
}

/// Places `nodes` inside `parent` under `layout`, or by their own `Position`
/// and `Size` where there is none.
pub(crate) fn arrange(nodes: &[Node], layout: Option<&Layout>, parent: &Rect) -> Arranged {
    match layout {
        Some(Layout::List(spec)) => {
            let (rects, size) = list::stacked(nodes, spec, parent);
            Arranged {
                rects,
                size,
                cells: None,
            }
        }
        Some(Layout::Grid(spec)) => {
            let (rects, size) = grid::grid(nodes, spec, parent);
            Arranged {
                rects,
                size,
                cells: None,
            }
        }
        Some(Layout::Table(spec)) => {
            let laid = table::table(nodes, spec, parent);
            Arranged {
                rects: laid.rects,
                size: laid.size,
                cells: Some(laid.cells),
            }
        }
        None => {
            let rects: Vec<Rect> = nodes
                .iter()
                .map(|node| {
                    place(
                        node.position,
                        sizing::extent(node, parent.size()),
                        node.anchor,
                        parent,
                    )
                })
                .collect();
            let size = content_size(&rects);
            Arranged {
                rects,
                size,
                cells: None,
            }
        }
    }
}

/// Places every sibling inside `parent`, then emits them in paint order.
///
/// The two orders are distinct: a layout decides where a sibling sits,
/// `ZIndex` decides which one is drawn over the other.
///
/// `given` is the one exception to a sibling being placed here at all: a
/// `UITableLayout` sizes its cells, which are its siblings' children, so it
/// hands them down ready-made.
fn children(
    nodes: &[Node],
    layout: Option<&Layout>,
    parent: &Rect,
    given: Option<&[Rect]>,
    context: Context,
    into: &mut Vec<Element>,
) {
    let arranged = match given {
        Some(rects) => Arranged {
            rects: rects.to_vec(),
            size: content_size(rects),
            cells: None,
        },
        None => arrange(nodes, layout, parent),
    };
    for index in sorted(nodes, context.global_z_index) {
        let cells = arranged.cells.as_ref().map(|cells| &cells[index][..]);
        emit(
            &nodes[index],
            context.carried(arranged.rects[index]),
            cells,
            context,
            into,
        );
    }
}

/// Sibling indices in the order a layout walks them: `SortOrder.Name`, or
/// `LayoutOrder` with ties in tree order — stable, so equal orders keep the
/// order they were added to the parent in.
pub(super) fn ordered(nodes: &[Node], by_name: bool) -> Vec<usize> {
    let mut order: Vec<usize> = (0..nodes.len()).collect();
    match by_name {
        true => order.sort_by(|&a, &b| nodes[a].name.cmp(&nodes[b].name)),
        false => order.sort_by_key(|&index| nodes[index].layout_order),
    }
    order
}

/// The extent a run of rects covers: `AbsoluteContentSize`, which the docs
/// describe as the space the elements take up "including any padding created
/// by the grid".
pub(super) fn content_size(rects: &[Rect]) -> [f32; 2] {
    let mut low = [f32::INFINITY; 2];
    let mut high = [f32::NEG_INFINITY; 2];
    for rect in rects {
        low[0] = low[0].min(rect.x);
        low[1] = low[1].min(rect.y);
        high[0] = high[0].max(rect.x + rect.width);
        high[1] = high[1].max(rect.y + rect.height);
    }
    match rects.is_empty() {
        true => [0.0, 0.0],
        false => [high[0] - low[0], high[1] - low[1]],
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

fn emit(
    node: &Node,
    rect: Rect,
    cells: Option<&[Rect]>,
    context: Context,
    into: &mut Vec<Element>,
) {
    let corner_radii = modifiers::radii(node.corner.as_ref(), rect.size());
    let rounded = corner_radii.iter().any(|&radius| radius > 0.0);
    into.push(Element {
        rect,
        clip: context.clip,
        rotation: context.angle + node.rotation,
        background: node.background,
        background_alpha: node.background_alpha,
        border: (node.border > 0.0 && !rounded).then_some((node.border, node.border_color)),
        border_inset: node.border_mode.inset(node.border),
        z_index: node.z_index,
        image: node.fill.as_ref().map(|fill| painted(fill, &rect)),
        corner_radii,
        stroke: node
            .stroke
            .as_ref()
            .map(|stroke| modifiers::stroke(stroke, rect.size())),
        gradient: node
            .gradient
            .as_ref()
            .map(|gradient| modifiers::gradient(gradient, rect.size())),
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
        cells,
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

/// Where a run of `length` starts inside `extent` for one alignment.
pub(super) fn offset(align: Align, extent: f32, length: f32) -> f32 {
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
    let (scale, repeat) = match fill.scale {
        ScaleMode::Stretch => (ImageScale::Stretch, [1.0, 1.0]),
        // A tile bigger than the box repeats less than once, which is
        // Roblox's own behaviour: `TileSize` is a size, not a count.
        ScaleMode::Tile { size } => {
            let tile = size.against(rect.size());
            let repeat = [ratio(rect.width, tile[0]), ratio(rect.height, tile[1])];
            (ImageScale::Tile, repeat)
        }
        ScaleMode::Slice { center, scale } => (ImageScale::Slice { center, scale }, [1.0, 1.0]),
        ScaleMode::Fit => (ImageScale::Fit, [1.0, 1.0]),
        ScaleMode::Crop => (ImageScale::Crop, [1.0, 1.0]),
    };
    Painted {
        asset: fill.asset.clone(),
        tint: fill.tint,
        alpha: fill.alpha,
        repeat,
        scale,
        rect_offset: fill.rect_offset,
        rect_size: fill.rect_size,
        pixelated: fill.pixelated,
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
